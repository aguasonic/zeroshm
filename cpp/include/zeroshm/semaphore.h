#pragma once

#include <atomic>
#include <chrono>
#include <cstddef>
#include <expected>
#include <format>
#include <stdexcept>
#include <utility>

#include <zeroshm/constants_generated.h>

#include "detail/spin_wait.h"
#include "error.h"
#include "memory.h"

namespace zeroshm {

/**
 * Lock-free semaphore for cross-process synchronization.
 *
 * A semaphore maintains a non-negative integer count representing available
 * resources or permits. Processes can acquire() to decrement the count and
 * release() to increment it. If count is zero, acquire() blocks until
 * another process calls release().
 *
 * Supports three modes:
 * - Binary semaphore (max_count=1): Acts as a mutex
 * - Counting semaphore (max_count=N): Resource pool with N permits
 * - Unbounded semaphore (max_count=0): No upper limit on count
 *
 * Thread-safe and process-safe.
 */
class Semaphore {
 public:
   struct Header {
      std::atomic<int32_t> count;     // Current count
      std::atomic<int32_t> waiting;   // Number of waiting processes
      int32_t max_count;              // Maximum count (0 = unbounded)
      int32_t _padding;               // Alignment padding
   };

   /// Compile-time validation: Semaphore::Header must be exactly 16 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 16,
                 "Semaphore::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, count) == SEMAPHORE_COUNT_OFFSET,
                 "Semaphore::Header.count must be at SEMAPHORE_COUNT_OFFSET");
   static_assert(offsetof(Header, waiting) == SEMAPHORE_WAITING_OFFSET,
                 "Semaphore::Header.waiting must be at SEMAPHORE_WAITING_OFFSET");
   static_assert(offsetof(Header, max_count) == SEMAPHORE_MAX_COUNT_OFFSET,
                 "Semaphore::Header.max_count must be at SEMAPHORE_MAX_COUNT_OFFSET");
   static_assert(offsetof(Header, _padding) == SEMAPHORE_PADDING_OFFSET,
                 "Semaphore::Header._padding must be at SEMAPHORE_PADDING_OFFSET");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Semaphore(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr) {
   }

   /// Core initialization logic for creating a new semaphore.
   std::expected<void, Error> init_create(int32_t initial_count,
                                          int32_t max_count) {
      if (initial_count < 0) {
         return std::unexpected(Error::InvalidArgument);
      }

      if (max_count < 0) {
         return std::unexpected(Error::InvalidArgument);
      }

      if (max_count > 0 && initial_count > max_count) {
         return std::unexpected(Error::InvalidArgument);
      }

      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->count.store(initial_count, std::memory_order_relaxed);
      header_->waiting.store(0, std::memory_order_relaxed);
      header_->max_count = max_count;
      header_->_padding = 0;

      return {};
   }

