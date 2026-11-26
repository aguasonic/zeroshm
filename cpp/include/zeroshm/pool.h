#pragma once

#include <atomic>
#include <cstddef>
#include <expected>
#include <format>
#include <span>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"

namespace zeroshm {

template <typename T> class Pool {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");

   struct Node {
      T data;
      std::atomic<uint32_t> next;   // Index of next free node
   };

   struct Header {
      std::atomic<uint64_t>
          free_head;   // Tagged pointer: [generation:32][index:32]
      std::atomic<uint32_t> allocated;   // Number of allocated items
      uint32_t padding_;   // Alignment padding (was part of old free_head slot)
      uint32_t capacity;
      uint32_t elem_size;
   };

   /// Compile-time validation: Pool::Header must be exactly 24 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 24,
                 "Pool::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, free_head) == POOL_FREE_HEAD_OFFSET,
                 "Pool::Header.free_head must be at POOL_FREE_HEAD_OFFSET");
   static_assert(offsetof(Header, allocated) == POOL_ALLOCATED_OFFSET,
                 "Pool::Header.allocated must be at POOL_ALLOCATED_OFFSET");
   static_assert(offsetof(Header, padding_) == POOL_PADDING_OFFSET,
                 "Pool::Header.padding_ must be at POOL_PADDING_OFFSET");
   static_assert(offsetof(Header, capacity) == POOL_CAPACITY_OFFSET,
                 "Pool::Header.capacity must be at POOL_CAPACITY_OFFSET");
   static_assert(offsetof(Header, elem_size) == POOL_ELEM_SIZE_OFFSET,
                 "Pool::Header.elem_size must be at POOL_ELEM_SIZE_OFFSET");

   static constexpr uint32_t NULL_INDEX = 0xFFFFFFFF;

   // Tagged pointer helpers to prevent ABA problem
   static uint64_t pack_tagged(uint32_t index, uint32_t generation) {
      return (static_cast<uint64_t>(generation) << 32) | index;
   }

   static uint32_t unpack_index(uint64_t tagged) {
      return static_cast<uint32_t>(tagged & 0xFFFFFFFF);
   }

   static uint32_t unpack_generation(uint64_t tagged) {
      return static_cast<uint32_t>(tagged >> 32);
   }

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Pool(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), nodes_(nullptr) {
   }

   /// Core initialization logic for creating a new pool.
   std::expected<void, Error> init_create(size_t capacity) {
      if (capacity == 0) {
         return std::unexpected(Error::InvalidCapacity);
      }

      // Check for overflow
      if (capacity > (SIZE_MAX - sizeof(Header)) / sizeof(Node)) {
         return std::unexpected(Error::AllocationOverflow);
      }

      size_t total_size = sizeof(Header) + sizeof(Node) * capacity;
      auto alloc_result = memory_.allocate(name_, total_size);
      if (!alloc_result) {
         return std::unexpected(alloc_result.error());
      }
      size_t offset = *alloc_result;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header with tagged pointer (generation=0, index=0)
      header_->free_head.store(pack_tagged(0, 0), std::memory_order_relaxed);
      header_->allocated.store(0, std::memory_order_relaxed);
      header_->padding_ = 0;
      header_->capacity = capacity;
      header_->elem_size = sizeof(T);

      nodes_ = reinterpret_cast<Node *>(reinterpret_cast<char *>(header_) +
                                        sizeof(Header));

      // Initialize free list - all nodes are free
      for (uint32_t i = 0; i < capacity - 1; ++i) {
         nodes_[i].next.store(i + 1, std::memory_order_relaxed);
      }
      nodes_[capacity - 1].next.store(NULL_INDEX, std::memory_order_relaxed);

      return {};
   }

