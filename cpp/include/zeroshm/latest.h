#pragma once

#include <atomic>
#include <cerrno>
#include <chrono>
#include <cstring>
#include <ctime>
#include <expected>
#include <format>
#include <pthread.h>
#include <thread>
#include <utility>

#include <zeroshm/layout.h>

#include "error.h"
#include "memory.h"
#include "table.h"

namespace zeroshm {

// Latest is a single-producer, multi-reader shared memory value with change
// detection. The producer calls set() to write a new value and increment the
// version. Consumers call get() to read the current value, or wait_for_change()
// to block until the version changes.
//
// Binary layout:
//   - Header (8 bytes): elem_size (uint32), aligned_size (uint32)
//   - value: elem_size bytes
//   - padding: to 8-byte alignment (if elem_size is not a multiple of 8)
//   - version: uint64 (atomic, starts at align_up(elem_size, 8))
//
// Total payload size after header = align_up(elem_size, 8) + 8 (version)
//   + aligned pthread_mutex_t + aligned pthread_cond_t (unified blocking
//   layout)
template <typename T> class Latest {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");
   static_assert(sizeof(T) > 0, "Latest does not support zero-size types");

   struct Header {
      uint32_t elem_size;      // Size of T in bytes
      uint32_t aligned_size;   // align_up(elem_size, 8)
   };

   /// Compile-time validation: Latest::Header must be exactly 8 bytes.
   static_assert(
       sizeof(Header) == EXPECTED_LATEST_HEADER_SIZE,
       "Latest::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, elem_size) == 0,
                 "Latest::Header.elem_size must be at offset 0");
   static_assert(offsetof(Header, aligned_size) == 4,
                 "Latest::Header.aligned_size must be at offset 4");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Latest(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), value_(nullptr),
         version_(nullptr), mutex_(nullptr), cond_(nullptr) {
   }

   /// Core initialization logic for creating a new Latest value.
   std::expected<void, Error> init_create(const T &initial_value) {
      size_t elem_size = sizeof(T);
      size_t aligned_size =
          (elem_size + MAX_ELEM_ALIGN - 1) & ~(MAX_ELEM_ALIGN - 1);

      // Use the unified layout size (includes mutex and condvar)
      size_t total_size = latest_total_size(elem_size);

      auto alloc_result = memory_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      // Zero-initialize the entire region (including padding and sync
      // structures)
      std::memset(memory_.at(offset), 0, total_size);

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->elem_size = static_cast<uint32_t>(elem_size);
      header_->aligned_size = static_cast<uint32_t>(aligned_size);

      // Pointers to value and version
      value_ = memory_.ptr_at<char>(offset + sizeof(Header));
      version_ = reinterpret_cast<std::atomic<uint64_t> *>(
          memory_.at(latest_version_offset(offset, elem_size)));

      // Write initial value
      std::memcpy(value_, &initial_value, elem_size);

      // Initialize version to 1
      version_->store(1, std::memory_order_release);

      // Initialize mutex with PTHREAD_PROCESS_SHARED
      mutex_ = memory_.ptr_at<pthread_mutex_t>(
          latest_mutex_offset(offset, elem_size));
      pthread_mutexattr_t mutex_attr;
      int ret = pthread_mutexattr_init(&mutex_attr);
      if (ret != 0) {
         return std::unexpected(Error::AllocationFailed);
      }
      ret = pthread_mutexattr_setpshared(&mutex_attr, PTHREAD_PROCESS_SHARED);
      if (ret != 0) {
         pthread_mutexattr_destroy(&mutex_attr);
         return std::unexpected(Error::AllocationFailed);
      }
      ret = pthread_mutex_init(mutex_, &mutex_attr);
      pthread_mutexattr_destroy(&mutex_attr);
      if (ret != 0) {
         return std::unexpected(Error::AllocationFailed);
      }

      // Initialize condvar with PTHREAD_PROCESS_SHARED
      cond_ =
          memory_.ptr_at<pthread_cond_t>(latest_cond_offset(offset, elem_size));
      pthread_condattr_t cond_attr;
      ret = pthread_condattr_init(&cond_attr);
      if (ret != 0) {
         pthread_mutex_destroy(mutex_);
         return std::unexpected(Error::AllocationFailed);
      }
      ret = pthread_condattr_setpshared(&cond_attr, PTHREAD_PROCESS_SHARED);
      if (ret != 0) {
         pthread_condattr_destroy(&cond_attr);
         pthread_mutex_destroy(mutex_);
         return std::unexpected(Error::AllocationFailed);
      }
      ret = pthread_cond_init(cond_, &cond_attr);
      pthread_condattr_destroy(&cond_attr);
      if (ret != 0) {
         pthread_mutex_destroy(mutex_);
         return std::unexpected(Error::AllocationFailed);
      }

      return {};
   }

   /// Core initialization logic for opening an existing Latest value.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      if (header_->elem_size != sizeof(T)) {
         return std::unexpected(Error::TypeMismatch);
      }

      size_t expected_aligned =
          (sizeof(T) + MAX_ELEM_ALIGN - 1) & ~(MAX_ELEM_ALIGN - 1);
      if (header_->aligned_size != expected_aligned) {
         return std::unexpected(Error::TypeMismatch);
      }

      // Check against the unified layout size (includes mutex and condvar)
      size_t expected_size = latest_total_size(sizeof(T));
      if (size != expected_size) {
         return std::unexpected(Error::TypeMismatch);
      }

