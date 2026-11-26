#pragma once

#include <atomic>
#include <cassert>
#include <cstddef>
#include <expected>
#include <format>
#include <stdexcept>
#include <string_view>
#include <tuple>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"
#include "mutex.h"

namespace zeroshm {

/**
 * @brief Read-Write Lock state for shared memory
 *
 * Binary layout:
 * - 4 bytes: active readers count
 * - 4 bytes: writer waiting flag
 * - Mutex for reader coordination
 * - Mutex for writer exclusion
 */
struct RWLockState {
   std::atomic<int32_t> readers{0};         // Number of active readers
   std::atomic<int32_t> writer_active{0};   // 1 if writer active, 0 otherwise

   RWLockState() : readers(0), writer_active(0) {
   }
};

static_assert(std::is_trivially_copyable_v<RWLockState>,
              "RWLockState must be trivially copyable");

/// Compile-time validation: RWLockState must be exactly 8 bytes and match
/// the layout expected by Go/Rust implementations.
static_assert(sizeof(RWLockState) == 8,
              "RWLockState size mismatch -- binary format incompatibility");
static_assert(offsetof(RWLockState, readers) == RWLOCK_READERS_OFFSET,
              "RWLockState.readers must be at RWLOCK_READERS_OFFSET");
static_assert(offsetof(RWLockState, writer_active) == RWLOCK_WRITER_ACTIVE_OFFSET,
              "RWLockState.writer_active must be at RWLOCK_WRITER_ACTIVE_OFFSET");

/**
 * @brief Read-Write Lock for shared memory
 *
 * Allows multiple concurrent readers OR one exclusive writer.
 * Optimized for read-heavy workloads where reads vastly outnumber writes.
 *
 * Features:
 * - Multiple readers can hold lock simultaneously
 * - Writer gets exclusive access (no readers, no other writers)
 * - RAII-compatible with SharedLock and UniqueLock
 *
 * Performance characteristics:
 * - reader_lock(): Fast when no writers (just atomic increment)
 * - writer_lock(): Waits for all readers to finish
 *
 * Example:
 * @code
 * zeroshm::Memory mem("/data", 1024 * 1024);
 * zeroshm::RWLock rwlock(mem, "data_lock");
 * zeroshm::Array<int> data(mem, "data", 1000);
 *
 * // Many readers (concurrent)
 * rwlock.reader_lock();
 * int value = data[0];
 * rwlock.reader_unlock();
 *
 * // Single writer (exclusive)
 * rwlock.writer_lock();
 * data[0] = 42;
 * rwlock.writer_unlock();
 *
 * // RAII style
 * {
 *     SharedLock read_guard(rwlock);  // For reading
 *     int value = data[0];
 * }
 * {
 *     UniqueLock write_guard(rwlock);  // For writing
 *     data[0] = 99;
 * }
 * @endcode
 */
class RWLock {
 private:
   /// Tag types for constructor dispatch
   struct FactoryTag {};
   struct AutoCreate {};
   struct OpenExisting {};

   /// Constructor that creates a new RWLock.
   RWLock(Memory &mem, std::string_view name, FactoryTag)
       : RWLock(mem, name, AutoCreate{}) {
   }

   /// Constructor that auto-creates or opens based on table.
   RWLock(Memory &mem, std::string_view name, AutoCreate)
       : mem_(mem), name_(name) {
      auto [state_name, reader_mtx_name, writer_mtx_name] = derive_names();

      auto find_result = mem.table()->find(name);

      if (find_result) {
         init_open(state_name, reader_mtx_name, writer_mtx_name);
      } else {
         init_create(state_name, reader_mtx_name, writer_mtx_name);
      }
   }

   /// Constructor that opens an existing RWLock.
   RWLock(Memory &mem, std::string_view name, OpenExisting)
       : mem_(mem), name_(name) {
      auto [state_name, reader_mtx_name, writer_mtx_name] = derive_names();

      init_open(state_name, reader_mtx_name, writer_mtx_name);
   }

