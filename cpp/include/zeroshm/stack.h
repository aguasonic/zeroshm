#pragma once

#include <atomic>
#include <cstddef>
#include <expected>
#include <format>
#include <thread>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"

namespace zeroshm {

template <typename T> class Stack {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");

   struct Header {
      std::atomic<int32_t> top;   // -1 when empty
      uint32_t capacity;
      uint32_t elem_size;
      uint32_t
          reserved;   // pads header to 16 bytes so the data array is 8-aligned
   };

   /// Compile-time validation: Stack::Header must be exactly 16 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 16,
                 "Stack::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, top) == STACK_TOP_OFFSET,
                 "Stack::Header.top must be at STACK_TOP_OFFSET");
   static_assert(offsetof(Header, capacity) == STACK_CAPACITY_OFFSET,
                 "Stack::Header.capacity must be at STACK_CAPACITY_OFFSET");
   static_assert(offsetof(Header, elem_size) == STACK_ELEM_SIZE_OFFSET,
                 "Stack::Header.elem_size must be at STACK_ELEM_SIZE_OFFSET");
   static_assert(offsetof(Header, reserved) == STACK_RESERVED_OFFSET,
                 "Stack::Header.reserved must be at STACK_RESERVED_OFFSET");

   // Per-slot states for the 4-state CAS protocol:
   //   EMPTY(0)  -> WRITING(1) -> READY(2) -> READING(3) -> EMPTY(0)
   // Push: CAS(EMPTY -> WRITING), write data, store(READY)
   // Pop:  CAS(READY -> READING), read data, store(EMPTY)
   static constexpr uint32_t SLOT_EMPTY = 0;
   static constexpr uint32_t SLOT_WRITING = 1;
   static constexpr uint32_t SLOT_READY = 2;
   static constexpr uint32_t SLOT_READING = 3;

   // Bound on every slot-state spin loop. A peer that crashes mid-operation
   // leaves its slot permanently claimed; an unbounded spin would then hang
   // every later operation that lands on that slot. Bailing out makes
   // push/pop/top best-effort: they may fail spuriously if a peer died (or
   // under pathological contention), but they never hang. On bail-out the
   // operation undoes its top reservation when possible and returns failure.
   static constexpr int MAX_SPINS = 10000;

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Stack(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), data_(nullptr),
         state_(nullptr) {
   }

   /// Core initialization logic for creating a new stack.
   std::expected<void, Error> init_create(size_t capacity) {
      // Layout: [Header(16)][data: T*capacity][pad][state:
      // atomic<uint32_t>*capacity] The state array is 8-aligned so its atomics
      // are always naturally aligned.
      size_t state_off = align_up(sizeof(T) * capacity, SECTION_ALIGN);
      size_t total_size =
          sizeof(Header) + state_off + sizeof(std::atomic<uint32_t>) * capacity;
      auto alloc_result = memory_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->top.store(-1, std::memory_order_relaxed);
      header_->capacity = capacity;
      header_->elem_size = sizeof(T);
      header_->reserved = 0;

      data_ = reinterpret_cast<T *>(reinterpret_cast<char *>(header_) +
                                    sizeof(Header));

      state_ = reinterpret_cast<std::atomic<uint32_t> *>(
          reinterpret_cast<char *>(data_) + state_off);

      // Initialize all slot states to EMPTY
      for (size_t i = 0; i < capacity; ++i) {
         state_[i].store(SLOT_EMPTY, std::memory_order_relaxed);
      }
      std::atomic_thread_fence(std::memory_order_release);

      return {};
   }