   /// Core initialization logic for opening an existing semaphore.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      if (size != sizeof(Header)) {
         return std::unexpected(Error::TypeMismatch);
      }

      header_ = memory_.ptr_at<Header>(offset);

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_;

   inline bool try_decrement_count() {
      int32_t current = header_->count.load(std::memory_order_acquire);
      if (current > 0) {
         return header_->count.compare_exchange_weak(
             current, current - 1, std::memory_order_acquire,
             std::memory_order_relaxed);
      }
      return false;
   }

 public:
   /// Create a new semaphore in shared memory.
   /// Returns Error::InvalidArgument if initial_count or max_count are invalid,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Semaphore, Error> create(Memory &memory,
                                                 std::string_view name,
                                                 int32_t initial_count,
                                                 int32_t max_count = 0) {
      Semaphore s(memory, name, FactoryTag{});
      auto result = s.init_create(initial_count, max_count);
      if (!result)
         return std::unexpected(result.error());
      return s;
   }

   /// Open an existing semaphore in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the stored size doesn't match.
   static std::expected<Semaphore, Error> open(Memory &memory,
                                               std::string_view name) {
      Semaphore s(memory, name, FactoryTag{});
      auto result = s.init_open();
      if (!result)
         return std::unexpected(result.error());
      return s;
   }

   /**
    * Create a new semaphore.
    *
    * @param memory Memory instance
    * @param name Unique identifier for this semaphore
    * @param initial_count Initial value for the semaphore count
    * @param max_count Maximum count (0 for unbounded, 1 for binary/mutex)
    */
   Semaphore(Memory &memory, std::string_view name, int32_t initial_count,
             int32_t max_count = 0)
       : Semaphore(memory, name, FactoryTag{}) {

      auto result = init_create(initial_count, max_count);
      if (!result) {
         switch (result.error()) {
         case Error::InvalidArgument:
            throw std::invalid_argument(
                "Initial count or max count is invalid");
         case Error::AllocationOverflow:
            throw std::overflow_error("Semaphore allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Semaphore allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Semaphore name already exists: {}", name));
         default:
            throw std::runtime_error(
                std::format("Failed to allocate semaphore: {}",
                            error_message(result.error())));
         }
      }
   }

   /**
    * Open an existing semaphore.
    *
    * @param memory Memory instance
    * @param name Name of existing semaphore
    */
   Semaphore(Memory &memory, std::string_view name)
       : Semaphore(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(
                std::format("Semaphore not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Invalid semaphore size");
         default:
            throw std::runtime_error(std::format(
                "Semaphore::open failed: {}", error_message(result.error())));
         }
      }
   }

   /**
    * Acquire one permit from the semaphore.
    * Blocks until a permit is available.
    *
    * Uses spin-waiting with exponential backoff to reduce CPU usage.
    */
   void acquire() {
      header_->waiting.fetch_add(1, std::memory_order_relaxed);

      detail::spin_wait([this] { return try_decrement_count(); });

      header_->waiting.fetch_sub(1, std::memory_order_relaxed);
   }

   /**
    * Try to acquire one permit without blocking.
    *
    * @return expected<void, Error> with Success if permit was acquired,
    *         or SemaphoreNotAvailable if count was 0
    */
   [[nodiscard]] std::expected<void, Error> try_acquire() {
      int32_t current = header_->count.load(std::memory_order_acquire);

      while (current > 0) {
         if (header_->count.compare_exchange_weak(current, current - 1,
                                                  std::memory_order_acquire,
                                                  std::memory_order_relaxed)) {
            return std::expected<void, Error>{};
            // std::unreachable: this branch always returns, the retry
            // path below is unreachable when CAS succeeds. Helps the
            // compiler eliminate dead branches in the spin loop.
            std::unreachable();
         }
         // CAS failed, current was updated, retry
      }

      return std::unexpected(Error::SemaphoreNotAvailable);
   }

   /**
    * Try to acquire one permit with a timeout.
    *
    * @param timeout Maximum time to wait
    * @return expected<void, Error> with Success if permit was acquired,
    *         or SemaphoreNotAvailable if timed out
    */
   template <typename Rep, typename Period>
   [[nodiscard]] std::expected<void, Error>
   acquire_for(const std::chrono::duration<Rep, Period> &timeout) {

      header_->waiting.fetch_add(1, std::memory_order_relaxed);

      bool acquired = detail::spin_wait_for(
          [this] { return try_decrement_count(); }, timeout);

      header_->waiting.fetch_sub(1, std::memory_order_relaxed);
      if (acquired) {
         return std::expected<void, Error>{};
      }
      return std::unexpected(Error::SemaphoreNotAvailable);
   }

   /**
    * Release one permit back to the semaphore.
    * Increments the count, potentially waking a waiting process.
    *
    * @throws std::overflow_error if max_count would be exceeded
    */
   void release() {
      int32_t max = header_->max_count;

      // Atomically check and increment to prevent TOCTOU race
      while (true) {
         int32_t current = header_->count.load(std::memory_order_relaxed);
         if (max > 0 && current >= max) {
            throw std::overflow_error("Semaphore count would exceed maximum");
            // std::unreachable: this branch always throws, the CAS attempt
            // below is unreachable when the count would exceed the maximum.
            std::unreachable();
         }
         if (header_->count.compare_exchange_weak(current, current + 1,
                                                  std::memory_order_release,
                                                  std::memory_order_relaxed)) {
            break;
         }
      }
      // Waiting processes will see the incremented count and wake up
   }

   /**
    * Get current semaphore count.
    *
    * @return Current count value
    */
   [[nodiscard]] int32_t count() const {
      return header_->count.load(std::memory_order_acquire);
   }

   /**
    * Get number of processes currently waiting.
    *
    * @return Number of waiting processes
    */
   [[nodiscard]] int32_t waiting() const {
      return header_->waiting.load(std::memory_order_acquire);
   }

   /**
    * Get maximum count (0 = unbounded).
    *
    * @return Maximum count
    */
   [[nodiscard]] int32_t max_count() const {
      return header_->max_count;
   }

   /**
    * Get semaphore name.
    */
   [[nodiscard]] std::string_view name() const {
      return name_;
   }

   /**
    * @brief Destroy the semaphore and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

/**
 * RAII guard for automatic semaphore acquire/release.
 *
 * Acquires the semaphore on construction and releases on destruction.
 *
 * Example:
 *   Semaphore sem(mem, "mutex", 1, 1);
 *   {
 *       SemaphoreGuard guard(sem);
 *       // Critical section - automatically released on scope exit
 *   }
 */
class SemaphoreGuard {
 private:
   Semaphore &sem_;
   bool acquired_;

 public:
   explicit SemaphoreGuard(Semaphore &sem) : sem_(sem), acquired_(true) {
      sem_.acquire();
   }

   ~SemaphoreGuard() {
      if (acquired_) {
         sem_.release();
      }
   }

   // Non-copyable
   SemaphoreGuard(const SemaphoreGuard &) = delete;
   SemaphoreGuard &operator=(const SemaphoreGuard &) = delete;

   // Movable
   SemaphoreGuard(SemaphoreGuard &&other) noexcept
       : sem_(other.sem_), acquired_(other.acquired_) {
      other.acquired_ = false;
   }

   // Move assignment not supported (due to reference member)
   SemaphoreGuard &operator=(SemaphoreGuard &&) = delete;
};

}   // namespace zeroshm
