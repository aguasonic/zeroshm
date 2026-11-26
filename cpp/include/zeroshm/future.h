#pragma once

#include <atomic>
#include <chrono>
#include <cstddef>
#include <cstring>
#include <expected>
#include <format>
#include <thread>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"

namespace zeroshm {

/**
 * @brief Shared memory Future for asynchronous computation results
 *
 * Future represents a value that will be available at some point in the future.
 * It enables asynchronous computation patterns where one process can compute
 * a value while others wait for the result. This is the shared memory
 * equivalent of std::future but works across process boundaries.
 *
 * @motivation
 * In distributed simulations and parallel processing, we often need to:
 * - Decouple producers and consumers of computation results
 * - Allow multiple processes to wait on the same result
 * - Chain computations without blocking (continuation-passing style)
 * - Handle timeouts and error conditions gracefully
 *
 * @example
 * ```cpp
 * // Process A: Producer
 * Memory mem("/simulation", 10*1024*1024);
 * Future<double> energy_future(mem, "total_energy");
 *
 * // Compute expensive result
 * double energy = compute_system_energy();
 * energy_future.set_value(energy);
 *
 * // Process B: Consumer
 * Memory mem("/simulation");
 * Future<double> energy_future(mem, "total_energy", true);  // open existing
 *
 * // Wait for result (blocks)
 * double energy = energy_future.get();
 *
 * // Or try with timeout
 * if (auto energy = energy_future.get_for(100ms)) {
 *     process_energy(*energy);
 * }
 * ```
 *
 * @tparam T Type of the future value (must be trivially copyable)
 */
template <typename T> class Future {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");

   enum State : uint32_t { PENDING = 0, COMPUTING = 1, READY = 2, ERROR = 3 };

   struct Header {
      std::atomic<State> state;
      std::atomic<uint32_t> waiters;
      std::atomic<uint64_t> completion_time;   // For timeout support
      T value;
      char error_msg[256];
   };

   /// Compile-time validation: Future::Header fixed portion must match the
   /// layout expected by Go/Rust implementations.
   static_assert(offsetof(Header, state) == FUTURE_STATE_OFFSET,
                 "Future::Header.state must be at FUTURE_STATE_OFFSET");
   static_assert(offsetof(Header, waiters) == FUTURE_WAITERS_OFFSET,
                 "Future::Header.waiters must be at FUTURE_WAITERS_OFFSET");
   static_assert(offsetof(Header, completion_time) == FUTURE_COMPLETION_TIME_OFFSET,
                 "Future::Header.completion_time must be at FUTURE_COMPLETION_TIME_OFFSET");

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Future(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr) {
   }

   /// Core initialization logic for creating a new future.
   std::expected<void, Error> init_create() {
      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->state.store(PENDING, std::memory_order_relaxed);
      header_->waiters.store(0, std::memory_order_relaxed);
      header_->completion_time.store(0, std::memory_order_relaxed);
      std::memset(header_->error_msg, 0, sizeof(header_->error_msg));

      return {};
   }

   /// Core initialization logic for opening an existing future.
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
   Header *header_ = nullptr;

   void wait_ready() {
      header_->waiters.fetch_add(1, std::memory_order_relaxed);

      while (!is_ready()) {
         // In real impl: use futex_wait or condition variable
         std::this_thread::yield();
      }

      header_->waiters.fetch_sub(1, std::memory_order_relaxed);
   }

   bool wait_ready_for(std::chrono::milliseconds timeout) {
      auto start = std::chrono::steady_clock::now();
      header_->waiters.fetch_add(1, std::memory_order_relaxed);

      while (!is_ready()) {
         if (std::chrono::steady_clock::now() - start > timeout) {
            header_->waiters.fetch_sub(1, std::memory_order_relaxed);
            return false;
         }
         std::this_thread::yield();
      }

      header_->waiters.fetch_sub(1, std::memory_order_relaxed);
      return true;
   }

   void wake_waiters() {
      // In real impl: use futex_wake or condition variable notify_all
      // For now, waiters will notice state change on next check
   }

 public:
   /// Create a new future in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Future<T>, Error> create(Memory &memory,
                                                 std::string_view name) {
      Future<T> f(memory, name, FactoryTag{});
      auto result = f.init_create();
      if (!result)
         return std::unexpected(result.error());
      return f;
   }

