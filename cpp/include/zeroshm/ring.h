#pragma once

#include <atomic>
#include <cstring>
#include <expected>
#include <format>
#include <span>

#include <zeroshm/constants_generated.h>
#include <zeroshm/layout.h>

#include "error.h"
#include "memory.h"

namespace zeroshm {

// Ring buffer for streaming (MPSC -- multiple-producers / single-consumer).
// write() uses CAS on the write position so multiple producers can safely
// write concurrently. read() is single-consumer only and uses plain load/store
// on the read position. Concurrent consumers will race and corrupt data.
// The header capacity field stores BYTES (a multiple of elem_size); this
// is the canonical layout mirrored across all language implementations.
template <typename T> class Ring {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");

   struct Header {
      std::atomic<uint64_t> write_pos;   // Total bytes written
      std::atomic<uint64_t> read_pos;    // Total bytes read
      uint32_t capacity;                 // Ring buffer capacity in bytes
      uint32_t elem_size;
   };

   /// Compile-time validation: Ring::Header must be exactly 24 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == EXPECTED_RING_HEADER_SIZE,
                 "Ring::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, write_pos) == RING_WRITE_POS_OFFSET,
                 "Ring::Header.write_pos must be at RING_WRITE_POS_OFFSET");
   static_assert(offsetof(Header, read_pos) == RING_READ_POS_OFFSET,
                 "Ring::Header.read_pos must be at RING_READ_POS_OFFSET");
   static_assert(offsetof(Header, capacity) == RING_CAPACITY_OFFSET,
                 "Ring::Header.capacity must be at RING_CAPACITY_OFFSET");
   static_assert(offsetof(Header, elem_size) == RING_ELEM_SIZE_OFFSET,
                 "Ring::Header.elem_size must be at RING_ELEM_SIZE_OFFSET");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Ring(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), buffer_(nullptr) {
   }

   /// Core initialization logic for creating a new ring buffer.
   std::expected<void, Error> init_create(size_t capacity) {
      if (capacity == 0) {
         return std::unexpected(Error::InvalidCapacity);
      }

      // Ensure capacity is a multiple of element size for alignment
      capacity = (capacity / sizeof(T)) * sizeof(T);
      if (capacity == 0) {
         capacity = sizeof(T);
      }

      // Check for overflow
      if (capacity > SIZE_MAX - sizeof(Header)) {
         return std::unexpected(Error::AllocationOverflow);
      }

      size_t total_size = sizeof(Header) + capacity;
      auto alloc_result = memory_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->write_pos.store(0, std::memory_order_relaxed);
      header_->read_pos.store(0, std::memory_order_relaxed);
      header_->capacity = capacity;
      header_->elem_size = sizeof(T);

      buffer_ = reinterpret_cast<char *>(header_) + sizeof(Header);

      return {};
   }