      // Pointers to value and version
      value_ = memory_.ptr_at<char>(offset + sizeof(Header));
      version_ = reinterpret_cast<std::atomic<uint64_t> *>(
          memory_.at(latest_version_offset(offset, sizeof(T))));

      // Pointers to existing mutex and condvar (no re-initialization)
      mutex_ = memory_.ptr_at<pthread_mutex_t>(
          latest_mutex_offset(offset, sizeof(T)));
      cond_ =
          memory_.ptr_at<pthread_cond_t>(latest_cond_offset(offset, sizeof(T)));

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_ = nullptr;
   char *value_ = nullptr;
   std::atomic<uint64_t> *version_ = nullptr;
   pthread_mutex_t *mutex_ = nullptr;
   pthread_cond_t *cond_ = nullptr;

 public:
   /// Create a new Latest value in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Latest, Error>
   create(Memory &memory, std::string_view name, const T &initial_value) {
      Latest l(memory, name, FactoryTag{});
      auto result = l.init_create(initial_value);
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   /// Open an existing Latest value in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match.
   static std::expected<Latest, Error> open(Memory &memory,
                                            std::string_view name) {
      Latest l(memory, name, FactoryTag{});
      auto result = l.init_open();
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   // Create new Latest value
   Latest(Memory &memory, std::string_view name, const T &initial_value)
       : Latest(memory, name, FactoryTag{}) {

      auto result = init_create(initial_value);
      if (!result) {
         throw std::runtime_error(std::format("Latest::create failed: {}",
                                              error_message(result.error())));
      }
   }

   // Open existing Latest value
   Latest(Memory &memory, std::string_view name)
       : Latest(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         throw std::runtime_error(std::format("Latest::open failed: {}",
                                              error_message(result.error())));
      }
   }

   /// Read the current value and version assuming the mutex is already held.
   /// Used by get() and wait_for_change() to avoid double-locking.
   std::pair<T, uint64_t> read_unlocked() const {
      uint64_t v = version_->load(std::memory_order_acquire);
      T value;
      std::memcpy(&value, value_, sizeof(T));
      return {value, v};
   }

   // Get reads the current value and version. Safe for concurrent readers.
   // Locks the mutex to ensure a consistent snapshot of value + version.
   [[nodiscard]] std::pair<T, uint64_t> get() const {
      pthread_mutex_lock(mutex_);
      auto result = read_unlocked();
      pthread_mutex_unlock(mutex_);
      return result;
   }

   // Set writes a new value, increments the version, and wakes all blocked
   // readers. Single producer only. Locks the mutex to protect the value +
   // version update and broadcasts the condition variable to wake waiting
   // readers.
   void set(const T &value) {
      pthread_mutex_lock(mutex_);

      // Write the value
      std::memcpy(value_, &value, sizeof(T));

      // Atomically increment version with release ordering
      uint64_t current = version_->load(std::memory_order_relaxed);
      while (!version_->compare_exchange_strong(current, current + 1,
                                                std::memory_order_release,
                                                std::memory_order_relaxed)) {
         // CAS failed, retry with updated current
      }

      // Wake all blocked readers
      pthread_cond_broadcast(cond_);

      pthread_mutex_unlock(mutex_);
   }

   // Version returns the current version number. Safe for concurrent readers.
   [[nodiscard]] uint64_t version() const {
      return version_->load(std::memory_order_acquire);
   }

   // Wait_for_change blocks until the version is greater than last_version or
   // timeout expires. Returns (value, version, changed) where changed is true
   // if the version advanced. Uses pthread_cond_timedwait for true OS-level
   // blocking (zero CPU while waiting).
   [[nodiscard]] std::tuple<T, uint64_t, bool>
   wait_for_change(uint64_t last_version,
                   std::chrono::milliseconds timeout) const {
      pthread_mutex_lock(mutex_);

      // Convert timeout to a absolute timespec for pthread_cond_timedwait.
      // pthread_cond_timedwait expects CLOCK_REALTIME.
      struct timespec ts;
      clock_gettime(CLOCK_REALTIME, &ts);
      ts.tv_sec += timeout.count() / 1000;
      ts.tv_nsec += (timeout.count() % 1000) * 1000000;
      if (ts.tv_nsec >= 1000000000) {
         ts.tv_sec += ts.tv_nsec / 1000000000;
         ts.tv_nsec %= 1000000000;
      }

      while (true) {
         auto [value, v] = read_unlocked();
         if (v > last_version) {
            pthread_mutex_unlock(mutex_);
            return {value, v, true};
         }

         int ret = pthread_cond_timedwait(cond_, mutex_, &ts);
         if (ret == ETIMEDOUT) {
            // Timeout -- return current value
            auto [cur_value, cur_v] = read_unlocked();
            pthread_mutex_unlock(mutex_);
            return {cur_value, cur_v, false};
         }
         // Spurious wakeup or broadcast -- loop and re-check version
      }
   }

   // Name returns the Latest value name.
   [[nodiscard]] const std::string &name() const {
      return name_;
   }

   // Destroy removes the Latest value from the shared memory table.
   // Destroys the mutex and condvar before removing the entry.
   void destroy() {
      if (mutex_) {
         pthread_mutex_destroy(mutex_);
      }
      if (cond_) {
         pthread_cond_destroy(cond_);
      }
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
