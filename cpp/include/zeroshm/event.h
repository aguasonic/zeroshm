#pragma once

#include <atomic>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <expected>
#include <format>
#include <stdexcept>
#include <string_view>
#include <utility>

#include <zeroshm/constants_generated.h>

#include "detail/spin_wait.h"
#include "error.h"
#include "memory.h"
#include "semaphore.h"

namespace zeroshm {

/**
 * @brief Event synchronization mode
 */
enum class EventMode : uint32_t {
   AutoReset = 0,    ///< Signal wakes one waiter, auto-resets
   ManualReset = 1   ///< Signal wakes all waiters, stays signaled
};

/**
 * @brief Event state structure for shared memory
 *
 * Binary layout:
 * - 4 bytes: signaled flag (0 = not signaled, 1 = signaled)
 * - 4 bytes: mode (AutoReset or ManualReset)
 * - 4 bytes: waiting count
 * - Variable: Semaphore (for blocking)
 */
struct EventState {
   std::atomic<uint32_t> signaled{0};
   uint32_t mode;
   std::atomic<uint32_t> waiting{0};

   EventState(EventMode m)
       : signaled(0), mode(std::to_underlying(m)), waiting(0) {
   }
};

static_assert(std::is_trivially_copyable_v<EventState>,
              "EventState must be trivially copyable");

/// Compile-time validation: EventState must be exactly 12 bytes and match
/// the layout expected by Go/Rust implementations.
static_assert(sizeof(EventState) == 12,
              "EventState size mismatch -- binary format incompatibility");
static_assert(offsetof(EventState, signaled) == EVENT_SIGNALED_OFFSET,
              "EventState.signaled must be at EVENT_SIGNALED_OFFSET");
static_assert(offsetof(EventState, mode) == EVENT_MODE_OFFSET,
              "EventState.mode must be at EVENT_MODE_OFFSET");
static_assert(offsetof(EventState, waiting) == EVENT_WAITING_OFFSET,
              "EventState.waiting must be at EVENT_WAITING_OFFSET");

/**
 * @brief Event synchronization primitive for shared memory
 *
 * Provides manual-reset and auto-reset event semantics similar to
 * Win32 Events or POSIX condition variables.
 *
 * - **AutoReset**: signal() wakes one waiter, then auto-resets
 * - **ManualReset**: signal() wakes all waiters, stays signaled until reset()
 *
 * Example:
 * @code
 * zeroshm::Memory mem("/sync", 1024 * 1024);
 * zeroshm::Event ready(mem, "ready", zeroshm::EventMode::ManualReset);
 *
 * // Process A - waits for ready signal
 * ready.wait();
 * process_data();
 *
 * // Process B - signals when ready
 * prepare_data();
 * ready.signal();
 * @endcode
 */
class Event {
 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Event(Memory &mem, std::string_view name, FactoryTag)
       : mem_(mem), name_(name), state_(nullptr), sem_(nullptr) {
   }

   /// Core initialization logic for creating a new event.
   std::expected<void, Error> init_create(EventMode mode) {
      size_t total_size = sizeof(EventState);
      auto alloc_result = mem_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      state_ = mem_.ptr_at<EventState>(offset);

      // Initialize state
      new (state_) EventState(mode);

      // Create semaphore for blocking (initially locked)
      std::string sem_name = std::string(name_) + "_sem";
      auto sem_result = Semaphore::create(mem_, sem_name, 0);
      if (!sem_result) {
         return std::unexpected(sem_result.error());
      }
      sem_ = std::make_unique<Semaphore>(std::move(*sem_result));

      return {};
   }

   /// Core initialization logic for opening an existing event.
   std::expected<void, Error> init_open() {
      auto find_result = mem_.table()->find(name_);
      if (!find_result) {
         return std::unexpected(Error::NotFound);
      }

      state_ = mem_.ptr_at<EventState>((*find_result)->offset);

      // Find the semaphore
      std::string sem_name = std::string(name_) + "_sem";
      auto sem_result = Semaphore::open(mem_, sem_name);
      if (!sem_result) {
         return std::unexpected(sem_result.error());
      }
      sem_ = std::make_unique<Semaphore>(std::move(*sem_result));

      return {};
   }

   Memory &mem_;
   std::string name_;
   EventState *state_;
   std::unique_ptr<Semaphore> sem_;

   bool is_manual_reset() const {
      return state_->mode == std::to_underlying(EventMode::ManualReset);
   }

   inline void reset_event() {
      if (state_->mode == std::to_underlying(EventMode::AutoReset)) {
         state_->signaled.store(0, std::memory_order_release);
      }
   }

 public:
   /// Create a new event in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Event, Error>
   create(Memory &mem, std::string_view name,
          EventMode mode = EventMode::AutoReset) {
      Event e(mem, name, FactoryTag{});
      auto result = e.init_create(mode);
      if (!result)
         return std::unexpected(result.error());
      return e;
   }

