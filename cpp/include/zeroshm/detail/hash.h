#pragma once

#include <array>
#include <bit>
#include <cstring>
#include <functional>
#include <string_view>
#include <type_traits>

namespace zeroshm::detail {

/*
/// Hash function for trivially copyable types in shared memory.
/// Uses multiplicative hash for integers, byte-level hash otherwise.
/// C++20 std::bit_cast provides a well-defined, type-safe way to obtain
/// the object representation of a trivially copyable value as bytes.
*/
template <typename T> size_t trivial_hash(const T &val) {
   if constexpr (std::is_integral_v<T>) {
      return static_cast<size_t>(val) * 2654435761U;
   } else {
      std::hash<std::string_view> hasher;
      // std::bit_cast safely converts the value to its byte representation
      // (well-defined for trivially copyable types, unlike reinterpret_cast
      // of object addresses which relies on contiguous object representation)
      auto bytes = std::bit_cast<std::array<uint8_t, sizeof(T)>>(val);
      std::string_view sv(reinterpret_cast<const char *>(bytes.data()),
                          bytes.size());
      return hasher(sv);
   }
}

/*
/// Equality comparison for trivially copyable types in shared memory.
/// Uses == for arithmetic types, memcmp for structs.
*/
template <typename T> bool trivial_equal(const T &a, const T &b) {
   if constexpr (std::is_integral_v<T> || std::is_floating_point_v<T>) {
      return a == b;
   } else {
      return std::memcmp(&a, &b, sizeof(T)) == 0;
   }
}

/*
/// Convert a trivially copyable value to its byte representation using
std::bit_cast.
/// This is a well-defined, type-safe alternative to reinterpret_cast for
value-to-bytes
/// conversion. Requires T to be trivially copyable and non-empty.
/// @tparam T The trivially copyable type to convert
/// @param val The value to convert
/// @return std::array<uint8_t, sizeof(T)> containing the object representation
of val
*/
template <typename T>
   requires std::is_trivially_copyable_v<T> && (sizeof(T) > 0)

std::array<uint8_t, sizeof(T)> to_bytes(T val) {
   return std::bit_cast<std::array<uint8_t, sizeof(T)>>(val);
}

}   // namespace zeroshm::detail
