#pragma once

#include <chrono>
#include <expected>
#include <stdexcept>
#include <string_view>

#include "error.h"
#include "memory.h"
#include "semaphore.h"

namespace zeroshm {

/**
 * @brief Mutex for mutual exclusion across processes
 *
 * Provides RAII-compatible mutual exclusion synchronization. Works across
 * processes sharing the same memory region. Compatible with std::lock_guard
 * and std::unique_lock.
 *
 * Internally implemented as a binary semaphore (initial count = 1).
 *
 * Example:
 * @code
 * zeroshm::Memory mem("/data", 1024 * 1024);
 * zeroshm::Mutex mtx(mem, "data_mutex");
 *
 * // Manual locking
 * mtx.lock();
 * // ...critical section...
 * mtx.unlock();
 *
 * // RAII locking
 * {
 *     std::lock_guard<zeroshm::Mutex> lock(mtx);
 *     // ...critical section...
 * }  // Automatically unlocked
 * @endcode
 */
class Mutex {
 private:
   std::unique_ptr<Semaphore> sem_;

 public:
   /**
    * @brief Create or open a Mutex
    * @param mem Memory region
    * @param name Unique name for this mutex
    * @throws std::runtime_error if allocation fails
    */
   Mutex(Memory &mem, std::string_view name) {
      auto entry = mem.table()->find(name);

      if (entry) {
         // Open existing semaphore
         sem_ = std::make_unique<Semaphore>(mem, name);
      } else {
         // Create new binary semaphore (initial_count=1, max_count=1 for mutex)
         sem_ = std::make_unique<Semaphore>(mem, name, 1, 1);
      }
   }

   /**
    * @brief Lock the mutex
    *
    * Blocks until the mutex is acquired. If another thread/process holds
    * the lock, this will wait until it's released.
    */
   void lock() {
      sem_->acquire();
   }

   /**
    * @brief Try to lock the mutex without blocking
    * @return expected<void, Error> -- success if acquired, LockNotAcquired if
    * already locked
    */
   [[nodiscard]] std::expected<void, Error> try_lock() {
      return sem_->try_acquire();
   }

   /**
    * @brief Try to lock with timeout
    * @param timeout Maximum time to wait
    * @return expected<void, Error> -- success if acquired, LockNotAcquired on
    * timeout
    */
   template <typename Rep, typename Period>
   [[nodiscard]] std::expected<void, Error>
   try_lock_for(const std::chrono::duration<Rep, Period> &timeout) {
      return sem_->acquire_for(timeout);
   }

   /**
    * @brief Unlock the mutex
    *
    * Must be called by the same thread/process that locked it.
    */
   void unlock() {
      sem_->release();
   }

   /**
    * @brief Destroy the mutex and remove it from the shared memory table.
    *
    * Deallocates the underlying semaphore so the name can be reused.
    */
   void destroy() {
      if (sem_) {
         sem_->destroy();
      }
   }

   // Prevent copying
   Mutex(const Mutex &) = delete;
   Mutex &operator=(const Mutex &) = delete;

   // Allow moving
   Mutex(Mutex &&) = default;
   Mutex &operator=(Mutex &&) = default;
};

}   // namespace zeroshm