   /// Core initialization logic for opening an existing pool.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      if (!memory_.find(name_, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      if (header_->elem_size != sizeof(T)) {
         return std::unexpected(Error::TypeMismatch);
      }

      nodes_ = reinterpret_cast<Node *>(reinterpret_cast<char *>(header_) +
                                        sizeof(Header));

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_ = nullptr;
   Node *nodes_ = nullptr;

 public:
   /// Create a new pool in shared memory.
   /// Returns Error::InvalidCapacity if capacity is 0,
   /// Error::AllocationOverflow or Error::BoundsExceeded if allocation fails,
   /// Error::NameAlreadyExists if the name is already in use.
   static std::expected<Pool, Error>
   create(Memory &memory, std::string_view name, size_t capacity) {
      Pool p(memory, name, FactoryTag{});
      auto result = p.init_create(capacity);
      if (!result)
         return std::unexpected(result.error());
      return p;
   }

   /// Open an existing pool in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match.
   static std::expected<Pool, Error> open(Memory &memory,
                                          std::string_view name) {
      Pool p(memory, name, FactoryTag{});
      auto result = p.init_open();
      if (!result)
         return std::unexpected(result.error());
      return p;
   }

   // Create new pool
   Pool(Memory &memory, std::string_view name, size_t capacity)
       : Pool(memory, name, FactoryTag{}) {

      auto result = init_create(capacity);
      if (!result) {
         throw std::runtime_error(std::format("Pool::create failed: {}",
                                              error_message(result.error())));
      }
   }

   // Open existing pool
   Pool(Memory &memory, std::string_view name)
       : Pool(memory, name, FactoryTag{}) {

      auto result = init_open();
      if (!result) {
         throw std::runtime_error(std::format("Pool::open failed: {}",
                                              error_message(result.error())));
      }
   }

   // Allocate an object from the pool (lock-free, ABA-safe)
   [[nodiscard]] std::expected<T *, Error> allocate() {
      uint64_t old_head;
      uint64_t new_head;

      // Try to get a free node using tagged pointer CAS
      do {
         old_head = header_->free_head.load(std::memory_order_acquire);
         uint32_t free_index = unpack_index(old_head);
         uint32_t generation = unpack_generation(old_head);

         if (free_index == NULL_INDEX) {
            return std::unexpected(Error::PoolExhausted);   // Pool is full
         }

         uint32_t next =
             nodes_[free_index].next.load(std::memory_order_relaxed);

         // Pack new head with bumped generation to prevent ABA
         new_head = pack_tagged(next, generation + 1);

         // Try to update the free head (tagged CAS prevents ABA)
         if (header_->free_head.compare_exchange_weak(
                 old_head, new_head, std::memory_order_release,
                 std::memory_order_acquire)) {
            // Success - we got the node
            header_->allocated.fetch_add(1, std::memory_order_relaxed);
            return &nodes_[free_index].data;
         }
      } while (true);
   }

   // Deallocate an object back to the pool (lock-free, ABA-safe)
   [[nodiscard]] std::expected<void, Error> deallocate(T *ptr) {
      if (!ptr)
         return std::expected<void, Error>{};

      // Calculate the node index from the pointer
      Node *node = reinterpret_cast<Node *>(reinterpret_cast<char *>(ptr) -
                                            offsetof(Node, data));
      uint32_t node_index = node - nodes_;

      // Validate the index
      if (node_index >= header_->capacity) {
         return std::unexpected(Error::PoolInvalidPointer);
      }

      // Add the node back to the free list using tagged CAS
      uint64_t old_head;
      uint64_t new_head;
      do {
         old_head = header_->free_head.load(std::memory_order_acquire);
         uint32_t old_index = unpack_index(old_head);
         uint32_t generation = unpack_generation(old_head);

         node->next.store(old_index, std::memory_order_relaxed);

         // Pack new head with bumped generation to prevent ABA
         new_head = pack_tagged(node_index, generation + 1);
      } while (!header_->free_head.compare_exchange_weak(
          old_head, new_head, std::memory_order_release,
          std::memory_order_acquire));

      header_->allocated.fetch_sub(1, std::memory_order_relaxed);
      return std::expected<void, Error>{};
   }

   // Construct an object in the pool
   template <typename... Args>
   [[nodiscard]] std::expected<T *, Error> construct(Args &&...args) {
      auto ptr = allocate();
      if (ptr) {
         new (*ptr) T(std::forward<Args>(args)...);
      }
      return ptr;
   }

   // Destroy an object and return it to the pool
   [[nodiscard]] std::expected<void, Error> destroy(T *ptr) {
      if (ptr) {
         ptr->~T();
         return deallocate(ptr);
      }
      return std::expected<void, Error>{};
   }

   // Get number of allocated objects
   [[nodiscard]] size_t allocated() const {
      return header_->allocated.load(std::memory_order_relaxed);
   }

   // Get number of free objects
   [[nodiscard]] size_t available() const {
      return header_->capacity - allocated();
   }

   // Get pool capacity
   [[nodiscard]] size_t capacity() const {
      return header_->capacity;
   }

   // Check if pool is empty (all objects are free)
   [[nodiscard]] bool empty() const {
      return allocated() == 0;
   }

   // Check if pool is full (no free objects)
   [[nodiscard]] bool full() const {
      return allocated() == header_->capacity;
   }

   /// Zero-copy view of the slot array (entire capacity).
   /// Each Node contains {data, next}.
   [[nodiscard]] std::span<const Node> nodes() const {
      return std::span<const Node>(nodes_, header_->capacity);
   }

   /**
    * @brief Destroy the pool and remove it from the shared memory table.
    *
    * Removes the table entry so the name can be reused.
    * Note: this does NOT call destructors on allocated objects.
    */
   void destroy() {
      (void)memory_.deallocate(name_);
   }
};

}   // namespace zeroshm
