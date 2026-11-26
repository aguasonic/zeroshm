#pragma once

#include <atomic>
#include <chrono>
#include <cstddef>
#include <expected>
#include <format>
#include <stdexcept>

#include <zeroshm/constants_generated.h>

#include "detail/spin_wait.h"
#include "error.h"
#include "memory.h"

namespace zeroshm {

/**
 * Lock-free latch for one-time countdown synchronization.
 *
 * A latch counts down from an initial value to zero. Once it reaches zero,
 * it stays at zero (one-time use). Any number of processes can wait() for
 * the count to reach zero, and any process can count_down() to decrement it.
 *
 * Unlike Barrier which resets and cycles through generations, Latch is
 * single-use and cannot be reset.
 *
 * Thread-safe and process-safe.
 *
 * Common use cases:
 * - Start gate: Initialize with count=1, workers wait(), coordinator counts
 * down
 * - Completion detection: Initialize with count=N, each worker counts down when
 * done
 *
 * Example:
 *   // Wait for all workers to initialize
 *   Latch ready_latch(mem, "workers_ready", num_workers);
 *
 *   // Each worker thread:
 *   initialize();
 *   ready_latch.count_down();  // Signal ready
 *
 *   // Coordinator:
 *   ready_latch.wait();  // Wait for all workers to be ready
 *   start_work();
 */
class Latch {
 public:
   struct Header {
      std::atomic<int32_t> count;   // Current count (counts down to zero)
      int32_t initial_count;        // Initial count value (immutable)
      int32_t _padding[2];          // Alignment padding
   };

   /// Compile-time validation: Latch::Header must be exactly 16 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 16,
                 "Latch::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, count) == LATCH_COUNT_OFFSET,
                 "Latch::Header.count must be at LATCH_COUNT_OFFSET");
   static_assert(offsetof(Header, initial_count) == LATCH_INITIAL_COUNT_OFFSET,
                 "Latch::Header.initial_count must be at LATCH_INITIAL_COUNT_OFFSET");
   static_assert(offsetof(Header, _padding) == LATCH_PADDING0_OFFSET,
                 "Latch::Header._padding[0] must be at LATCH_PADDING0_OFFSET");
   static_assert(offsetof(Header, _padding) + 4 == LATCH_PADDING1_OFFSET,
                 "Latch::Header._padding[1] must be at LATCH_PADDING1_OFFSET");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Latch(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr) {
   }

   /// Core initialization logic for creating a new latch.
   std::expected<void, Error> init_create(int32_t count) {
      if (count < 0) {
         return std::unexpected(Error::InvalidArgument);
      }

      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->count.store(count, std::memory_order_relaxed);
      header_->initial_count = count;
      header_->_padding[0] = 0;
      header_->_padding[1] = 0;

      return {};
   }

   /// Core initialization logic for opening an existing latch.
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

 public:
   /// Create a new latch in shared memory.
   /// Returns Error::InvalidArgument if count is negative,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Latch, Error>
   create(Memory &memory, std::string_view name, int32_t count) {
      Latch l(memory, name, FactoryTag{});
      auto result = l.init_create(count);
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   /// Open an existing latch in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the stored size doesn't match.
   static std::expected<Latch, Error> open(Memory &memory,
                                           std::string_view name) {
      Latch l(memory, name, FactoryTag{});
      auto result = l.init_open();
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   /**
    * Create a new latch.
    *
    * @param memory Memory instance
    * @param name Unique identifier for this latch
    * @param count Initial count value (must be >= 0)
    */
   Latch(Memory &memory, std::string_view name, int32_t count)
       : Latch(memory, name, FactoryTag{}) {

      auto result = init_create(count);
      if (!result) {
         switch (result.error()) {
         case Error::InvalidArgument:
            throw std::invalid_argument("Count must be non-negative");
         case Error::AllocationOverflow:
            throw std::overflow_error("Latch allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Latch allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Latch name already exists: {}", name));
         default:
            throw std::runtime_error(std::format(
                "Failed to allocate latch: {}", error_message(result.error())));
         }
      }
   }

   /**
    * Open an existing latch.
    *
    * @param memory Memory instance
    * @param name Name of existing latch
    */
   Latch(Memory &memory, std::string_view name)
       : Latch(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(std::format("Latch not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Invalid latch size");
         default:
            throw std::runtime_error(std::format(
                "Latch::open failed: {}", error_message(result.error())));
         }
      }
   }

   /**
    * Decrement the count by n (default 1).
    *
    * Atomically decrements the count, saturating at 0. If the count
    * reaches 0, all waiting processes are released.
    *
    * @param n Amount to decrement (default 1, must be > 0)
    */
   void count_down(int32_t n = 1) {
      if (n <= 0) {
         throw std::invalid_argument("count_down amount must be positive");
      }

      int32_t current = header_->count.load(std::memory_order_acquire);

      while (current > 0) {
         int32_t new_count = (current >= n) ? (current - n) : 0;

         if (header_->count.compare_exchange_weak(current, new_count,
                                                  std::memory_order_release,
                                                  std::memory_order_acquire)) {
            return;
         }
         // CAS failed, current was updated, retry
      }
   }

   /**
    * Wait for the count to reach zero.
    *
    * Blocks until the latch count reaches 0. If the count is already 0,
    * returns immediately.
    *
    * Uses spin-waiting with exponential backoff to reduce CPU usage.
    */
   void wait() {
      detail::spin_wait([this] {
         return header_->count.load(std::memory_order_acquire) <= 0;
      });
   }

   /**
    * Try to wait without blocking.
    *
    * @return expected<void, Error> with Success if count is 0,
    *         or LatchNotComplete if still counting down
    */
   [[nodiscard]] std::expected<void, Error> try_wait() const {
      if (header_->count.load(std::memory_order_acquire) == 0) {
         return std::expected<void, Error>{};
      }
      return std::unexpected(Error::LatchNotComplete);
   }

   /**
    * Wait for the count to reach zero with a timeout.
    *
    * @param timeout Maximum time to wait
    * @return expected<void, Error> with Success if count reached 0,
    *         or LatchNotComplete if timed out
    */
   template <typename Rep, typename Period>
   [[nodiscard]] std::expected<void, Error>
   wait_for(const std::chrono::duration<Rep, Period> &timeout) {

      bool complete = detail::spin_wait_for(
          [this] {
             return header_->count.load(std::memory_order_acquire) <= 0;
          },
          timeout);
      if (complete) {
         return std::expected<void, Error>{};
      }
      return std::unexpected(Error::LatchNotComplete);
   }

   /**
    * Get current count value.
    *
    * @return Current count (>= 0)
    */
   [[nodiscard]] int32_t count() const {
      return header_->count.load(std::memory_order_acquire);
   }

   /**
    * Get initial count value.
    *
    * @return Initial count that the latch was created with
    */
   [[nodiscard]] int32_t initial_count() const {
      return header_->initial_count;
   }

   /**
    * Get latch name.
    */
   [[nodiscard]] std::string_view name() const {
      return name_;
   }

   /**
    * Reset the latch to its initial count.
    *
    * WARNING: This is NOT thread-safe if other threads are waiting or
    * counting down. Only use when you have exclusive access or when
    * all other threads have completed their wait().
    *
    * This allows the latch to be reused for multiple synchronization phases.
    */
   void reset() {
      header_->count.store(header_->initial_count, std::memory_order_release);
   }

   /**
    * @brief Destroy the latch and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
