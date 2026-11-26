#pragma once

#include <atomic>
#include <chrono>
#include <expected>
#include <format>
#include <functional>
#include <stdexcept>
#include <string_view>
#include <vector>

#include "error.h"
#include "memory.h"
#include "mutex.h"

namespace zeroshm {

/**
 * @brief Signal state for shared memory
 *
 * Binary layout:
 * - 8 bytes: version (increments on each change)
 * - T bytes: value
 */
template <typename T> struct SignalState {
   std::atomic<uint64_t> version{0};
   T value;

   SignalState() : version(0), value() {
   }
   explicit SignalState(const T &val) : version(0), value(val) {
   }
};

/**
 * @brief Reactive signal for fine-grained reactivity across processes
 *
 * A Signal<T> stores a value that can be observed for changes. When the value
 * changes, the version number increments, allowing other processes to detect
 * and react to changes.
 *
 * Features:
 * - Fine-grained reactivity (SolidJS/Preact style)
 * - Version tracking for efficient polling
 * - Cross-process change detection
 * - Local callbacks via on_change()
 * - Atomic updates via update()
 *
 * This enables reactive programming patterns across shared memory!
 *
 * Example:
 * @code
 * zeroshm::Memory mem("/data", 1024 * 1024);
 * zeroshm::Signal<int> counter(mem, "counter", 0);
 *
 * // Process A - producer
 * counter.set(counter.get() + 1);
 *
 * // Process B - reactive consumer
 * uint64_t last_version = counter.version();
 * while (true) {
 *     if (counter.has_changed(last_version)) {
 *         std::cout << "Counter changed to: " << counter.get() << "\n";
 *         last_version = counter.version();
 *     }
 * }
 *
 * // Or wait for changes
 * counter.wait_for_change(last_version, std::chrono::seconds(1));
 * @endcode
 */
template <typename T> class Signal {
 private:
   /// Tag types for constructor dispatch
   struct FactoryTag {};
   struct OpenExistingTag {};

   /// Constructor for factory create
   Signal(Memory &mem, std::string_view name, const T &initial_value,
          FactoryTag)
       : mem_(mem), name_(name), initial_value_(initial_value),
         create_if_missing_(true) {

      std::string state_name = std::string(name) + "_state";
      std::string mutex_name = std::string(name) + "_mtx";

      init_create(state_name, mutex_name, initial_value);
   }

   /// Constructor for factory open
   Signal(Memory &mem, std::string_view name, const T &, OpenExistingTag)
       : mem_(mem), name_(name), create_if_missing_(false) {

      std::string state_name = std::string(name) + "_state";
      std::string mutex_name = std::string(name) + "_mtx";

      init_open(state_name, mutex_name);
   }

   void init_create(const std::string &state_name,
                    const std::string &mutex_name, const T &initial_value) {
      if (!create_if_missing_) {
         throw std::runtime_error(std::format("Signal not found: {}", name_));
      }

      auto state_alloc = mem_.allocate(state_name, sizeof(SignalState<T>));
      if (!state_alloc) {
         throw std::runtime_error(
             std::format("Failed to allocate signal state: {}",
                         error_message(state_alloc.error())));
      }
      size_t state_offset = *state_alloc;
      state_ = mem_.ptr_at<SignalState<T>>(state_offset);
      new (state_) SignalState<T>(initial_value);

      mutex_ = std::make_unique<Mutex>(mem_, mutex_name);

      auto marker_alloc = mem_.allocate(name_, 1);
      if (!marker_alloc) {
         throw std::runtime_error(
             std::format("Failed to allocate signal marker: {}",
                         error_message(marker_alloc.error())));
      }
      (void)*marker_alloc;
   }

   void init_open(const std::string &state_name,
                  const std::string &mutex_name) {
      auto state_result = mem_.table()->find(state_name);
      if (!state_result) {
         throw std::runtime_error("Signal state not found");
      }

      state_ = mem_.ptr_at<SignalState<T>>((*state_result)->offset);

      mutex_ = std::make_unique<Mutex>(mem_, mutex_name);
   }

   Memory &mem_;
   std::string name_;
   T initial_value_;
   bool create_if_missing_;
   SignalState<T> *state_;
   std::unique_ptr<Mutex> mutex_;
   std::vector<std::function<void(const T &)>> callbacks_;

 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "Signal type must be trivially copyable");

   /// Create a new signal in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Signal, Error>
   create(Memory &mem, std::string_view name, const T &initial_value = T()) {
      try {
         return Signal(mem, name, initial_value, FactoryTag{});
      } catch (const std::overflow_error &) {
         return std::unexpected(Error::AllocationOverflow);
      } catch (const std::runtime_error &) {
         return std::unexpected(Error::BoundsExceeded);
      }
   }

   /// Open an existing signal in shared memory.
   /// Returns Error::NotFound if the name doesn't exist.
   static std::expected<Signal, Error> open(Memory &mem,
                                            std::string_view name) {
      try {
         return Signal(mem, name, T(), OpenExistingTag{});
      } catch (const std::runtime_error &) {
         return std::unexpected(Error::NotFound);
      }
   }

