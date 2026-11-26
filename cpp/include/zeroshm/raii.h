#pragma once

#include <memory>

namespace zeroshm {

/// Custom deleter for shared memory objects.
/// When the unique_ptr goes out of scope, it calls destroy() on the object.
///
/// This requires the structure to be movable (i.e., it must have a move
/// constructor). If the structure holds a Memory& reference member, it is
/// not movable by default, and the unique_ptr approach won't work directly.
/// In that case, use SharedMemoryGuard instead.
///
/// Usage with movable structures:
/// ```cpp
/// auto result = Queue<int>::create(mem, "my_queue", 64);
/// if (result) {
///     auto queue_ptr = std::unique_ptr<Queue<int>, SharedMemoryDeleter>(
///         std::make_unique<Queue<int>>(std::move(*result)),
///         SharedMemoryDeleter{}
///     );
///     // queue_ptr automatically calls destroy() when it goes out of scope
/// }
/// ```
struct SharedMemoryDeleter {
   template <typename T> void operator()(T *ptr) const {
      if (ptr) {
         ptr->destroy();
      }
   }
};

/// RAII wrapper for shared memory objects.
/// Automatically calls destroy() when the guard goes out of scope.
///
/// Unlike SharedMemoryDeleter, this does not require the structure to be
/// movable. It holds a reference to the object and calls destroy() in its
/// destructor. This works with the current structure implementations that
/// hold Memory& reference members.
///
/// Usage:
/// ```cpp
/// auto result = Queue<int>::create(mem, "my_queue", 64);
/// if (result) {
///     SharedMemoryGuard guard(*result);
///     guard->push(42);
///     // guard automatically calls destroy() when it goes out of scope
/// }
/// ```
///
/// The guard is non-copyable and non-movable to ensure single ownership
/// and prevent double-destruction.
template <typename T> class SharedMemoryGuard {
 private:
   T &obj_;

 public:
   explicit SharedMemoryGuard(T &obj) : obj_(obj) {
   }

   ~SharedMemoryGuard() {
      obj_.destroy();
   }

   // Non-copyable, non-movable
   SharedMemoryGuard(const SharedMemoryGuard &) = delete;
   SharedMemoryGuard &operator=(const SharedMemoryGuard &) = delete;
   SharedMemoryGuard(SharedMemoryGuard &&) = delete;
   SharedMemoryGuard &operator=(SharedMemoryGuard &&) = delete;

   T &get() {
      return obj_;
   }
   const T &get() const {
      return obj_;
   }
   T &operator*() {
      return obj_;
   }
   const T &operator*() const {
      return obj_;
   }
   T *operator->() {
      return &obj_;
   }
   const T *operator->() const {
      return &obj_;
   }
};

}   // namespace zeroshm
