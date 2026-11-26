#pragma once

#include <atomic>
#include <cstddef>
#include <expected>
#include <format>
#include <span>
#include <thread>

#include <zeroshm/constants_generated.h>

#include "detail/hash.h"
#include "error.h"
#include "memory.h"

namespace zeroshm {

template <typename T> class Set {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");

   struct Entry {
      std::atomic<uint32_t>
          state;   // 0=empty, 1=occupied, 2=deleted, 3=inserting
      T value;
   };

   struct Header {
      std::atomic<uint32_t> size;   // Current number of elements
      uint32_t capacity;            // Total capacity
      uint32_t elem_size;
      uint32_t
          reserved;   // pads header to 16 bytes so Entry array is 8-aligned
   };

   /// Compile-time validation: Set::Header must be exactly 16 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 16,
                 "Set::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, size) == SET_SIZE_OFFSET,
                 "Set::Header.size must be at SET_SIZE_OFFSET");
   static_assert(offsetof(Header, capacity) == SET_CAPACITY_OFFSET,
                 "Set::Header.capacity must be at SET_CAPACITY_OFFSET");
   static_assert(offsetof(Header, elem_size) == SET_ELEM_SIZE_OFFSET,
                 "Set::Header.elem_size must be at SET_ELEM_SIZE_OFFSET");
   static_assert(offsetof(Header, reserved) == SET_RESERVED_OFFSET,
                 "Set::Header.reserved must be at SET_RESERVED_OFFSET");

   // State values for entries
   static constexpr uint32_t EMPTY = 0;
   static constexpr uint32_t OCCUPIED = 1;
   static constexpr uint32_t DELETED = 2;
   static constexpr uint32_t INSERTING = 3;

   // Bound on waiting for a slot stuck in INSERTING (a crashed peer can
   // leave it that way forever). Matches Stack/Queue/Map MAX_SPINS.
   static constexpr int MAX_SPINS = 10000;

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Set(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), entries_(nullptr) {
   }

   /// Core initialization logic for creating a new set.
   std::expected<void, Error> init_create(size_t capacity) {
      if (capacity == 0) {
         return std::unexpected(Error::InvalidCapacity);
      }

      // Check for overflow
      if (capacity > (SIZE_MAX - sizeof(Header)) / sizeof(Entry)) {
         return std::unexpected(Error::AllocationOverflow);
      }

      size_t total_size = sizeof(Header) + sizeof(Entry) * capacity;
      auto alloc_result = memory_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->size.store(0, std::memory_order_relaxed);
      header_->capacity = capacity;
      header_->elem_size = sizeof(T);
      header_->reserved = 0;

      entries_ = reinterpret_cast<Entry *>(reinterpret_cast<char *>(header_) +
                                           sizeof(Header));

      // Initialize all entries as empty
      for (size_t i = 0; i < capacity; ++i) {
         entries_[i].state.store(EMPTY, std::memory_order_relaxed);
      }

      return {};
   }