   /// Open an existing future in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the stored size doesn't match.
   static std::expected<Future<T>, Error> open(Memory &memory,
                                               std::string_view name) {
      Future<T> f(memory, name, FactoryTag{});
      auto result = f.init_open();
      if (!result)
         return std::unexpected(result.error());
      return f;
   }

   // Create new future
   Future(Memory &memory, std::string_view name)
       : Future(memory, name, FactoryTag{}) {

      auto result = init_create();
      if (!result) {
         switch (result.error()) {
         case Error::AllocationOverflow:
            throw std::overflow_error("Future allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Future allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Future name already exists: {}", name));
         default:
            throw std::runtime_error(
                std::format("Failed to allocate future: {}",
                            error_message(result.error())));
         }
      }
   }

   // Open existing future
   Future(Memory &memory, std::string_view name, bool)
       : Future(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(std::format("Future not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Invalid future size");
         default:
            throw std::runtime_error(std::format(
                "Future::open failed: {}", error_message(result.error())));
         }
      }
   }

   // Set the value (can only be called once)
   [[nodiscard]] bool set_value(const T &value) {
      State expected = PENDING;
      if (!header_->state.compare_exchange_strong(expected, COMPUTING,
                                                  std::memory_order_acquire,
                                                  std::memory_order_relaxed)) {
         return false;   // Already set or in error
      }

      header_->value = value;
      header_->completion_time.store(
          std::chrono::steady_clock::now().time_since_epoch().count(),
          std::memory_order_relaxed);

      header_->state.store(READY, std::memory_order_release);

      // Wake any waiters (in real impl, would use futex or condvar)
      wake_waiters();
      return true;
   }

   // Set error state
   [[nodiscard]] bool set_error(const std::string &error) {
      State expected = PENDING;
      if (!header_->state.compare_exchange_strong(expected, ERROR,
                                                  std::memory_order_acq_rel,
                                                  std::memory_order_relaxed)) {
         return false;
      }

      std::strncpy(header_->error_msg, error.c_str(),
                   sizeof(header_->error_msg) - 1);
      header_->completion_time.store(
          std::chrono::steady_clock::now().time_since_epoch().count(),
          std::memory_order_relaxed);

      wake_waiters();
      return true;
   }

   // Get the value (blocks until ready)
   [[nodiscard]] T get() {
      wait_ready();

      State state = header_->state.load(std::memory_order_acquire);
      if (state == ERROR) {
         throw std::runtime_error(header_->error_msg);
      }
      return header_->value;
   }

   // Try to get value without blocking
   [[nodiscard]] std::expected<T, Error> try_get() {
      State state = header_->state.load(std::memory_order_acquire);
      if (state == READY) {
         return header_->value;
      }
      if (state == ERROR) {
         throw std::runtime_error(header_->error_msg);
      }
      return std::unexpected(Error::FutureNotReady);
   }

   // Wait with timeout
   [[nodiscard]] std::expected<T, Error>
   get_for(std::chrono::milliseconds timeout) {
      if (wait_ready_for(timeout)) {
         State state = header_->state.load(std::memory_order_acquire);
         if (state == ERROR) {
            throw std::runtime_error(header_->error_msg);
         }
         return header_->value;
      }
      return std::unexpected(Error::FutureNotReady);
   }

   // Check if ready
   [[nodiscard]] bool is_ready() const {
      State state = header_->state.load(std::memory_order_acquire);
      return state == READY || state == ERROR;
   }

   // Get current state
   [[nodiscard]] State state() const {
      return header_->state.load(std::memory_order_acquire);
   }

   // Then combinator for chaining (simplified version)
   template <typename F>
   Future<std::invoke_result_t<F, T>>
   then(Memory &mem, const std::string &next_name, F &&func) {
      using U = std::invoke_result_t<F, T>;
      Future<U> next(mem, next_name);

      // In a real implementation, this would register a continuation
      // For now, we'll do a simple synchronous version
      if (is_ready()) {
         try {
            U result = func(get());
            next.set_value(result);
         } catch (const std::exception &e) {
            next.set_error(e.what());
         }
      }

      return next;
   }

   /**
    * @brief Destroy the future and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
