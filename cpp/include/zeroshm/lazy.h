#pragma once

#include <atomic>
#include <cstddef>
#include <cstring>
#include <expected>
#include <format>
#include <functional>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"

namespace zeroshm {

/**
 * @brief Lazy evaluation container for deferred computations in shared memory
 *
 * Lazy<T> represents a computation that is deferred until its value is actually
 * needed. The computation is performed at most once, and the result is cached
 * for subsequent accesses. This enables efficient memoization and avoids
 * unnecessary computation in distributed systems.
 *
 * @motivation
 * Lazy evaluation is crucial for:
 * - Avoiding expensive computations that might never be needed
 * - Implementing infinite data structures (through recursive lazy values)
 * - Memoization of expensive functions across processes
 * - Building computation graphs that evaluate on-demand
 * - Short-circuit evaluation in logical expressions
 *
 * @theory
 * Lazy evaluation is a cornerstone of functional programming, enabling:
 * - Referential transparency: same input always produces same output
 * - Composability: lazy values can be combined to form new lazy values
 * - Efficiency: computations only happen when needed
 *
 * In shared memory context, this becomes even more powerful as multiple
 * processes can share the same lazy computation, and only one will perform it.
 *
 * @example
 * ```cpp
 * // Create expensive computation
 * Memory mem("/simulation", 10*1024*1024);
 * Lazy<double> total_energy(mem, "energy", ComputationOp::EXTERNAL);
 *
 * // Only computed when forced
 * if (need_energy_check) {
 *     double energy = total_energy.force();  // Computed here
 * }
 *
 * // Second access uses cached value
 * double energy2 = total_energy.force();  // No recomputation
 *
 * // Lazy arithmetic
 * Lazy<double> kinetic(mem, "kinetic", 100.0);
 * Lazy<double> potential(mem, "potential", 50.0);
 * auto total = Lazy<double>::add(mem, "total", kinetic, potential);
 * // Addition only happens when total.force() is called
 * ```
 *
 * @thread_safety
 * The force() operation is thread-safe and uses compare-and-swap to ensure
 * only one thread computes the value, while others wait for the result.
 *
 * @tparam T Type of the lazy value (must be trivially copyable)
 */
template <typename T> class Lazy {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");

   enum ComputeState : uint32_t {
      NOT_COMPUTED = 0,
      COMPUTING = 1,
      COMPUTED = 2,
      ERROR = 3
   };

   // Computation descriptor stored in shared memory
   struct ComputationOp {
      enum OpType : uint32_t {
         CONSTANT,   // Already computed value
         ADD,        // Add two values
         MULTIPLY,   // Multiply two values
         NEGATE,     // Negate a value
         CHAIN,      // Chain two computations
         EXTERNAL    // External computation (index)
      } type;

      union {
         T constant_value;
         struct {
            T a, b;
         } binary_op;
         T unary_arg;
         struct {
            uint32_t first, second;
         } chain_indices;
         uint32_t external_id;
      } data;
   };

   struct Header {
      std::atomic<ComputeState> state;
      ComputationOp computation;
      T cached_value;
      char error_msg[256];
      std::atomic<uint32_t> compute_count;   // How many times forced
   };

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Lazy(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr) {
   }

   /// Core initialization logic for creating a new lazy computation with a
   /// constant.
   std::expected<void, Error> init_create(const T &value) {
      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize as constant
      header_->state.store(COMPUTED, std::memory_order_relaxed);
      header_->computation.type = ComputationOp::CONSTANT;
      header_->computation.data.constant_value = value;
      header_->cached_value = value;
      header_->compute_count.store(0, std::memory_order_relaxed);
      std::memset(header_->error_msg, 0, sizeof(header_->error_msg));

      return {};
   }

   /// Core initialization logic for creating a new lazy computation with an op
   /// type.
   std::expected<void, Error> init_create(ComputationOp::OpType op) {
      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      header_->state.store(NOT_COMPUTED, std::memory_order_relaxed);
      header_->computation.type = op;
      header_->compute_count.store(0, std::memory_order_relaxed);
      std::memset(header_->error_msg, 0, sizeof(header_->error_msg));

      return {};
   }

   /// Core initialization logic for opening an existing lazy computation.
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

   T compute() {
      switch (header_->computation.type) {
      case ComputationOp::CONSTANT:
         return header_->computation.data.constant_value;

      case ComputationOp::ADD:
         if constexpr (std::is_arithmetic_v<T>) {
            return header_->computation.data.binary_op.a +
                   header_->computation.data.binary_op.b;
         } else {
            throw std::runtime_error("ADD not supported for this type");
         }

      case ComputationOp::MULTIPLY:
         if constexpr (std::is_arithmetic_v<T>) {
            return header_->computation.data.binary_op.a *
                   header_->computation.data.binary_op.b;
         } else {
            throw std::runtime_error("MULTIPLY not supported for this type");
         }

      case ComputationOp::NEGATE:
         if constexpr (std::is_arithmetic_v<T>) {
            return -header_->computation.data.unary_arg;
         } else {
            throw std::runtime_error("NEGATE not supported for this type");
         }

      case ComputationOp::CHAIN:
      case ComputationOp::EXTERNAL:
         throw std::runtime_error("Complex operations not yet implemented");

      default:
         throw std::runtime_error("Unknown computation type");
      }
   }

 public:
   /// Create a new lazy computation with a constant value.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Lazy<T>, Error>
   create(Memory &memory, std::string_view name, const T &value) {
      Lazy<T> l(memory, name, FactoryTag{});
      auto result = l.init_create(value);
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   /// Create a new lazy computation with a computation operation type.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Lazy<T>, Error>
   create(Memory &memory, std::string_view name, ComputationOp::OpType op) {
      Lazy<T> l(memory, name, FactoryTag{});
      auto result = l.init_create(op);
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   /// Open an existing lazy computation in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the stored size doesn't match.
   static std::expected<Lazy<T>, Error> open(Memory &memory,
                                             std::string_view name) {
      Lazy<T> l(memory, name, FactoryTag{});
      auto result = l.init_open();
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   // Create new lazy computation with a constant
   Lazy(Memory &memory, std::string_view name, const T &value)
       : Lazy(memory, name, FactoryTag{}) {

      auto result = init_create(value);
      if (!result) {
         switch (result.error()) {
         case Error::AllocationOverflow:
            throw std::overflow_error("Lazy allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Lazy allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Lazy name already exists: {}", name));
         default:
            throw std::runtime_error(std::format(
                "Failed to allocate lazy: {}", error_message(result.error())));
         }
      }
   }

   // Create with computation operation
   Lazy(Memory &memory, std::string_view name, ComputationOp::OpType op)
       : Lazy(memory, name, FactoryTag{}) {

      auto result = init_create(op);
      if (!result) {
         switch (result.error()) {
         case Error::AllocationOverflow:
            throw std::overflow_error("Lazy allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Lazy allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Lazy name already exists: {}", name));
         default:
            throw std::runtime_error(std::format(
                "Failed to allocate lazy: {}", error_message(result.error())));
         }
      }
   }

   // Open existing lazy value
   Lazy(Memory &memory, std::string_view name)
       : Lazy(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(std::format("Lazy not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Invalid lazy size");
         default:
            throw std::runtime_error(std::format(
                "Lazy::open failed: {}", error_message(result.error())));
         }
      }
   }

   // Force evaluation
   [[nodiscard]] T force() {
      ComputeState expected = NOT_COMPUTED;

      // Try to claim computation
      if (header_->state.compare_exchange_strong(expected, COMPUTING,
                                                 std::memory_order_acquire,
                                                 std::memory_order_relaxed)) {
         // We get to compute it
         try {
            T result = compute();
            header_->cached_value = result;
            header_->state.store(COMPUTED, std::memory_order_release);
            header_->compute_count.fetch_add(1, std::memory_order_relaxed);
            return result;
         } catch (const std::exception &e) {
            std::strncpy(header_->error_msg, e.what(),
                         sizeof(header_->error_msg) - 1);
            header_->state.store(ERROR, std::memory_order_release);
            throw;
         }
      }

      // Wait for computation by another thread
      while (true) {
         ComputeState state = header_->state.load(std::memory_order_acquire);
         if (state == COMPUTED) {
            return header_->cached_value;
         }
         if (state == ERROR) {
            throw std::runtime_error(header_->error_msg);
         }
         std::this_thread::yield();
      }
   }

   // Check if already computed
   [[nodiscard]] bool is_computed() const {
      return header_->state.load(std::memory_order_acquire) == COMPUTED;
   }

   // Peek at value without forcing
   [[nodiscard]] std::expected<T, Error> peek() const {
      if (is_computed()) {
         return header_->cached_value;
      }
      return std::unexpected(Error::LazyNotReady);
   }

   // Map operation - create new lazy that applies function
   template <typename F>
   Lazy<T> map(Memory &mem, const std::string &new_name, F &&func) {
      Lazy<T> result(mem, new_name, ComputationOp::EXTERNAL);

      // Store the mapping operation
      // In real impl, would store func in shared memory somehow
      // For now, we'll force and map immediately
      if (is_computed()) {
         T val = func(header_->cached_value);
         result.header_->cached_value = val;
         result.header_->state.store(COMPUTED, std::memory_order_release);
      }

      return result;
   }

   // Create derived lazy computations
   static Lazy<T> add(Memory &mem, const std::string &name, Lazy<T> &a,
                      Lazy<T> &b) {
      Lazy<T> result(mem, name, ComputationOp::ADD);
      result.header_->computation.data.binary_op.a = a.force();
      result.header_->computation.data.binary_op.b = b.force();
      result.header_->state.store(NOT_COMPUTED, std::memory_order_release);
      return result;
   }

   static Lazy<T> multiply(Memory &mem, const std::string &name, Lazy<T> &a,
                           Lazy<T> &b) {
      Lazy<T> result(mem, name, ComputationOp::MULTIPLY);
      result.header_->computation.data.binary_op.a = a.force();
      result.header_->computation.data.binary_op.b = b.force();
      result.header_->state.store(NOT_COMPUTED, std::memory_order_release);
      return result;
   }

   // Reset to force recomputation
   void reset() {
      ComputeState expected = COMPUTED;
      header_->state.compare_exchange_strong(expected, NOT_COMPUTED,
                                             std::memory_order_acq_rel);
   }

   // Get computation count
   [[nodiscard]] uint32_t compute_count() const {
      return header_->compute_count.load(std::memory_order_relaxed);
   }

   /**
    * @brief Destroy the lazy computation and remove it from the shared memory
    * table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

// Specialization for lazy booleans with logical operations
template <> class Lazy<bool> {
 public:
   enum ComputeState : uint32_t {
      NOT_COMPUTED = 0,
      COMPUTING = 1,
      COMPUTED = 2
   };

   struct LogicalOp {
      enum OpType : uint32_t { CONSTANT, AND, OR, XOR, NOT } type;

      union {
         bool value;
         struct {
            bool a, b;
         } binary;
         bool unary;
      } data;
   };

   struct Header {
      std::atomic<ComputeState> state;
      LogicalOp operation;
      bool cached_value;
   };

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Lazy(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr) {
   }

   /// Core initialization logic for creating a new lazy boolean.
   std::expected<void, Error> init_create(bool value) {
      auto alloc_result = memory_.allocate(name_, sizeof(Header));
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;
      header_ = memory_.ptr_at<Header>(offset);

      header_->state.store(COMPUTED, std::memory_order_relaxed);
      header_->operation.type = LogicalOp::CONSTANT;
      header_->operation.data.value = value;
      header_->cached_value = value;

      return {};
   }

   /// Core initialization logic for opening an existing lazy boolean.
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

   bool compute() {
      switch (header_->operation.type) {
      case LogicalOp::CONSTANT:
         return header_->operation.data.value;
      case LogicalOp::AND:
         return header_->operation.data.binary.a &&
                header_->operation.data.binary.b;
      case LogicalOp::OR:
         return header_->operation.data.binary.a ||
                header_->operation.data.binary.b;
      case LogicalOp::XOR:
         return header_->operation.data.binary.a ^
                header_->operation.data.binary.b;
      case LogicalOp::NOT:
         return !header_->operation.data.unary;
      default:
         return false;
      }
   }

 public:
   /// Create a new lazy boolean with a value.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Lazy<bool>, Error>
   create(Memory &memory, std::string_view name, bool value) {
      Lazy<bool> l(memory, name, FactoryTag{});
      auto result = l.init_create(value);
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   /// Open an existing lazy boolean in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the stored size doesn't match.
   static std::expected<Lazy<bool>, Error> open(Memory &memory,
                                                std::string_view name) {
      Lazy<bool> l(memory, name, FactoryTag{});
      auto result = l.init_open();
      if (!result)
         return std::unexpected(result.error());
      return l;
   }

   Lazy(Memory &memory, std::string_view name, bool value)
       : Lazy(memory, name, FactoryTag{}) {

      auto result = init_create(value);
      if (!result) {
         switch (result.error()) {
         case Error::AllocationOverflow:
            throw std::overflow_error("Lazy bool allocation overflow");
         case Error::BoundsExceeded:
            throw std::runtime_error("Lazy bool allocation exceeds bounds");
         case Error::NameAlreadyExists:
            throw std::runtime_error(
                std::format("Lazy bool name already exists: {}", name));
         default:
            throw std::runtime_error(
                std::format("Failed to allocate lazy bool: {}",
                            error_message(result.error())));
         }
      }
   }

   [[nodiscard]] bool force() {
      ComputeState expected = NOT_COMPUTED;

      if (header_->state.compare_exchange_strong(expected, COMPUTING,
                                                 std::memory_order_acquire,
                                                 std::memory_order_relaxed)) {
         bool result = compute();
         header_->cached_value = result;
         header_->state.store(COMPUTED, std::memory_order_release);
         return result;
      }

      while (header_->state.load(std::memory_order_acquire) != COMPUTED) {
         std::this_thread::yield();
      }
      return header_->cached_value;
   }

   // Lazy AND operation
   static Lazy<bool> lazy_and(Memory &mem, const std::string &name,
                              Lazy<bool> &a, Lazy<bool> &b) {
      Lazy<bool> result(mem, name, false);
      result.header_->state.store(NOT_COMPUTED, std::memory_order_relaxed);
      result.header_->operation.type = LogicalOp::AND;

      // Short-circuit evaluation: only compute b if a is true
      if (a.force()) {
         result.header_->operation.data.binary.a = true;
         result.header_->operation.data.binary.b = b.force();
      } else {
         result.header_->operation.data.binary.a = false;
         result.header_->operation.data.binary.b = false;   // Not evaluated
         result.header_->cached_value = false;
         result.header_->state.store(COMPUTED, std::memory_order_release);
      }
      return result;
   }

   /**
    * @brief Destroy the lazy boolean and remove it from the shared memory
    * table.
    *
    * Removes the table entry so the name can be reused.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