   /// Open an existing event in shared memory.
   /// Returns Error::NotFound if the name doesn't exist.
   static std::expected<Event, Error> open(Memory &mem, std::string_view name) {
      Event e(mem, name, FactoryTag{});
      auto result = e.init_open();
      if (!result)
         return std::unexpected(result.error());
      return e;
   }

   /**
    * @brief Create or open an Event
    * @param mem Memory region
    * @param name Unique name for this event
    * @param mode Event mode (only used when creating new event)
    * @throws std::runtime_error if allocation fails
    */
   Event(Memory &mem, std::string_view name,
         EventMode mode = EventMode::AutoReset)
       : Event(mem, name, FactoryTag{}) {

      auto find_result = mem_.table()->find(name_);

      if (find_result) {
         auto result = init_open();
         if (!result) {
            switch (result.error()) {
            case Error::NotFound:
               throw std::runtime_error(
                   std::format("Event semaphore not found: {}", name));
            case Error::TypeMismatch:
               throw std::runtime_error("Invalid event semaphore size");
            default:
               throw std::runtime_error(std::format(
                   "Event::open failed: {}", error_message(result.error())));
            }
         }
      } else {
         auto result = init_create(mode);
         if (!result) {
            switch (result.error()) {
            case Error::AllocationOverflow:
               throw std::overflow_error("Event allocation overflow");
            case Error::BoundsExceeded:
               throw std::runtime_error("Event allocation exceeds bounds");
            case Error::NameAlreadyExists:
               throw std::runtime_error(
                   std::format("Event name already exists: {}", name));
            default:
               throw std::runtime_error(
                   std::format("Failed to allocate event: {}",
                               error_message(result.error())));
            }
         }
      }
   }

   /**
    * @brief Signal the event
    *
    * - AutoReset: Wakes one waiting thread (semaphore acts as the event)
    * - ManualReset: Wakes all waiting threads, stays signaled
    */
   void signal() {
      state_->signaled.store(1, std::memory_order_release);

      if (!is_manual_reset()) {
         // AutoReset: also release semaphore to wake one waiter
         sem_->release();
      }
   }

   /**
    * @brief Wait for the event to be signaled
    *
    * Blocks until signal() is called. For AutoReset events, consuming
    * the signal is automatic (semaphore). For ManualReset events, the
    * event stays signaled until reset() is called.
    */
   void wait() {
      if (is_manual_reset()) {
         state_->waiting.fetch_add(1, std::memory_order_relaxed);
         detail::spin_wait([this] { return is_signaled(); });
         state_->waiting.fetch_sub(1, std::memory_order_release);
      } else {
         sem_->acquire();
         reset_event();
      }
   }

   /**
    * @brief Wait with timeout
    * @param timeout Maximum time to wait
    * @return expected<void, Error> with Success if signaled,
    *         or EventNotSignaled if timeout
    */
   template <typename Rep, typename Period>
   [[nodiscard]] std::expected<void, Error>
   wait_for(const std::chrono::duration<Rep, Period> &timeout) {
      if (is_manual_reset()) {
         state_->waiting.fetch_add(1, std::memory_order_relaxed);
         bool signaled = detail::spin_wait_for(
             [this] { return is_signaled(); }, timeout);
         state_->waiting.fetch_sub(1, std::memory_order_release);
         if (signaled) {
            return std::expected<void, Error>{};
         }
         return std::unexpected(Error::EventNotSignaled);
      } else {
         auto result = sem_->acquire_for(timeout);
         if (result) {
            reset_event();
            return std::expected<void, Error>{};
         }
         return std::unexpected(Error::EventNotSignaled);
      }
   }

   /**
    * @brief Reset the event to non-signaled state
    *
    * Only meaningful for ManualReset events. AutoReset events
    * reset automatically.
    */
   void reset() {
      state_->signaled.store(0, std::memory_order_release);
   }

   /**
    * @brief Pulse the event (signal then reset after all waiters wake)
    *
    * Wakes all waiting threads then resets once they've all seen the signal.
    * Only meaningful for ManualReset events.
    */
   void pulse() {
      signal();
      // Wait for all registered waiters to see the signal and unregister
      detail::spin_wait([this] {
         return state_->waiting.load(std::memory_order_acquire) == 0;
      });
      reset();
   }

   /**
    * @brief Check if event is currently signaled
    * @return true if signaled, false otherwise
    */
   [[nodiscard]] bool is_signaled() const {
      return state_->signaled.load(std::memory_order_acquire) == 1;
   }

   /**
    * @brief Destroy the event and remove it from the shared memory table.
    *
    * Destroys the internal semaphore and removes the event table entry
    * so the name can be reused.
    */
   void destroy() {
      if (sem_) {
         sem_->destroy();
      }
      (void)mem_.deallocate(name_);
   }
};

}   // namespace zeroshm
