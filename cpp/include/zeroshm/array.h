#pragma once

#include <cstring>
#include <format>
#include <span>
#include <stdexcept>
#include <string_view>
#include <type_traits>

#include <zeroshm/layout.h>
#include <zeroshm/memory.h>

namespace zeroshm {

/**
 * Fixed-size array in shared memory.
 *
 * @tparam T Element type (must be trivially copyable)
 *
 * The array stores only its capacity in the header.
 * No type information is stored - users are responsible for type consistency.
 */
template <typename T> class Array {
   static_assert(std::is_trivially_copyable_v<T>,
                 "Array elements must be trivially copyable");
   static_assert(
       alignof(T) <= MAX_ELEM_ALIGN,
       "T alignment exceeds the 8-byte guarantee of shared memory layout");

 public:
   struct Header {
      uint64_t capacity;
   };

   /// Compile-time validation: Array::Header must be exactly 8 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == EXPECTED_ARRAY_HEADER_SIZE,
                 "Array::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, capacity) == 0,
                 "Array::Header.capacity must be at offset 0");

 private:
   Memory &memory_;
   Header *header_;
   T *data_;
   size_t capacity_;
   size_t offset_;
   std::string name_;

 public:
   /**
    * Create or open an array
    * @param memory Shared memory instance
    * @param name Name of the array
    * @param capacity Number of elements (0 to open existing)
    */
   Array(Memory &memory, std::string_view name, size_t capacity = 0)
       : memory_(memory) {

      if (name.size() >= MAX_NAME_SIZE) {
         throw std::invalid_argument(
             "Name too long (max 254 characters, 255 bytes with null)");
      }

      auto find_result = memory.table()->find(name);

      if (find_result) {
         auto *entry = *find_result;
         // Open existing array
         if (capacity != 0) {
            // Optionally validate capacity if provided
            Header *hdr = static_cast<Header *>(memory.at(entry->offset));
            if (hdr->capacity != capacity) {
               throw std::runtime_error(std::format(
                   "Capacity mismatch: array has {} but requested {}",
                   hdr->capacity, capacity));
            }
         }

         offset_ = entry->offset;
         header_ = static_cast<Header *>(memory.at(offset_));
         data_ = static_cast<T *>(memory.at(offset_ + sizeof(Header)));
         capacity_ = header_->capacity;
         name_ = name;
      } else {
         // Create new array
         if (capacity == 0) {
            throw std::invalid_argument(
                "Capacity required to create new array");
         }

         size_t total_size = sizeof(Header) + capacity * sizeof(T);
         offset_ = memory.table()->allocate(total_size);

         auto add_result = memory.table()->add(name, offset_, total_size);
         if (!add_result) {
            throw std::runtime_error(
                std::format("Failed to add array to table: {}",
                            error_message(add_result.error())));
         }

         // Initialize header
         header_ = static_cast<Header *>(memory.at(offset_));
         header_->capacity = capacity;

         // Get data pointer
         data_ = static_cast<T *>(memory.at(offset_ + sizeof(Header)));

         // Zero-initialize elements
         std::memset(data_, 0, capacity * sizeof(T));

         capacity_ = capacity;
         name_ = name;
      }
   }

   /**
    * Access element by index
    */
   T &operator[](size_t index) {
      return at(index);
   }

   const T &operator[](size_t index) const {
      return at(index);
   }

   /**
    * Access element with bounds checking
    */
   T &at(size_t index) {
      if (index >= capacity_) {
         throw std::out_of_range("Index out of bounds");
      }
      return data_[index];
   }

   const T &at(size_t index) const {
      if (index >= capacity_) {
         throw std::out_of_range("Index out of bounds");
      }
      return data_[index];
   }

   /**
    * Get a std::span view of the array data (C++20).
    * Enables zero-copy, bounds-aware access and std::ranges algorithms:
    *   std::ranges::for_each(arr, [](int& x) { x *= 2; });
    *   for (auto x : arr) { ... }  // works via begin()/end()
    */
   std::span<T> data() {
      return std::span<T>(data_, capacity_);
   }
   std::span<const T> data() const {
      return std::span<const T>(data_, capacity_);
   }

   /**
    * Get raw pointer to data (backward compatibility).
    */
   T *raw_data() {
      return data_;
   }
   const T *raw_data() const {
      return data_;
   }

   /**
    * Get array capacity
    */
   size_t capacity() const {
      return capacity_;
   }

   /**
    * Get array name
    */
   std::string_view name() const {
      return name_;
   }

   /**
    * Iterator support (enables range-based for loops)
    */
   auto begin() {
      return data().begin();
   }
   auto end() {
      return data().end();
   }
   auto begin() const {
      return data().begin();
   }
   auto end() const {
      return data().end();
   }

   /**
    * Fill array with value
    */
   void fill(const T &value) {
      for (size_t i = 0; i < capacity_; ++i) {
         data_[i] = value;
      }
   }
};

}   // namespace zeroshm
