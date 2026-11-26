#pragma once

#include <string_view>
#include <system_error>
#include <utility>

namespace zeroshm {

/**
 * Unified error type for ZeroSHM operations.
 *
 * This enum class replaces ad-hoc exception throwing and bool return values
 * in the public API. Errors are returned via `std::expected<T, Error>` at
 * the API boundary, keeping error handling explicit and exception-free in
 * hot paths.
 */
enum class Error : int {
   Success = 0,

   // Table errors
   NotFound = 1,            // Entry not found in table
   TableFull = 2,           // Table has reached maximum capacity
   NameTooLong = 3,         // Name exceeds maximum length (254 chars)
   NameAlreadyExists = 4,   // Entry with this name already exists

   // Memory allocation errors
   AllocationOverflow = 5,   // Allocation size would cause integer overflow
   BoundsExceeded = 6,       // Allocation would exceed shared memory bounds

   // Queue errors
   QueueFull = 7,    // Queue is full, cannot push
   QueueEmpty = 8,   // Queue is empty, cannot pop

   // Type/size mismatch errors
   TypeMismatch = 9,       // Element type size doesn't match stored value
   InvalidCapacity = 10,   // Capacity is not valid (e.g., not power of two)

   // Stack errors
   StackFull = 11,        // Stack push when at capacity
   StackUnderflow = 12,   // Stack pop when empty

   // Map errors
   MapFull = 13,       // Map insert when table is full
   MapNotFound = 14,   // Map erase/find when key doesn't exist

   // Set errors
   SetFull = 15,       // Set insert when table is full
   SetNotFound = 16,   // Set erase/find when element doesn't exist

   // Pool errors
   PoolExhausted = 17,        // Pool acquire when no free slots
   PoolInvalidPointer = 18,   // Pool release with invalid pointer

   // Ring errors
   RingFull = 19,    // Ring push when full
   RingEmpty = 20,   // Ring pop when empty

   // Semaphore errors
   SemaphoreNotAvailable = 21,   // Semaphore try_acquire when count is 0

   // Latch errors
   LatchNotComplete = 22,   // Latch try_wait when not yet reached

   // Event errors
   EventNotSignaled = 23,   // Event try_wait when not signaled

   // Lock errors
   LockNotAcquired = 24,   // Mutex/RwLock try_lock when held

   // Future errors
   FutureNotReady = 25,   // Future get() when not yet set

   // Lazy errors
   LazyNotReady = 26,   // Lazy get() when not yet initialized

   // Channel errors
   ChannelFull = 27,     // Channel try_send when buffer full
   ChannelEmpty = 28,    // Channel try_recv when buffer empty
   ChannelClosed = 29,   // Operation on closed channel

   // Signal errors
   SignalNotChanged = 30,   // Signal wait_for_change timed out without change

   // General validation errors
   InvalidArgument = 31,   // Invalid parameter value (e.g., negative count)
   NameTooShort = 32,      // Name is shorter than minimum length (3 chars)

   // Synchronization errors
   AllocationFailed =
       33,   // Failed to allocate or initialize synchronization primitives
};

/**
 * Get a human-readable message for an error code.
 */
constexpr std::string_view error_message(Error err) {
   switch (err) {
   case Error::Success:
      return "Success";
   case Error::NotFound:
      return "Entry not found";
   case Error::TableFull:
      return "Table is full";
   case Error::NameTooLong:
      return "Name too long (max 254 characters)";
   case Error::NameAlreadyExists:
      return "Name already exists";
   case Error::AllocationOverflow:
      return "Allocation size overflow";
   case Error::BoundsExceeded:
      return "Allocation would exceed memory bounds";
   case Error::QueueFull:
      return "Queue is full";
   case Error::QueueEmpty:
      return "Queue is empty";
   case Error::TypeMismatch:
      return "Type size mismatch";
   case Error::InvalidCapacity:
      return "Invalid capacity";
   case Error::StackFull:
      return "Stack is full";
   case Error::StackUnderflow:
      return "Stack is empty";
   case Error::MapFull:
      return "Map is full";
   case Error::MapNotFound:
      return "Key not found in map";
   case Error::SetFull:
      return "Set is full";
   case Error::SetNotFound:
      return "Element not found in set";
   case Error::PoolExhausted:
      return "Pool is exhausted";
   case Error::PoolInvalidPointer:
      return "Invalid pointer to deallocate";
   case Error::RingFull:
      return "Ring buffer is full";
   case Error::RingEmpty:
      return "Ring buffer is empty";
   case Error::SemaphoreNotAvailable:
      return "Semaphore not available";
   case Error::LatchNotComplete:
      return "Latch not complete";
   case Error::EventNotSignaled:
      return "Event not signaled";
   case Error::LockNotAcquired:
      return "Lock not acquired";
   case Error::FutureNotReady:
      return "Future not ready";
   case Error::LazyNotReady:
      return "Lazy value not yet computed";
   case Error::ChannelFull:
      return "Channel is full";
   case Error::ChannelEmpty:
      return "Channel is empty";
   case Error::ChannelClosed:
      return "Channel is closed";
   case Error::SignalNotChanged:
      return "Signal not changed within timeout";
   case Error::InvalidArgument:
      return "Invalid argument";
   case Error::NameTooShort:
      return "Name too short (min 3 characters)";
   case Error::AllocationFailed:
      return "Failed to initialize synchronization primitives";
   default:
      return "Unknown error";
   }
}

/**
 * Convert an Error to a std::error_code.
 */
inline std::error_code make_error_code(Error err) {
   struct error_category : std::error_category {
      const char *name() const noexcept override {
         return "zeroshm";
      }
      std::string message(int ev) const override {
         return std::string(error_message(static_cast<Error>(ev)));
      }
   };
   static error_category category;
   return std::error_code(std::to_underlying(err), category);
}

}   // namespace zeroshm

namespace std {
template <> struct is_error_code_enum<zeroshm::Error> : true_type {};
}   // namespace std
