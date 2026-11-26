#pragma once

#include <cerrno>
#include <cstring>
#include <expected>
#include <fcntl.h>
#include <format>
#include <memory>
#include <span>
#include <stdexcept>
#include <string>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include <zeroshm/error.h>
#include <zeroshm/table.h>

namespace zeroshm {

/**
 * POSIX shared memory wrapper with automatic cleanup and table management.
 *
 * This class manages a shared memory segment and its metadata table.
 * The table is always placed at the beginning of the shared memory.
 */
class Memory {
 public:
   /// Convert a logical segment name to a short OS-level name suitable for
   /// shm_open. Uses xxhash3-64 with the ZeroSHM seed to produce a
   /// deterministic short name matching Go's osSegmentName: "/z" + 16 hex
   /// digits.
   static std::string os_segment_name(std::string_view name) {
      std::string normalized;
      if (name.empty() || name.front() != '/') {
         normalized = "/";
         normalized.append(name);
      } else {
         normalized.assign(name);
      }
      uint64_t h = XXH3_64bits_withSeed(normalized.data(), normalized.size(),
                                        XXHASH_SEED);
      return std::format("/z{:016x}", h);
   }

 private:
   void create() {
      // Hash the logical name to produce the OS-level segment name
      std::string os_name = os_segment_name(name_);

      // Create shared memory
      fd_ = shm_open(os_name.c_str(), O_CREAT | O_RDWR | O_EXCL, 0666);
      if (fd_ < 0) {
         if (errno == EEXIST) {
            // Try to unlink and recreate
            shm_unlink(os_name.c_str());
            fd_ = shm_open(os_name.c_str(), O_CREAT | O_RDWR | O_EXCL, 0666);
         }

         if (fd_ < 0) {
            throw std::runtime_error(std::format(
                "Failed to create shared memory: {}", strerror(errno)));
         }
      }

      // Set size
      if (ftruncate(fd_, size_) < 0) {
         close(fd_);
         shm_unlink(os_name.c_str());
         throw std::runtime_error(std::format(
             "Failed to set shared memory size: {}", strerror(errno)));
      }

      // Map memory
      memory_ =
          mmap(nullptr, size_, PROT_READ | PROT_WRITE, MAP_SHARED, fd_, 0);
      if (memory_ == MAP_FAILED) {
         close(fd_);
         shm_unlink(os_name.c_str());
         throw std::runtime_error(
             std::format("Failed to map shared memory: {}", strerror(errno)));
      }

      // Zero out the memory
      std::memset(memory_, 0, size_);
   }

   void open() {
      // Hash the logical name to produce the OS-level segment name
      std::string os_name = os_segment_name(name_);

      // Open existing shared memory
      fd_ = shm_open(os_name.c_str(), O_RDWR, 0666);
      if (fd_ < 0) {
         throw std::runtime_error(
             std::format("Failed to open shared memory: {}", strerror(errno)));
      }

      // Get size
      struct stat st;
      if (fstat(fd_, &st) < 0) {
         close(fd_);
         throw std::runtime_error(std::format(
             "Failed to get shared memory info: {}", strerror(errno)));
      }
      size_ = st.st_size;

      // Map memory
      memory_ =
          mmap(nullptr, size_, PROT_READ | PROT_WRITE, MAP_SHARED, fd_, 0);
      if (memory_ == MAP_FAILED) {
         close(fd_);
         throw std::runtime_error(
             std::format("Failed to map shared memory: {}", strerror(errno)));
      }
   }

   std::string name_;
   size_t size_;
   size_t max_entries_;
   int fd_;
   void *memory_;
   std::unique_ptr<Table> table_;
   bool owner_;

 public:
   /**
    * Create or open shared memory
    * @param name Shared memory name (e.g., "/myshm")
    * @param size Size in bytes (0 to open existing)
    * @param max_entries Maximum number of table entries (default 64)
    */
   Memory(const std::string &name, size_t size = 0, size_t max_entries = 64)
       : name_(name), size_(size), max_entries_(max_entries), fd_(-1),
         memory_(nullptr), table_(nullptr), owner_(size > 0) {

      if (size > 0) {
         create();
      } else {
         open();
      }

      // Initialize table
      table_ = std::make_unique<Table>(memory_, max_entries_, size_, owner_);
   }