   /**
    * @brief Create or open a Signal
    * @param mem Memory region
    * @param name Unique name for this signal
    * @param initial_value Initial value (only used if creating new signal)
    * @param create_if_missing If true, creates new signal; if false, opens
    * existing
    */
   Signal(Memory &mem, std::string_view name, const T &initial_value,
          bool create_if_missing)
       : mem_(mem), name_(name), initial_value_(initial_value),
         create_if_missing_(create_if_missing) {

      std::string state_name = std::string(name) + "_state";
      std::string mutex_name = std::string(name) + "_mtx";

      auto find_result = mem.table()->find(name);

      if (find_result) {
         init_open(state_name, mutex_name);
      } else {
         init_create(state_name, mutex_name, initial_value);
      }
   }

   /**
    * @brief Create a new Signal
    * @param mem Memory region
    * @param name Unique name for this signal
    * @param initial_value Initial value
    */
   Signal(Memory &mem, std::string_view name, const T &initial_value = T())
       : Signal(mem, name, initial_value, true) {
   }

   /// Tag type for opening an existing signal (public API for constructor
   /// dispatch)
   struct OpenExisting {};

   /**
    * @brief Open an existing Signal (tag-based overload)
    */
   Signal(Memory &mem, std::string_view name, OpenExisting)
       : Signal(mem, name, T(), false) {
   }

   /**
    * @brief Get current value
    * @return Copy of current value
    */
   [[nodiscard]] T get() const {
      mutex_->lock();
      T value = state_->value;
      mutex_->unlock();
      return value;
   }

   /**
    * @brief Set new value
    *
    * Atomically sets the value and increments the version.
    * Triggers all registered callbacks.
    *
    * @param new_value New value to set
    */
   void set(const T &new_value) {
      mutex_->lock();
      state_->value = new_value;
      state_->version.fetch_add(1, std::memory_order_release);
      mutex_->unlock();

      // Trigger local callbacks
      for (auto &callback : callbacks_) {
         callback(new_value);
      }
   }

   /**
    * @brief Atomic update with function
    *
    * Applies a function to the current value atomically.
    *
    * @param func Function that takes current value and returns new value
    *
    * Example:
    * @code
    * counter.update([](int val) { return val + 1; });
    * @endcode
    */
   template <typename F> void update(F &&func) {
      mutex_->lock();
      state_->value = func(state_->value);
      state_->version.fetch_add(1, std::memory_order_release);
      T new_val = state_->value;
      mutex_->unlock();

      // Trigger local callbacks
      for (auto &callback : callbacks_) {
         callback(new_val);
      }
   }

   /**
    * @brief Get current version number
    *
    * Version increments on each set/update. Use for change detection.
    *
    * @return Current version number
    */
   [[nodiscard]] uint64_t version() const {
      return state_->version.load(std::memory_order_acquire);
   }

   /**
    * @brief Check if signal has changed since a version
    *
    * @param old_version Previous version to compare against
    * @return true if version has incremented
    */
   [[nodiscard]] bool has_changed(uint64_t old_version) const {
      return version() != old_version;
   }

   /**
    * @brief Wait for signal to change
    *
    * Blocks until the signal's version changes from old_version.
    *
    * @param old_version Version to wait for change from
    * @param timeout Maximum time to wait
    * @return expected<void, Error> -- success if changed, SignalNotChanged on
    * timeout
    */
   template <typename Rep, typename Period>
   [[nodiscard]] std::expected<void, Error>
   wait_for_change(uint64_t old_version,
                   const std::chrono::duration<Rep, Period> &timeout) {

      auto deadline = std::chrono::steady_clock::now() + timeout;

      while (std::chrono::steady_clock::now() < deadline) {
         if (has_changed(old_version)) {
            return {};
         }
         std::this_thread::sleep_for(std::chrono::microseconds(100));
      }

      return std::unexpected(Error::SignalNotChanged);
   }

   /**
    * @brief Register a callback for changes (local process only)
    *
    * Callbacks are triggered when set() or update() is called in the
    * current process. Not cross-process.
    *
    * @param callback Function to call on change
    */
   void on_change(std::function<void(const T &)> callback) {
      callbacks_.push_back(std::move(callback));
   }

   /**
    * @brief Destroy the signal and remove it from the shared memory table.
    *
    * Destroys the internal mutex and removes the table entries so
    * the names can be reused.
    */
   void destroy() {
      if (mutex_) {
         mutex_->destroy();
      }
      (void)mem_.deallocate(name_ + "_state");
      (void)mem_.deallocate(name_);
   }

   // Prevent copying
   Signal(const Signal &) = delete;
   Signal &operator=(const Signal &) = delete;

   // Allow moving
   Signal(Signal &&) = default;
   Signal &operator=(Signal &&) = default;
};

}   // namespace zeroshm
