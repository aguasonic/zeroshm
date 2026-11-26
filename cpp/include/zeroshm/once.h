#pragma once

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <expected>
#include <format>
#include <stdexcept>
#include <string_view>
#include <thread>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"

namespace zeroshm {

/**
 * @brief Once flag for one-time initialization across processes
 *
 * Binary layout:
 * - 4 bytes: atomic state (0 = PENDING, 1 = EXECUTING, 2 = DONE)
 */
struct OnceFlag {
   std::atomic<uint32_t> state{0};

   OnceFlag() : state(0) {
   }
};

static_assert(std::is_trivially_copyable_v<OnceFlag>,
              "OnceFlag must be trivially copyable");

/// Compile-time validation: OnceFlag must be exactly 4 bytes and match
/// the layout expected by Go/Rust implementations.
static_assert(sizeof(OnceFlag) == 4,
              "OnceFlag size mismatch -- binary format incompatibility");
static_assert(offsetof(OnceFlag, state) == ONCE_STATE_OFFSET,
              "OnceFlag.state must be at ONCE_STATE_OFFSET");

/**
 * @brief One-time initialization primitive for shared memory
 *
 * Ensures a function is executed exactly once across all processes,
 * similar to std::call_once. Thread-safe and process-safe.
 *
 * Uses 3-state protocol to ensure callers wait for completion:
 * - PENDING (0): No one has started execution yet
 * - EXECUTING (1): A thread/process is currently running the function
 * - DONE (2): The function has completed execution
 *
 * Binary layout: 4 bytes (OnceFlag)
 *
 * Example:
 * @code
 * zeroshm::Memory mem("/config", 1024);
 * zeroshm::Once init(mem, "initialize");
 *
 * // This will execute exactly once across all processes
 * init.call([]() {
 *     std::cout << "Initializing shared resources...\n";
 *     // Expensive initialization here
 * });
 * @endcode
 */
class Once {
 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Once(Memory &mem, std::string_view name, FactoryTag)
       : mem_(mem), name_(name), flag_(nullptr) {
   }

   /// Core initialization logic for creating a new once flag.
   std::expected<void, Error> init_create() {
      auto alloc_result = mem_.allocate(name_, sizeof(OnceFlag));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      flag_ = mem_.ptr_at<OnceFlag>(offset);

      // Initialize to PENDING state
      new (flag_) OnceFlag();

      return {};
   }

   /// Core initialization logic for opening an existing once flag.
   std::expected<void, Error> init_open() {
      auto find_result = mem_.table()->find(name_);
      if (!find_result) {
         return std::unexpected(Error::NotFound);
      }

      flag_ = mem_.ptr_at<OnceFlag>((*find_result)->offset);

      return {};
   }

   Memory &mem_;
   std::string name_;
   OnceFlag *flag_;

 public:
   // State constants
   static constexpr uint32_t STATE_PENDING = 0;
   static constexpr uint32_t STATE_EXECUTING = 1;
   static constexpr uint32_t STATE_DONE = 2;

   /// Create a new once flag in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Once, Error> create(Memory &mem,
                                            std::string_view name) {
      Once o(mem, name, FactoryTag{});
      auto result = o.init_create();
      if (!result)
         return std::unexpected(result.error());
      return o;
   }

   /// Open an existing once flag in shared memory.
   /// Returns Error::NotFound if the name doesn't exist.
   static std::expected<Once, Error> open(Memory &mem, std::string_view name) {
      Once o(mem, name, FactoryTag{});
      auto result = o.init_open();
      if (!result)
         return std::unexpected(result.error());
      return o;
   }

   /**
    * @brief Create or open a Once flag
    * @param mem Memory region
    * @param name Unique name for this once flag
    * @throws std::runtime_error if allocation fails
    */
   Once(Memory &mem, std::string_view name) : Once(mem, name, FactoryTag{}) {

      auto find_result = mem_.table()->find(name_);

      if (find_result) {
         auto result = init_open();
         if (!result) {
            throw std::runtime_error(
                std::format("Once flag not found: {}", name));
         }
      } else {
         auto result = init_create();
         if (!result) {
            switch (result.error()) {
            case Error::AllocationOverflow:
               throw std::overflow_error("Once flag allocation overflow");
            case Error::BoundsExceeded:
               throw std::runtime_error("Once flag allocation exceeds bounds");
            case Error::NameAlreadyExists:
               throw std::runtime_error(
                   std::format("Once flag name already exists: {}", name));
            default:
               throw std::runtime_error(
                   std::format("Failed to allocate once flag: {}",
                               error_message(result.error())));
            }
         }
      }
   }

   /**
    * @brief Execute function exactly once
    *
    * If this is the first call, executes func. Otherwise, blocks until
    * the first call completes, then returns without executing func.
    *
    * @param func Callable to execute once
    *
    * Note: If func throws an exception, the once flag is still marked
    * as done (matching std::call_once behavior).
    */
   template <typename F> void call(F &&func) {
      // Fast path: already done
      if (flag_->state.load(std::memory_order_acquire) == STATE_DONE) {
         return;
      }

      // Try to transition from PENDING to EXECUTING
      uint32_t expected = STATE_PENDING;
      if (flag_->state.compare_exchange_strong(expected, STATE_EXECUTING,
                                               std::memory_order_acq_rel,
                                               std::memory_order_acquire)) {

         // We won the race - execute the function
         try {
            func();
         } catch (...) {
            // Even if func throws, mark as done (std::call_once semantics)
            flag_->state.store(STATE_DONE, std::memory_order_release);
            throw;
         }

         // Transition from EXECUTING to DONE
         flag_->state.store(STATE_DONE, std::memory_order_release);
      } else {
         // We lost the race - spin-wait until the executor finishes
         while (flag_->state.load(std::memory_order_acquire) ==
                STATE_EXECUTING) {
            std::this_thread::yield();
         }
      }
   }

   /**
    * @brief Check if the once flag has been called
    * @return true if call() has completed execution, false otherwise
    */
   [[nodiscard]] bool already_called() const {
      return flag_->state.load(std::memory_order_acquire) == STATE_DONE;
   }

   /**
    * @brief Reset the once flag (testing/debugging only!)
    *
    * WARNING: Not thread-safe. Only use when you know no other
    * threads/processes are accessing this once flag.
    */
   void reset_unsafe() {
      flag_->state.store(STATE_PENDING, std::memory_order_release);
   }

   /**
    * @brief Destroy the once flag and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)mem_.deallocate(name_);
   }
};

}   // namespace zeroshm