   ~Memory() {
      if (memory_ && size_ > 0) {
         munmap(memory_, size_);
      }
      if (fd_ >= 0) {
         close(fd_);
      }
   }

   // Disable copy
   Memory(const Memory &) = delete;
   Memory &operator=(const Memory &) = delete;

   // Enable move
   Memory(Memory &&other) noexcept
       : name_(std::move(other.name_)), size_(other.size_),
         max_entries_(other.max_entries_), fd_(other.fd_),
         memory_(other.memory_), table_(std::move(other.table_)),
         owner_(other.owner_) {
      other.fd_ = -1;
      other.memory_ = nullptr;
      other.size_ = 0;
   }

   Memory &operator=(Memory &&other) noexcept {
      if (this != &other) {
         // Clean up current resources
         if (memory_ && size_ > 0) {
            munmap(memory_, size_);
         }
         if (fd_ >= 0) {
            close(fd_);
         }

         // Move resources
         name_ = std::move(other.name_);
         size_ = other.size_;
         max_entries_ = other.max_entries_;
         fd_ = other.fd_;
         memory_ = other.memory_;
         table_ = std::move(other.table_);
         owner_ = other.owner_;

         // Clear other
         other.fd_ = -1;
         other.memory_ = nullptr;
         other.size_ = 0;
      }
      return *this;
   }

   /**
    * Unlink (delete) the shared memory
    */
   void unlink() {
      shm_unlink(os_segment_name(name_).c_str());
   }

   /**
    * Static method to unlink shared memory by name
    */
   static void unlink(std::string_view name) {
      shm_unlink(os_segment_name(name).c_str());
   }

   /**
    * Get pointer to the shared memory
    */
   void *data() {
      return memory_;
   }
   const void *data() const {
      return memory_;
   }

   /**
    * Get base pointer (alias for data())
    */
   void *base() {
      return memory_;
   }
   const void *base() const {
      return memory_;
   }

   /**
    * Get pointer to memory at specific offset
    */
   void *at(size_t offset) {
      if (offset >= size_) {
         throw std::out_of_range("Offset out of bounds");
      }
      return static_cast<char *>(memory_) + offset;
   }

   const void *at(size_t offset) const {
      if (offset >= size_) {
         throw std::out_of_range("Offset out of bounds");
      }
      return static_cast<const char *>(memory_) + offset;
   }

   /**
    * Get typed pointer to memory at specific offset.
    * Replaces the common pattern:
    *   reinterpret_cast<T*>(static_cast<char*>(memory.base()) + offset)
    */
   template <typename T> T *ptr_at(size_t offset) {
      if (offset + sizeof(T) > size_) {
         throw std::out_of_range("ptr_at: offset out of bounds");
      }
      return reinterpret_cast<T *>(static_cast<char *>(memory_) + offset);
   }

   template <typename T> const T *ptr_at(size_t offset) const {
      if (offset + sizeof(T) > size_) {
         throw std::out_of_range("ptr_at: offset out of bounds");
      }
      return reinterpret_cast<const T *>(static_cast<const char *>(memory_) +
                                         offset);
   }

   /**
    * Get a std::span view of memory at specific offset (C++20).
    * Returns a zero-copy, bounds-aware view of `count` elements of type T
    * starting at `offset`. Enables std::ranges algorithms on shared memory:
    *   std::ranges::for_each(mem.span_at<float>(offset, 100), [](float x) { ...
    * });
    *
    * @tparam T Element type (must be trivially copyable)
    * @param offset Byte offset from start of shared memory
    * @param count Number of elements in the span
    * @return std::span<T> view of the memory region
    * @throws std::out_of_range if offset + count*sizeof(T) exceeds memory size
    */
   template <typename T> std::span<T> span_at(size_t offset, size_t count) {
      if (count == 0) {
         if (offset > size_) {
            throw std::out_of_range("span_at: offset out of bounds");
         }
         return std::span<T>();
      }
      size_t total = count * sizeof(T);
      if (offset > size_ || total > size_ - offset) {
         throw std::out_of_range("span_at: region exceeds memory bounds");
      }
      return std::span<T>(
          reinterpret_cast<T *>(static_cast<char *>(memory_) + offset), count);
   }

