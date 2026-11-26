#pragma once

#include <atomic>
#include <bit>
#include <expected>
#include <format>
#include <span>
#include <utility>

#include <zeroshm/constants_generated.h>
#include <zeroshm/error.h>
#include <zeroshm/layout.h>

#include "memory.h"

namespace zeroshm {

template <typename T> class Queue {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");

   struct Header {
      std::atomic<uint32_t> head;
      std::atomic<uint32_t> tail;
      uint32_t capacity;
      uint32_t elem_size;
   };

   /// Compile-time validation: Queue::Header must be exactly 16 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == EXPECTED_QUEUE_HEADER_SIZE,
                 "Queue::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, head) == QUEUE_HEAD_OFFSET,
                 "Queue::Header.head must be at QUEUE_HEAD_OFFSET");
   static_assert(offsetof(Header, tail) == QUEUE_TAIL_OFFSET,
                 "Queue::Header.tail must be at QUEUE_TAIL_OFFSET");
   static_assert(offsetof(Header, capacity) == QUEUE_CAPACITY_OFFSET,
                 "Queue::Header.capacity must be at QUEUE_CAPACITY_OFFSET");
   static_assert(offsetof(Header, elem_size) == QUEUE_ELEM_SIZE_OFFSET,
                 "Queue::Header.elem_size must be at QUEUE_ELEM_SIZE_OFFSET");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Queue(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), data_(nullptr),
         sequence_(nullptr) {
   }

   /// Core initialization logic for creating a new queue.
   std::expected<void, Error> init_create(size_t capacity) {
      if (capacity == 0) {
         return std::unexpected(Error::InvalidCapacity);
      }

      // The Vyukov slot mapping (counter % capacity on monotonically
      // increasing uint32 counters) is only correct across the 2^32
      // counter wraparound when capacity divides 2^32. Round the request
      // up to the next power of two: callers get at least the capacity
      // they asked for, and capacity() reports the actual value.
      if (capacity > (SIZE_MAX >> 1) + 1) {
         return std::unexpected(Error::AllocationOverflow);
      }
      capacity = std::bit_ceil(capacity);

      // Check for overflow
      size_t seq_array_size = sizeof(std::atomic<uint32_t>) * capacity;
      if (capacity > (SIZE_MAX - sizeof(Header) - seq_array_size) / sizeof(T)) {
         return std::unexpected(Error::AllocationOverflow);
      }

      // The sequence array is 8-aligned so its atomics are naturally aligned.
      size_t seq_off = align_up(sizeof(T) * capacity, SECTION_ALIGN);
      size_t total_size = sizeof(Header) + seq_off + seq_array_size;
      auto alloc_result = memory_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header. Atomics (head/tail) are hardware-endian aware;
      // non-atomic fields are stored little-endian (C++23 std::byteswap
      // converts on big-endian platforms; no-op on little-endian).
      header_->head.store(0, std::memory_order_relaxed);
      header_->tail.store(0, std::memory_order_relaxed);
      header_->capacity = to_le(static_cast<uint32_t>(capacity));
      header_->elem_size = to_le(static_cast<uint32_t>(sizeof(T)));

      data_ = reinterpret_cast<T *>(reinterpret_cast<char *>(header_) +
                                    sizeof(Header));

      // Sequence array lives after the data array (8-aligned)
      sequence_ = reinterpret_cast<std::atomic<uint32_t> *>(
          reinterpret_cast<char *>(data_) + seq_off);

      // Initialize per-slot sequence numbers: sequence[i] = i
      for (size_t i = 0; i < capacity; i++) {
         sequence_[i].store(static_cast<uint32_t>(i),
                            std::memory_order_relaxed);
      }

      return {};
   }

   /// Core initialization logic for opening an existing queue.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      // Read non-atomic fields from little-endian storage (C++23 std::byteswap
      // converts to native representation on big-endian platforms)
      uint32_t elem_size = from_le(header_->elem_size);
      uint32_t cap = from_le(header_->capacity);

      if (elem_size != sizeof(T)) {
         return std::unexpected(Error::TypeMismatch);
      }

      // Wrap-safety requires a power-of-two capacity (see the create
      // constructor). A non-power-of-two value means the segment was
      // written by a pre-amendment implementation; using it risks
      // corruption at counter wraparound.
      if ((cap & (cap - 1)) != 0) {
         return std::unexpected(Error::InvalidCapacity);
      }

      data_ = reinterpret_cast<T *>(reinterpret_cast<char *>(header_) +
                                    sizeof(Header));

      // Sequence array lives after the data array (8-aligned)
      sequence_ = reinterpret_cast<std::atomic<uint32_t> *>(
          reinterpret_cast<char *>(data_) + align_up(sizeof(T) * cap, 8));

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_;
   T *data_;
   std::atomic<uint32_t> *sequence_;

   inline std::pair<uint32_t, uint32_t> load_head_tail() const {
      return {header_->head.load(std::memory_order_acquire),
              header_->tail.load(std::memory_order_acquire)};
   }

 public:
   /// Create a new queue in shared memory.
   /// Returns Error::InvalidCapacity if capacity is 0 or too large,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Queue, Error>
   create(Memory &memory, std::string_view name, size_t capacity) {
      Queue q(memory, name, FactoryTag{});
      auto result = q.init_create(capacity);
      if (!result)
         return std::unexpected(result.error());
      return q;
   }