   void init_create(const std::string &state_name,
                    const std::string &reader_mtx_name,
                    const std::string &writer_mtx_name) {
      // Create new
      auto state_alloc = mem_.allocate(state_name, sizeof(RWLockState));
      if (!state_alloc) {
         throw std::runtime_error(
             std::format("Failed to allocate RWLock state: {}",
                         error_message(state_alloc.error())));
      }
      size_t state_offset = *state_alloc;
      state_ = mem_.ptr_at<RWLockState>(state_offset);
      new (state_) RWLockState();

      create_mutexes(reader_mtx_name, writer_mtx_name);

      // Add marker entry
      auto marker_alloc = mem_.allocate(name_, 1);
      if (!marker_alloc) {
         throw std::runtime_error(
             std::format("Failed to allocate RWLock marker: {}",
                         error_message(marker_alloc.error())));
      }
      (void)*marker_alloc;
   }

   void init_open(const std::string &state_name,
                  const std::string &reader_mtx_name,
                  const std::string &writer_mtx_name) {
      // Open existing
      auto state_result = mem_.table()->find(state_name);
      if (!state_result) {
         throw std::runtime_error("RWLock state not found");
      }
      state_ = mem_.ptr_at<RWLockState>((*state_result)->offset);

      create_mutexes(reader_mtx_name, writer_mtx_name);
   }

   Memory &mem_;
   std::string name_;
   RWLockState *state_;
   std::unique_ptr<Mutex> reader_mutex_;
   std::unique_ptr<Mutex> writer_mutex_;

   inline std::tuple<std::string, std::string, std::string> derive_names() const {
      return {std::string(name_) + "_state",
              std::string(name_) + "_rmtx",
              std::string(name_) + "_wmtx"};
   }

   inline void create_mutexes(const std::string &reader_mtx_name,
                              const std::string &writer_mtx_name) {
      reader_mutex_ = std::make_unique<Mutex>(mem_, reader_mtx_name);
      writer_mutex_ = std::make_unique<Mutex>(mem_, writer_mtx_name);
   }

 public:
   /// Create a new RWLock in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<RWLock, Error> create(Memory &mem,
                                              std::string_view name) {
      try {
         return RWLock(mem, name, FactoryTag{});
      } catch (const std::overflow_error &) {
         return std::unexpected(Error::AllocationOverflow);
      } catch (const std::runtime_error &) {
         return std::unexpected(Error::BoundsExceeded);
      }
   }

   /// Open an existing RWLock in shared memory.
   /// Returns Error::NotFound if the name doesn't exist.
   static std::expected<RWLock, Error> open(Memory &mem,
                                            std::string_view name) {
      try {
         return RWLock(mem, name, OpenExisting{});
      } catch (const std::runtime_error &) {
         return std::unexpected(Error::NotFound);
      }
   }

   /**
    * @brief Create or open a RWLock
    * @param mem Memory region
    * @param name Unique name for this RWLock
    * @throws std::runtime_error if allocation fails
    */
   RWLock(Memory &mem, std::string_view name)
       : RWLock(mem, name, AutoCreate{}) {
   }

   /**
    * @brief Acquire read lock (shared access)
    *
    * Multiple threads can hold read locks simultaneously.
    * Blocks if a writer currently holds the lock.
    */
   void reader_lock() {
      reader_mutex_->lock();

      // Wait for any active writer to finish
      while (state_->writer_active.load(std::memory_order_acquire) != 0) {
         reader_mutex_->unlock();
         std::this_thread::yield();
         reader_mutex_->lock();
      }

      state_->readers.fetch_add(1, std::memory_order_release);
      reader_mutex_->unlock();
   }

   /**
    * @brief Try to acquire read lock without blocking
    * @return expected<void, Error> -- success if acquired, LockNotAcquired if
    * busy
    */
   [[nodiscard]] std::expected<void, Error> try_reader_lock() {
      auto reader_result = reader_mutex_->try_lock();
      if (!reader_result) {
         return reader_result;
      }

      if (state_->writer_active.load(std::memory_order_acquire) != 0) {
         reader_mutex_->unlock();
         return std::unexpected(Error::LockNotAcquired);
      }

      state_->readers.fetch_add(1, std::memory_order_release);
      reader_mutex_->unlock();
      return {};
   }