   /// Core initialization logic for opening an existing set.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      if (header_->elem_size != sizeof(T)) {
         return std::unexpected(Error::TypeMismatch);
      }

      entries_ = reinterpret_cast<Entry *>(reinterpret_cast<char *>(header_) +
                                           sizeof(Header));

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_ = nullptr;
   Entry *entries_ = nullptr;

   size_t hash_value(const T &value) const {
      return detail::trivial_hash(value);
   }
   bool values_equal(const T &a, const T &b) const {
      return detail::trivial_equal(a, b);
   }

 public:
   /// Create a new set in shared memory.
   /// Returns Error::InvalidCapacity if capacity is 0,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Set, Error>
   create(Memory &memory, std::string_view name, size_t capacity) {
      Set s(memory, name, FactoryTag{});
      auto result = s.init_create(capacity);
      if (!result)
         return std::unexpected(result.error());
      return s;
   }

   /// Open an existing set in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match.
   static std::expected<Set, Error> open(Memory &memory,
                                         std::string_view name) {
      Set s(memory, name, FactoryTag{});
      auto result = s.init_open();
      if (!result)
         return std::unexpected(result.error());
      return s;
   }

   // Create new set
   Set(Memory &memory, std::string_view name, size_t capacity)
       : Set(memory, name, FactoryTag{}) {

      auto result = init_create(capacity);
      if (!result) {
         throw std::runtime_error(std::format("Set::create failed: {}",
                                              error_message(result.error())));
      }
   }

   // Open existing set
   Set(Memory &memory, std::string_view name)
       : Set(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         throw std::runtime_error(std::format("Set::open failed: {}",
                                              error_message(result.error())));
      }
   }

   // Insert element (lock-free with linear probing)
   [[nodiscard]] std::expected<void, Error> insert(const T &value) {
      size_t hash = hash_value(value);
      size_t capacity = header_->capacity;

      // Two-phase insert. Phase 1 scans the whole probe chain for the
      // value (returning false if present), remembering the first
      // reusable DELETED slot; the chain ends at the first EMPTY slot.
      // Phase 2 claims the remembered slot. Claiming before the chain
      // is fully scanned would duplicate a value that lives past a
      // DELETED slot, and advancing past a slot whose CAS we lost would
      // duplicate a value a concurrent insert is writing to it -- both
      // paths must re-examine, never skip.
      for (;;) {
         Entry *deleted_target = nullptr;   // first reusable slot
         Entry *empty_target = nullptr;     // chain-terminating slot

         for (size_t i = 0; i < capacity && !empty_target; ++i) {
            Entry &entry = entries_[(hash + i) % capacity];

            int spins = 0;
            for (;;) {
               uint32_t state = entry.state.load(std::memory_order_acquire);

               if (state == INSERTING) {
                  // Mid-write by another thread; it may be writing
                  // this value. Wait bounded (a crashed peer can
                  // leave the slot stuck forever), then skip it.
                  if (++spins >= MAX_SPINS)
                     break;
                  std::this_thread::yield();
                  continue;
               }

               if (state == OCCUPIED) {
                  if (values_equal(entry.value, value)) {
                     return std::expected<void,
                                          Error>{};   // Already exists, treat
                                                      // as success (idempotent)
                  }
                  break;   // different value: next slot
               }

               if (state == DELETED) {
                  if (!deleted_target)
                     deleted_target = &entry;
                  break;   // keep scanning: the value may live further on
               }

               // EMPTY: the probe chain ends here; the value is absent
               empty_target = &entry;
               break;
            }
         }

         Entry *target = deleted_target ? deleted_target : empty_target;
         if (!target)
            break;   // set is full

         uint32_t expected = deleted_target ? DELETED : EMPTY;
         if (target->state.compare_exchange_strong(expected, INSERTING,
                                                   std::memory_order_acquire,
                                                   std::memory_order_relaxed)) {
            // We exclusively own this slot; write the value
            target->value = value;
            // Publish the entry: INSERTING -> OCCUPIED (release so readers see
            // data)
            target->state.store(OCCUPIED, std::memory_order_release);
            header_->size.fetch_add(1, std::memory_order_relaxed);
            return std::expected<void, Error>{};
         }
         // The slot changed under us -- a competing operation completed
         // (possibly inserting this very value). Rescan from the top.
      }

      return std::unexpected(Error::SetFull);   // Set is full
   }

   // Check if element exists
   [[nodiscard]] bool contains(const T &value) const {
      size_t hash = hash_value(value);
      size_t capacity = header_->capacity;

      for (size_t i = 0; i < capacity; ++i) {
         size_t idx = (hash + i) % capacity;
         const Entry &entry = entries_[idx];

         uint32_t state = entry.state.load(std::memory_order_acquire);

         if (state == EMPTY) {
            return false;   // Not found
         }

         if (state == OCCUPIED && values_equal(entry.value, value)) {
            return true;
         }

         // Continue searching through DELETED and INSERTING entries
         // INSERTING slots are being written by another thread; skip them
      }

      return false;
   }

   // Remove element
   [[nodiscard]] std::expected<void, Error> erase(const T &value) {
      size_t hash = hash_value(value);
      size_t capacity = header_->capacity;

      for (size_t i = 0; i < capacity; ++i) {
         size_t idx = (hash + i) % capacity;
         Entry &entry = entries_[idx];

         uint32_t state = entry.state.load(std::memory_order_acquire);

         if (state == EMPTY) {
            return std::unexpected(Error::SetNotFound);   // Not found
         }

         if (state == OCCUPIED && values_equal(entry.value, value)) {
            // CAS from OCCUPIED to DELETED; only the winner decrements size
            uint32_t expected = OCCUPIED;
            if (entry.state.compare_exchange_strong(
                    expected, DELETED, std::memory_order_release,
                    std::memory_order_relaxed)) {
               header_->size.fetch_sub(1, std::memory_order_relaxed);
               return std::expected<void, Error>{};
            }
            // CAS failed: another thread already erased or is modifying this
            // slot
            if (expected == DELETED) {
               return std::unexpected(Error::SetNotFound);
            }
            // Otherwise (e.g., INSERTING), continue probing
         }

         // Continue searching through DELETED and INSERTING entries
      }

      return std::unexpected(Error::SetNotFound);
   }

   // Get current size
   [[nodiscard]] size_t size() const {
      return header_->size.load(std::memory_order_relaxed);
   }

   // Get capacity
   [[nodiscard]] size_t capacity() const {
      return header_->capacity;
   }

   // Check if empty
   [[nodiscard]] bool empty() const {
      return size() == 0;
   }

   // Clear all elements (not thread-safe with concurrent operations)
   void clear() {
      for (size_t i = 0; i < header_->capacity; ++i) {
         entries_[i].state.store(EMPTY, std::memory_order_relaxed);
      }
      header_->size.store(0, std::memory_order_relaxed);
   }

   /// Zero-copy view of all entries in the set (including empty/tombstone
   /// slots). Each Entry contains {state, value}.
   [[nodiscard]] std::span<const Entry> entries() const {
      return std::span<const Entry>(entries_, header_->capacity);
   }

   /**
    * @brief Destroy the set and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