   /// Open an existing queue in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match,
   /// Error::InvalidCapacity if the stored capacity is not a power of two.
   static std::expected<Queue, Error> open(Memory &memory,
                                           std::string_view name) {
      Queue q(memory, name, FactoryTag{});
      auto result = q.init_open();
      if (!result)
         return std::unexpected(result.error());
      return q;
   }

   // Create new queue
   Queue(Memory &memory, std::string_view name, size_t capacity)
       : Queue(memory, name, FactoryTag{}) {

      auto result = init_create(capacity);
      if (!result) {
         switch (result.error()) {
         case Error::InvalidCapacity:
            throw std::invalid_argument(
                "Queue capacity must be greater than 0");
         case Error::AllocationOverflow:
            throw std::overflow_error("Queue capacity too large");
         default:
            throw std::runtime_error(std::format(
                "Failed to allocate queue: {}", error_message(result.error())));
         }
      }
   }

   // Open existing queue
   Queue(Memory &memory, std::string_view name)
       : Queue(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(std::format("Queue not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Type size mismatch");
         case Error::InvalidCapacity:
            throw std::runtime_error("Queue capacity is not a power of two "
                                     "(created by an old implementation?)");
         default:
            throw std::runtime_error(std::format(
                "Queue::open failed: {}", error_message(result.error())));
         }
      }
   }

   // Enqueue (lock-free MPMC, Vyukov-style bounded queue)
   // Returns std::expected<void, Error> -- Error::QueueFull if queue is full
   [[nodiscard]] std::expected<void, Error> push(const T &value) {
      const uint32_t cap = from_le(header_->capacity);

      for (;;) {
         uint32_t tail = header_->tail.load(std::memory_order_relaxed);
         uint32_t slot = tail % cap;
         uint32_t seq = sequence_[slot].load(std::memory_order_acquire);
         auto diff = static_cast<int32_t>(seq) - static_cast<int32_t>(tail);

         if (diff == 0) {
            // Slot is ready for writing; try to claim it
            if (header_->tail.compare_exchange_weak(
                    tail, tail + 1, std::memory_order_relaxed,
                    std::memory_order_relaxed)) {
               // We own this slot - write the data
               data_[slot] = value;
               // Publish: set sequence to tail + 1 so consumers can see it
               sequence_[slot].store(tail + 1, std::memory_order_release);
               return {};
               // std::unreachable: this branch always returns, the retry
               // path below is unreachable when CAS succeeds. Helps the
               // compiler eliminate dead branches in the spin loop.
               std::unreachable();
            }
            // CAS failed, another producer got it; retry
         } else if (diff < 0) {
            // Queue is full
            return std::unexpected(Error::QueueFull);
         }
         // diff > 0: another producer claimed this slot but hasn't
         // finished yet, or we read a stale tail; retry
      }
   }

   // Dequeue (lock-free MPMC, Vyukov-style bounded queue)
   // Returns std::expected<T, Error> -- Error::QueueEmpty if queue is empty
   [[nodiscard]] std::expected<T, Error> pop() {
      const uint32_t cap = from_le(header_->capacity);

      for (;;) {
         uint32_t head = header_->head.load(std::memory_order_relaxed);
         uint32_t slot = head % cap;
         uint32_t seq = sequence_[slot].load(std::memory_order_acquire);
         auto diff = static_cast<int32_t>(seq) - static_cast<int32_t>(head + 1);

         if (diff == 0) {
            // Slot contains data; try to claim it
            if (header_->head.compare_exchange_weak(
                    head, head + 1, std::memory_order_relaxed,
                    std::memory_order_relaxed)) {
               // We own this slot - read the data
               T value = data_[slot];
               // Release: set sequence to head + capacity so producers
               // can reuse this slot on the next wrap-around
               sequence_[slot].store(head + cap, std::memory_order_release);
               return value;
               // std::unreachable: this branch always returns, the retry
               // path below is unreachable when CAS succeeds. Helps the
               // compiler eliminate dead branches in the spin loop.
               std::unreachable();
            }
            // CAS failed, another consumer got it; retry
         } else if (diff < 0) {
            // Queue is empty
            return std::unexpected(Error::QueueEmpty);
         }
         // diff > 0: another consumer claimed this slot but hasn't
         // finished yet, or we read a stale head; retry
      }
   }

   // Check if empty (approximate in concurrent context)
   bool empty() const {
      auto [head, tail] = load_head_tail();
      return head == tail;
   }

   // Check if full (approximate in concurrent context)
   bool full() const {
      auto [head, tail] = load_head_tail();
      return (tail - head) >= from_le(header_->capacity);
   }

   // Get current size (approximate in concurrent context)
   size_t size() const {
      auto [head, tail] = load_head_tail();
      // uint32_t subtraction handles wraparound correctly
      return static_cast<size_t>(tail - head);
   }

   size_t capacity() const {
      return from_le(header_->capacity);
   }

   /// Zero-copy, bounds-aware view of the ring buffer elements.
   /// Returns a span covering the entire capacity (including empty slots).
   [[nodiscard]] std::span<T> data() {
      return std::span<T>(data_, from_le(header_->capacity));
   }

   [[nodiscard]] std::span<const T> data() const {
      return std::span<const T>(data_, from_le(header_->capacity));
   }

   /// Raw pointer to the ring buffer (backward compatibility).
   [[nodiscard]] T *raw_data() {
      return data_;
   }
   [[nodiscard]] const T *raw_data() const {
      return data_;
   }

   /**
    * @brief Destroy the queue and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused. The underlying
    * memory is part of the mmap'd region and is not freed.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
