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
 * Lock-free barrier for cross-process synchronization.
 *
 * A barrier synchronizes N processes at a checkpoint. All participants must
 * call wait() before any can proceed. Once all arrive, the barrier releases
 * all waiters simultaneously and automatically resets for the next cycle.
 *
 * The generation counter prevents early arrivals for the next cycle from
 * releasing the current cycle, making the barrier reusable.
 *
 * Thread-safe and process-safe.
 *
 * Example:
 *   // Phase-based parallel algorithm
 *   Barrier barrier(mem, "phase_sync", 4);  // 4 worker processes
 *
 *   while (work_remaining) {
 *       do_phase_1();
 *       barrier.wait();  // All workers must complete phase 1
 *       do_phase_2();
 *       barrier.wait();  // All workers must complete phase 2
 *   }
 */
class Barrier {
 public:
   struct Header {
      std::atomic<int32_t> arrived;   // Number of processes that have arrived
      std::atomic<int32_t> generation;   // Generation counter (for reusability)
      int32_t num_participants;          // Total number of participants
      int32_t _padding;                  // Alignment padding
   };

   /// Compile-time validation: Barrier::Header must be exactly 16 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 16,
                 "Barrier::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, arrived) == BARRIER_ARRIVED_OFFSET,
                 "Barrier::Header.arrived must be at BARRIER_ARRIVED_OFFSET");
   static_assert(offsetof(Header, generation) == BARRIER_GENERATION_OFFSET,
                 "Barrier::Header.generation must be at BARRIER_GENERATION_OFFSET");
   static_assert(offsetof(Header, num_participants) == BARRIER_NUM_PARTICIPANTS_OFFSET,
                 "Barrier::Header.num_participants must be at BARRIER_NUM_PARTICIPANTS_OFFSET");
   static_assert(offsetof(Header, _padding) == BARRIER_PADDING_OFFSET,
                 "Barrier::Header._padding must be at BARRIER_PADDING_OFFSET");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Barrier(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr) {
   }

   /// Core initialization logic for creating a new barrier.
   std::expected<void, Error> init_create(int32_t num_participants) {
      if (num_participants <= 0) {
         return std::unexpected(Error::InvalidArgument);
      }

      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->arrived.store(0, std::memory_order_relaxed);
      header_->generation.store(0, std::memory_order_relaxed);
      header_->num_participants = num_participants;
      header_->_padding = 0;

      return {};
   }

   /// Core initialization logic for opening an existing barrier.
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
   /// Create a new barrier in shared memory.
   /// Returns Error::InvalidArgument if num_participants is not positive,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Barrier, Error>
   create(Memory &memory, std::string_view name, int32_t num_participants) {
      Barrier b(memory, name, FactoryTag{});
      auto result = b.init_create(num_participants);
      if (!result)
         return std::unexpected(result.error());
      return b;
   }

   /// Open an existing barrier in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the stored size doesn't match.
   static std::expected<Barrier, Error> open(Memory &memory,
                                             std::string_view name) {
      Barrier b(memory, name, FactoryTag{});
      auto result = b.init_open();
      if (!result)
         return std::unexpected(result.error());
      return b;
   }

   /**
    * Create a new barrier.
    *
    * @param memory Memory instance
    * @param name Unique identifier for this barrier
    * @param num_participants Number of processes that must arrive before
    * releasing
    */
   Barrier(Memory &memory, std::string_view name, int32_t num_participants)
       : Barrier(memory, name, FactoryTag{}) {

      auto result = init_create(num_participants);
      if (!result) {
         switch (result.error()) {
         case Error::InvalidArgument:
            throw std::invalid_argument(
                "Number of participants must be positive");
         case Error::AllocationOverflow:
            throw std::overflow_error("Barrier allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Barrier allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Barrier name already exists: {}", name));
         default:
            throw std::runtime_error(
                std::format("Failed to allocate barrier: {}",
                            error_message(result.error())));
         }
      }
   }

   /**
    * Open an existing barrier.
    *
    * @param memory Memory instance
    * @param name Name of existing barrier
    */
   Barrier(Memory &memory, std::string_view name)
       : Barrier(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(
                std::format("Barrier not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Invalid barrier size");
         default:
            throw std::runtime_error(std::format(
                "Barrier::open failed: {}", error_message(result.error())));
         }
      }
   }

   /**
    * Wait for all participants to arrive at the barrier.
    *
    * Blocks until all num_participants processes have called wait().
    * Once all arrive, all waiters are released simultaneously and the
    * barrier automatically resets for the next cycle.
    *
    * Uses spin-waiting with exponential backoff to reduce CPU usage.
    */
   void wait() {
      // Capture current generation before arriving
      int32_t my_generation =
          header_->generation.load(std::memory_order_acquire);

      // Atomically increment arrived counter
      int32_t arrived =
          header_->arrived.fetch_add(1, std::memory_order_acq_rel) + 1;

      if (arrived == header_->num_participants) {
         // Last to arrive - reset and release everyone
         header_->arrived.store(0, std::memory_order_relaxed);

         // Increment generation to release waiters
         // Use release ordering so other threads see the reset arrived count
         header_->generation.fetch_add(1, std::memory_order_release);
      } else {
         // Not last - wait for generation to change
         detail::spin_wait([this, my_generation] {
            return header_->generation.load(std::memory_order_acquire) !=
                   my_generation;
         });
      }
   }

   /**
    * Wait for all participants with a timeout.
    *
    * @param timeout Maximum time to wait
    * @return true if barrier released, false if timed out
    *
    * WARNING: If a timeout occurs, the barrier state may be inconsistent.
    * The caller is responsible for coordinating recovery with other processes.
    */
   template <typename Rep, typename Period>
   [[nodiscard]] bool
   wait_for(const std::chrono::duration<Rep, Period> &timeout) {

      auto start = std::chrono::steady_clock::now();

      // Capture current generation before arriving
      int32_t my_generation =
          header_->generation.load(std::memory_order_acquire);

      // Atomically increment arrived counter
      int32_t arrived =
          header_->arrived.fetch_add(1, std::memory_order_acq_rel) + 1;

      if (arrived == header_->num_participants) {
         // Last to arrive - reset and release everyone
         header_->arrived.store(0, std::memory_order_relaxed);
         header_->generation.fetch_add(1, std::memory_order_release);
         return true;
      } else {
         // Not last - wait for generation to change or timeout
         auto remaining = timeout - (std::chrono::steady_clock::now() - start);
         bool released = detail::spin_wait_for(
             [this, my_generation] {
                return header_->generation.load(std::memory_order_acquire) !=
                       my_generation;
             },
             remaining);

         if (!released) {
            // Timeout - decrement arrived count
            // WARNING: This creates a race if the last participant arrives
            // during this window. Use with caution.
            header_->arrived.fetch_sub(1, std::memory_order_acq_rel);
            return false;
         }

         return true;
      }
   }

   /**
    * Get number of processes currently waiting at the barrier.
    *
    * @return Number of arrived processes
    */
   [[nodiscard]] int32_t arrived() const {
      return header_->arrived.load(std::memory_order_acquire);
   }

   /**
    * Get current generation number.
    *
    * The generation increments each time all participants pass through.
    *
    * @return Current generation
    */
   [[nodiscard]] int32_t generation() const {
      return header_->generation.load(std::memory_order_acquire);
   }

   /**
    * Get number of participants required.
    *
    * @return Number of participants
    */
   [[nodiscard]] int32_t num_participants() const {
      return header_->num_participants;
   }

   /**
    * Get barrier name.
    */
   [[nodiscard]] std::string_view name() const {
      return name_;
   }

   /**
    * @brief Destroy the barrier and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