   /**
    * @brief Release read lock
    */
   void reader_unlock() {
      [[maybe_unused]] int32_t prev =
          state_->readers.fetch_sub(1, std::memory_order_release);
      assert(prev > 0 && "reader_unlock without a matching reader_lock");
   }

   /**
    * @brief Acquire write lock (exclusive access)
    *
    * Only one writer can hold the lock. Blocks until all readers
    * release their locks and no other writer holds the lock.
    */
   void writer_lock() {
      writer_mutex_->lock();

      // Mark writer as active UNDER reader_mutex_. Readers check
      // writer_active and increment readers while holding reader_mutex_;
      // setting the flag under the same mutex closes the race where a
      // reader passes its writer check while the writer simultaneously
      // passes its readers check, letting both hold the lock at once.
      reader_mutex_->lock();
      state_->writer_active.store(1, std::memory_order_release);
      reader_mutex_->unlock();

      // Wait for all readers to finish
      while (state_->readers.load(std::memory_order_acquire) > 0) {
         std::this_thread::yield();
      }
   }

   /**
    * @brief Try to acquire write lock without blocking
    * @return expected<void, Error> -- success if acquired, LockNotAcquired if
    * busy
    */
   [[nodiscard]] std::expected<void, Error> try_writer_lock() {
      auto writer_result = writer_mutex_->try_lock();
      if (!writer_result) {
         return writer_result;
      }

      // Readers only transition 0 -> 1 while holding reader_mutex_, so
      // checking readers and setting writer_active under it is atomic
      // with respect to new readers (see writer_lock).
      auto reader_result = reader_mutex_->try_lock();
      if (!reader_result) {
         writer_mutex_->unlock();
         return reader_result;
      }

      if (state_->readers.load(std::memory_order_acquire) > 0) {
         reader_mutex_->unlock();
         writer_mutex_->unlock();
         return std::unexpected(Error::LockNotAcquired);
      }

      state_->writer_active.store(1, std::memory_order_release);
      reader_mutex_->unlock();
      return {};
   }

   /**
    * @brief Release write lock
    */
   void writer_unlock() {
      state_->writer_active.store(0, std::memory_order_release);
      writer_mutex_->unlock();
   }

   /**
    * @brief Destroy the RWLock and remove it from the shared memory table.
    *
    * Destroys the internal mutexes and removes the table entries so
    * the names can be reused.
    */
   void destroy() {
      if (reader_mutex_) {
         reader_mutex_->destroy();
      }
      if (writer_mutex_) {
         writer_mutex_->destroy();
      }
      (void)mem_.deallocate(name_ + "_state");
      (void)mem_.deallocate(name_);
   }

   // Prevent copying
   RWLock(const RWLock &) = delete;
   RWLock &operator=(const RWLock &) = delete;

   // Allow moving (move assignment is deleted due to reference member)
   RWLock(RWLock &&) = default;
   RWLock &operator=(RWLock &&) = delete;
};

/**
 * @brief RAII wrapper for read lock (shared access)
 *
 * Automatically acquires reader lock on construction and releases on
 * destruction.
 */
class SharedLock {
 private:
   RWLock &rwlock_;

 public:
   explicit SharedLock(RWLock &rwlock) : rwlock_(rwlock) {
      rwlock_.reader_lock();
   }

   ~SharedLock() {
      rwlock_.reader_unlock();
   }

   // Prevent copying
   SharedLock(const SharedLock &) = delete;
   SharedLock &operator=(const SharedLock &) = delete;
};

/**
 * @brief RAII wrapper for write lock (exclusive access)
 *
 * Automatically acquires writer lock on construction and releases on
 * destruction.
 */
class UniqueLock {
 private:
   RWLock &rwlock_;

 public:
   explicit UniqueLock(RWLock &rwlock) : rwlock_(rwlock) {
      rwlock_.writer_lock();
   }

   ~UniqueLock() {
      rwlock_.writer_unlock();
   }

   // Prevent copying
   UniqueLock(const UniqueLock &) = delete;
   UniqueLock &operator=(const UniqueLock &) = delete;
};

}   // namespace zeroshm