   template <typename T>
   std::span<const T> span_at(size_t offset, size_t count) const {
      if (count == 0) {
         if (offset > size_) {
            throw std::out_of_range("span_at: offset out of bounds");
         }
         return std::span<const T>();
      }
      size_t total = count * sizeof(T);
      if (offset > size_ || total > size_ - offset) {
         throw std::out_of_range("span_at: region exceeds memory bounds");
      }
      return std::span<const T>(
          reinterpret_cast<const T *>(static_cast<const char *>(memory_) +
                                      offset),
          count);
   }

   /**
    * Get a std::span view of a single element at specific offset.
    * Convenience overload for accessing a single object as a span.
    */
   template <typename T> std::span<T> span_at(size_t offset) {
      if (offset > size_ - sizeof(T)) {
         throw std::out_of_range("span_at: offset out of bounds");
      }
      return std::span<T>(
          reinterpret_cast<T *>(static_cast<char *>(memory_) + offset), 1);
   }

   template <typename T> std::span<const T> span_at(size_t offset) const {
      if (offset > size_ - sizeof(T)) {
         throw std::out_of_range("span_at: offset out of bounds");
      }
      return std::span<const T>(
          reinterpret_cast<const T *>(static_cast<const char *>(memory_) +
                                      offset),
          1);
   }

   /**
    * Allocate space in shared memory
    * @param name Name for the table entry
    * @param size Size to allocate
    * @return std::expected<size_t, Error> -- offset of allocated space on
    * success, or Error::BoundsExceeded / Error::AllocationOverflow /
    *         Error::TableFull / Error::NameTooLong / Error::NameAlreadyExists
    */
   std::expected<size_t, Error> allocate(std::string_view name, size_t size) {
      uint64_t offset = 0;

      // First allocate the space -- catch exceptions from Table::allocate
      try {
         offset = table_->allocate(size);
      } catch (const std::overflow_error &) {
         return std::unexpected(Error::AllocationOverflow);
      } catch (const std::runtime_error &) {
         return std::unexpected(Error::BoundsExceeded);
      }

      // Then add to table
      auto result = table_->add(name, offset, size);
      if (!result) {
         return std::unexpected(result.error());
      }

      return offset;
   }

   /**
    * Find an entry in the table
    * @param name Name to find
    * @param offset Output: offset of the entry
    * @param size Output: size of the entry
    * @return true if found, false otherwise
    */
   bool find(std::string_view name, size_t &offset, size_t &size) {
      auto result = table_->find(name);
      if (result) {
         offset = (*result)->offset;
         size = (*result)->size;
         return true;
      }
      return false;
   }

   /**
    * Remove a named allocation from the table.
    * Note: This does NOT reclaim the space (no compaction).
    * The entry is removed so the name can be reused by a future allocation.
    * @param name Name to remove
    * @return std::expected<void, Error> -- Error::NotFound if the name doesn't
    * exist
    */
   [[nodiscard]] std::expected<void, Error> deallocate(std::string_view name) {
      return table_->remove(name);
   }

   /**
    * Get the table
    */
   Table *table() {
      return table_.get();
   }
   const Table *table() const {
      return table_.get();
   }

   /**
    * Get the size of the shared memory
    */
   size_t size() const {
      return size_;
   }

   /**
    * Get the name of the shared memory
    */
   const std::string &name() const {
      return name_;
   }

   /**
    * Check if this instance created the shared memory
    */
   bool is_owner() const {
      return owner_;
   }
};

}   // namespace zeroshm