   /// Core initialization logic for opening an existing ring buffer.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      if (header_->elem_size != sizeof(T)) {
         return std::unexpected(Error::TypeMismatch);
      }

      buffer_ = reinterpret_cast<char *>(header_) + sizeof(Header);

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_ = nullptr;
   char *buffer_ = nullptr;

   inline void ring_memcpy_write(const void *src, uint64_t write_pos,
                                 size_t bytes) {
      size_t offset = write_pos % header_->capacity;
      if (offset + bytes <= header_->capacity) {
         std::memcpy(buffer_ + offset, src, bytes);
      } else {
         size_t first = header_->capacity - offset;
         std::memcpy(buffer_ + offset, src, first);
         std::memcpy(buffer_, reinterpret_cast<const char *>(src) + first,
                     bytes - first);
      }
   }

   inline void ring_memcpy_read(void *dst, uint64_t read_pos, size_t bytes) {
      size_t offset = read_pos % header_->capacity;
      if (offset + bytes <= header_->capacity) {
         std::memcpy(dst, buffer_ + offset, bytes);
      } else {
         size_t first = header_->capacity - offset;
         std::memcpy(dst, buffer_ + offset, first);
         std::memcpy(reinterpret_cast<char *>(dst) + first, buffer_,
                     bytes - first);
      }
   }

 public:
   /// Create a new ring buffer in shared memory.
   /// Returns Error::InvalidCapacity if capacity is 0,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Ring, Error>
   create(Memory &memory, std::string_view name, size_t capacity) {
      Ring r(memory, name, FactoryTag{});
      auto result = r.init_create(capacity);
      if (!result)
         return std::unexpected(result.error());
      return r;
   }

   /// Open an existing ring buffer in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match.
   static std::expected<Ring, Error> open(Memory &memory,
                                          std::string_view name) {
      Ring r(memory, name, FactoryTag{});
      auto result = r.init_open();
      if (!result)
         return std::unexpected(result.error());
      return r;
   }

   // Create new ring buffer
   Ring(Memory &memory, std::string_view name, size_t capacity)
       : Ring(memory, name, FactoryTag{}) {

      auto result = init_create(capacity);
      if (!result) {
         throw std::runtime_error(std::format("Ring::create failed: {}",
                                              error_message(result.error())));
      }
   }

   // Open existing ring buffer
   Ring(Memory &memory, std::string_view name)
       : Ring(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         throw std::runtime_error(std::format("Ring::open failed: {}",
                                              error_message(result.error())));
      }
   }

   // Write data to ring buffer (lock-free MPSC -- multiple producers safe via
   // CAS)
   [[nodiscard]] std::expected<void, Error> write(const T &value) {
      while (true) {
         uint64_t write_pos =
             header_->write_pos.load(std::memory_order_relaxed);
         uint64_t read_pos = header_->read_pos.load(std::memory_order_acquire);

         // Check if there's space
         if (write_pos - read_pos + sizeof(T) > header_->capacity) {
            return std::unexpected(Error::RingFull);   // Buffer full
         }

         // Write the data to the claimed slot
         ring_memcpy_write(&value, write_pos, sizeof(T));

         // Try to claim the slot with CAS; retry if another producer won
         if (header_->write_pos.compare_exchange_strong(
                 write_pos, write_pos + sizeof(T), std::memory_order_release,
                 std::memory_order_relaxed)) {
            return std::expected<void, Error>{};
         }
      }
   }

   // Read data from ring buffer (lock-free SPSC optimized)
   [[nodiscard]] std::expected<T, Error> read() {
      uint64_t read_pos = header_->read_pos.load(std::memory_order_relaxed);
      uint64_t write_pos = header_->write_pos.load(std::memory_order_acquire);

      // Check if there's data
      if (read_pos + sizeof(T) > write_pos) {
         return std::unexpected(Error::RingEmpty);   // Buffer empty
      }

      // Read the data
      T value;
      ring_memcpy_read(&value, read_pos, sizeof(T));

      // Update read position
      header_->read_pos.store(read_pos + sizeof(T), std::memory_order_release);
      return value;
   }

   // Write multiple elements (more efficient for bulk operations, MPSC-safe via
   // CAS)
   [[nodiscard]] size_t write_bulk(const T *data, size_t count) {
      while (true) {
         uint64_t write_pos =
             header_->write_pos.load(std::memory_order_relaxed);
         uint64_t read_pos = header_->read_pos.load(std::memory_order_acquire);

         // Calculate how many we can write
         uint64_t available = header_->capacity - (write_pos - read_pos);
         size_t to_write =
             std::min(count, static_cast<size_t>(available / sizeof(T)));

         if (to_write == 0)
            return 0;

         size_t bytes_to_write = to_write * sizeof(T);

         ring_memcpy_write(data, write_pos, bytes_to_write);

         // Try to claim the slots with CAS; retry if another producer won
         if (header_->write_pos.compare_exchange_strong(
                 write_pos, write_pos + bytes_to_write,
                 std::memory_order_release, std::memory_order_relaxed)) {
            return to_write;
         }
      }
   }

   // Read multiple elements
   [[nodiscard]] size_t read_bulk(T *data, size_t count) {
      uint64_t read_pos = header_->read_pos.load(std::memory_order_relaxed);
      uint64_t write_pos = header_->write_pos.load(std::memory_order_acquire);

      // Calculate how many we can read
      uint64_t available = write_pos - read_pos;
      size_t to_read =
          std::min(count, static_cast<size_t>(available / sizeof(T)));

      if (to_read == 0)
         return 0;

      size_t bytes_to_read = to_read * sizeof(T);

      ring_memcpy_read(data, read_pos, bytes_to_read);

      // Update read position
      header_->read_pos.store(read_pos + bytes_to_read,
                              std::memory_order_release);
      return to_read;
   }

   // Get number of elements available to read
   [[nodiscard]] size_t available() const {
      uint64_t read_pos = header_->read_pos.load(std::memory_order_relaxed);
      uint64_t write_pos = header_->write_pos.load(std::memory_order_acquire);
      return (write_pos - read_pos) / sizeof(T);
   }

   // Get free space in elements
   [[nodiscard]] size_t free_space() const {
      uint64_t read_pos = header_->read_pos.load(std::memory_order_acquire);
      uint64_t write_pos = header_->write_pos.load(std::memory_order_relaxed);
      return (header_->capacity - (write_pos - read_pos)) / sizeof(T);
   }

   // Get capacity in elements
   [[nodiscard]] size_t capacity() const {
      return header_->capacity / sizeof(T);
   }

   // Check if empty
   [[nodiscard]] bool empty() const {
      uint64_t read_pos = header_->read_pos.load(std::memory_order_relaxed);
      uint64_t write_pos = header_->write_pos.load(std::memory_order_acquire);
      return read_pos == write_pos;
   }

   // Check if full
   [[nodiscard]] bool full() const {
      uint64_t read_pos = header_->read_pos.load(std::memory_order_acquire);
      uint64_t write_pos = header_->write_pos.load(std::memory_order_relaxed);
      return (write_pos - read_pos) >= header_->capacity;
   }

   // Reset the ring buffer (not thread-safe)
   void reset() {
      header_->write_pos.store(0, std::memory_order_relaxed);
      header_->read_pos.store(0, std::memory_order_relaxed);
   }

   /// Zero-copy view of the ring buffer (entire capacity).
   /// Returns a span of T elements covering the full buffer.
   [[nodiscard]] std::span<T> data() {
      return std::span<T>(reinterpret_cast<T *>(buffer_),
                          header_->capacity / sizeof(T));
   }

   [[nodiscard]] std::span<const T> data() const {
      return std::span<const T>(reinterpret_cast<const T *>(buffer_),
                                header_->capacity / sizeof(T));
   }

   /// Raw pointer to the ring buffer (backward compatibility).
   [[nodiscard]] T *raw_data() {
      return reinterpret_cast<T *>(buffer_);
   }
   [[nodiscard]] const T *raw_data() const {
      return reinterpret_cast<const T *>(buffer_);
   }

   /**
    * @brief Destroy the ring buffer and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