   /// Core initialization logic for opening an existing stack.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      if (header_->elem_size != sizeof(T)) {
         return std::unexpected(Error::TypeMismatch);
      }

      data_ = reinterpret_cast<T *>(reinterpret_cast<char *>(header_) +
                                    sizeof(Header));

      state_ = reinterpret_cast<std::atomic<uint32_t> *>(
          reinterpret_cast<char *>(data_) +
          align_up(sizeof(T) * header_->capacity, 8));

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_;
   T *data_;
   std::atomic<uint32_t> *state_;

   inline int32_t load_top() const {
      return header_->top.load(std::memory_order_acquire);
   }

 public:
   /// Create a new stack in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Stack, Error>
   create(Memory &memory, std::string_view name, size_t capacity) {
      Stack s(memory, name, FactoryTag{});
      auto result = s.init_create(capacity);
      if (!result)
         return std::unexpected(result.error());
      return s;
   }

   /// Open an existing stack in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match.
   static std::expected<Stack, Error> open(Memory &memory,
                                           std::string_view name) {
      Stack s(memory, name, FactoryTag{});
      auto result = s.init_open();
      if (!result)
         return std::unexpected(result.error());
      return s;
   }

   // Create new stack
   Stack(Memory &memory, std::string_view name, size_t capacity)
       : Stack(memory, name, FactoryTag{}) {

      auto result = init_create(capacity);
      if (!result) {
         throw std::runtime_error(std::format("Stack::create failed: {}",
                                              error_message(result.error())));
      }
   }

   // Open existing stack
   Stack(Memory &memory, std::string_view name)
       : Stack(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         throw std::runtime_error(std::format("Stack::open failed: {}",
                                              error_message(result.error())));
      }
   }

   // Push (lock-free with per-slot CAS)
   [[nodiscard]] std::expected<void, Error> push(const T &value) {
      int32_t current_top, new_top;

      // Step 1: Reserve a slot atomically by CAS-advancing top
      do {
         current_top = header_->top.load(std::memory_order_relaxed);

         // Check if full
         if (current_top >= static_cast<int32_t>(header_->capacity - 1)) {
            return std::unexpected(Error::StackFull);
         }

         new_top = current_top + 1;
      } while (!header_->top.compare_exchange_weak(current_top, new_top,
                                                   std::memory_order_acq_rel,
                                                   std::memory_order_relaxed));

      // Step 2: Exclusively claim the slot for writing: CAS(EMPTY -> WRITING).
      // Bounded spin (see MAX_SPINS): a crashed peer can leave the slot
      // permanently claimed.
      uint32_t expected = SLOT_EMPTY;
      bool claimed = false;
      for (int spins = 0; spins < MAX_SPINS; ++spins) {
         if (state_[new_top].compare_exchange_weak(expected, SLOT_WRITING,
                                                   std::memory_order_acq_rel,
                                                   std::memory_order_relaxed)) {
            claimed = true;
            break;
         }
         expected = SLOT_EMPTY;
         std::this_thread::yield();
      }
      if (!claimed) {
         // Best-effort undo of the reservation. If another push already
         // built on top of ours, the CAS fails and top stays advanced
         // over the stuck slot; operations landing on that slot also
         // fail bounded rather than hang.
         int32_t reserved = new_top;
         header_->top.compare_exchange_strong(reserved, current_top,
                                              std::memory_order_acq_rel,
                                              std::memory_order_relaxed);
         return std::unexpected(Error::StackFull);
      }

      // Step 3: Write data (we have exclusive ownership)
      data_[new_top] = value;

      // Step 4: Publish data: WRITING -> READY
      state_[new_top].store(SLOT_READY, std::memory_order_release);

      return std::expected<void, Error>{};
   }

   // Pop (lock-free with per-slot CAS)
   [[nodiscard]] std::expected<T, Error> pop() {
      int32_t current_top, new_top;

      // Step 1: Reserve a slot to read by CAS-decrementing top
      do {
         current_top = header_->top.load(std::memory_order_relaxed);

         // Check if empty
         if (current_top < 0) {
            return std::unexpected(Error::StackUnderflow);
         }

         new_top = current_top - 1;
      } while (!header_->top.compare_exchange_weak(current_top, new_top,
                                                   std::memory_order_acq_rel,
                                                   std::memory_order_relaxed));

      // Step 2: Exclusively claim the slot for reading: CAS(READY -> READING).
      // Bounded spin (see MAX_SPINS): normally this only waits for the
      // owning push to finish writing, but a pusher that crashed mid-write
      // would leave the slot stuck in WRITING forever.
      uint32_t expected = SLOT_READY;
      bool claimed = false;
      for (int spins = 0; spins < MAX_SPINS; ++spins) {
         if (state_[current_top].compare_exchange_weak(
                 expected, SLOT_READING, std::memory_order_acq_rel,
                 std::memory_order_relaxed)) {
            claimed = true;
            break;
         }
         expected = SLOT_READY;
         std::this_thread::yield();
      }
      if (!claimed) {
         // Best-effort undo: put the item back under top so it is not
         // silently dropped. If another operation moved top meanwhile,
         // the CAS fails and the stuck slot stays orphaned.
         int32_t reserved = new_top;
         header_->top.compare_exchange_strong(reserved, current_top,
                                              std::memory_order_acq_rel,
                                              std::memory_order_relaxed);
         return std::unexpected(Error::StackUnderflow);
      }

      // Step 3: Read the value (we have exclusive ownership)
      T value = data_[current_top];

      // Step 4: Release slot: READING -> EMPTY
      state_[current_top].store(SLOT_EMPTY, std::memory_order_release);

      return value;
   }

   // Peek at top without removing.
   //
   // A peek cannot passively read the slot: between observing READY and
   // reading the payload, a concurrent pop could recycle the slot and a new
   // push begin overwriting it, racing the read (a TOCTOU data race). So we
   // claim the slot exclusively through the same state machine pop uses
   // (READY -> READING), copy the value, then restore it to READY. The copy
   // therefore happens under exclusive ownership and never races push/pop.
   // The bounded spin preserves crash-safety: a peer that died mid-write
   // (slot stuck WRITING/READING) cannot hang the peek indefinitely.
   [[nodiscard]] std::expected<T, Error> top() const {
      for (int spins = 0; spins < MAX_SPINS; ++spins) {
         int32_t current_top = header_->top.load(std::memory_order_acquire);
         if (current_top < 0) {
            return std::unexpected(Error::StackUnderflow);
         }
         // Try to claim the top slot exclusively for reading.
         uint32_t expected = SLOT_READY;
         if (state_[current_top].compare_exchange_strong(
                 expected, SLOT_READING, std::memory_order_acq_rel,
                 std::memory_order_relaxed)) {
            T value = data_[current_top];           // exclusive ownership
            state_[current_top].store(SLOT_READY,   // hand the slot back
                                      std::memory_order_release);
            return value;
         }
         // Slot busy (push writing, pop reading, or already consumed):
         // retry until it settles or the top moves on.
         std::this_thread::yield();
      }
      return std::unexpected(Error::StackUnderflow);
   }

   // Check if empty
   bool empty() const {
      return load_top() < 0;
   }

   // Check if full
   bool full() const {
      return load_top() >= static_cast<int32_t>(header_->capacity - 1);
   }

   // Get current size
   size_t size() const {
      int32_t top = load_top();
      return top < 0 ? 0 : static_cast<size_t>(top + 1);
   }

   size_t capacity() const {
      return header_->capacity;
   }

   /**
    * @brief Destroy the stack and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
