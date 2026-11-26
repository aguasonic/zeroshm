#pragma once
// Compile-time layout validation for ZeroIPC binary format structures.
// Ensures C++ struct layouts match the specification and the Go/Rust
// implementations, catching binary format breakage at compile time.

#include <atomic>
#include <bit>
#include <cstddef>
#include <cstdint>
#include <pthread.h>
#include <type_traits>
#include <zeroshm/constants_generated.h>

namespace zeroshm {

// 
// Expected structure sizes (must match Go/Rust implementations and spec)
// 

/// Expected size of Table::Header (magic, version, entry_count, max_entries,
/// memory_size, next_offset) -- 4+4+4+4+8+8 = 32 bytes
constexpr std::size_t EXPECTED_TABLE_HEADER_SIZE = 32;

/// Expected size of Table::Entry (hash, offset, size, name[256], padding)
/// -- 8+8+8+256+104 = 384 bytes (matches Rust/Go ENTRY_SIZE)
constexpr std::size_t EXPECTED_TABLE_ENTRY_SIZE = 384;

/// Expected size of Queue<T>::Header (head, tail, capacity, elem_size)
/// -- std::atomic<uint32_t>2 + uint32_t2 = 4+4+4+4 = 16 bytes
constexpr std::size_t EXPECTED_QUEUE_HEADER_SIZE = 16;

/// Expected size of Ring<T>::Header (write_pos, read_pos, capacity, elem_size)
/// -- std::atomic<uint64_t>2 + uint32_t2 = 8+8+4+4 = 24 bytes
constexpr std::size_t EXPECTED_RING_HEADER_SIZE = 24;

/// Expected size of Array<T>::Header (capacity) -- uint64_t = 8 bytes
constexpr std::size_t EXPECTED_ARRAY_HEADER_SIZE = 8;

/// Expected size of Latest<T>::Header (elem_size, aligned_size)
/// -- uint32_t2 = 4+4 = 8 bytes
constexpr std::size_t EXPECTED_LATEST_HEADER_SIZE = 8;

// 
// Compile-time validation: sizes
// 

/// Verify that the cross-language constant matches the C++ struct size.
/// HEADER_SIZE and ENTRY_SIZE come from constants_generated.h (derived from
/// constants.pkl, the single source of truth shared with Go/Rust).
static_assert(HEADER_SIZE == EXPECTED_TABLE_HEADER_SIZE,
              "HEADER_SIZE constant must match expected Table::Header size");
static_assert(ENTRY_SIZE == EXPECTED_TABLE_ENTRY_SIZE,
              "ENTRY_SIZE constant must match expected Table::Entry size");

// 
// Compile-time validation: alignments
// 

/// All headers must be naturally aligned (no padding issues).
/// Queue/Ring headers contain atomics which require specific alignment.
static_assert(alignof(uint32_t) <= SECTION_ALIGN,
              "uint32_t alignment exceeds section alignment guarantee");
static_assert(alignof(uint64_t) <= SECTION_ALIGN,
              "uint64_t alignment exceeds section alignment guarantee");

// 
// consteval helpers: compute structure sizes at compile time
// 

/// Round n up to the next multiple of a (a must be a power of two).
/// Used by all structure size calculations.
constexpr std::size_t align_up_consteval(std::size_t n, std::size_t a) {
   return (n + a - 1) & ~(a - 1);
}

/// Total size of a Table with max_entries entries:
/// sizeof(Header) + max_entries * sizeof(Entry)
consteval std::size_t table_total_size(std::size_t max_entries) {
   return EXPECTED_TABLE_HEADER_SIZE + max_entries * EXPECTED_TABLE_ENTRY_SIZE;
}

/// Total size of a Queue<T> with the given capacity:
/// sizeof(Header) + align_up(capacity * sizeof(T), 8) + capacity *
/// sizeof(std::atomic<uint32_t>) The sequence array is 8-aligned so its atomics
/// are naturally aligned.
template <typename T>
consteval std::size_t queue_total_size(std::size_t capacity) {
   std::size_t seq_array_size = sizeof(std::atomic<uint32_t>) * capacity;
   std::size_t seq_off = align_up_consteval(sizeof(T) * capacity, SECTION_ALIGN);
   return EXPECTED_QUEUE_HEADER_SIZE + seq_off + seq_array_size;
}

/// Total size of a Ring<T> with the given capacity (in bytes):
/// sizeof(Header) + capacity
template <typename T>
consteval std::size_t ring_total_size(std::size_t capacity) {
   return EXPECTED_RING_HEADER_SIZE + capacity;
}

/// Total size of an Array<T> with the given capacity:
/// sizeof(Header) + capacity * sizeof(T)
template <typename T>
consteval std::size_t array_total_size(std::size_t capacity) {
   return EXPECTED_ARRAY_HEADER_SIZE + capacity * sizeof(T);
}

// 
// Latest<T> layout (unified binary format with pthread mutex + condvar)
// 

/// Binary layout of Latest[T]:
/// Offset 0:                        header (8 bytes: elem_size u32 +
/// aligned_size u32) Offset 8:                        value (elem_size bytes)
/// Offset 8 + elem_size:            padding (aligned_size - elem_size bytes,
/// zero-filled) Offset 8 + aligned_size:         version (u64, atomic) Offset 8
/// + aligned_size + 8:     mutex (pthread_mutex_t, aligned to
/// alignof(pthread_mutex_t)) Offset after mutex:              cond
/// (pthread_cond_t, aligned to alignof(pthread_cond_t))
///
/// total_size = 8 (header) + align_up(elem_size, 8) + 8 (version)
///              + align_up(sizeof(pthread_mutex_t), alignof(pthread_mutex_t))
///              + align_up(sizeof(pthread_cond_t), alignof(pthread_cond_t))
///
/// Note: pthread_mutex_t and pthread_cond_t sizes are platform-specific:
/// - Linux (glibc):   mutex=40 bytes, cond=48 bytes (both aligned to 8)
/// - macOS (libpthread): mutex=64 bytes, cond=48 bytes (both aligned to 8)
/// All three languages (C++, Go, Rust) must use the same platform's native
/// sizeof/alignof values to maintain cross-language compatibility.

/// Maximum alignment for the version field (must be 8 for atomic u64).
constexpr std::size_t LATEST_VERSION_ALIGN = SECTION_ALIGN;

/// Size of the Latest header in bytes (elem_size u32 + aligned_size u32).
constexpr std::size_t LATEST_HEADER_SIZE = EXPECTED_LATEST_HEADER_SIZE;

/// Size of pthread_mutex_t on the current platform (platform-specific).
constexpr std::size_t PTHREAD_MUTEX_SIZE = sizeof(pthread_mutex_t);

/// Alignment of pthread_mutex_t on the current platform (platform-specific).
constexpr std::size_t PTHREAD_MUTEX_ALIGN = alignof(pthread_mutex_t);

/// Size of pthread_cond_t on the current platform (platform-specific).
constexpr std::size_t PTHREAD_COND_SIZE = sizeof(pthread_cond_t);

/// Alignment of pthread_cond_t on the current platform (platform-specific).
constexpr std::size_t PTHREAD_COND_ALIGN = alignof(pthread_cond_t);

/// Compile-time validation: pthread structures must be naturally aligned.
static_assert(PTHREAD_MUTEX_ALIGN <= SECTION_ALIGN,
              "pthread_mutex_t alignment exceeds section alignment guarantee");
static_assert(PTHREAD_COND_ALIGN <= SECTION_ALIGN,
              "pthread_cond_t alignment exceeds section alignment guarantee");

/// Compile-time validation: version field must be 8-aligned for atomic u64.
static_assert(LATEST_VERSION_ALIGN == SECTION_ALIGN,
              "version field must be section-aligned for atomic u64");

/// Total size of a Latest[T] structure in shared memory:
/// header(8) + align_up(elem_size, 8) + version(8) + aligned mutex + aligned
/// cond
constexpr std::size_t latest_total_size(std::size_t elem_size) {
   std::size_t aligned_size =
       align_up_consteval(elem_size, LATEST_VERSION_ALIGN);
   std::size_t version_end =
       LATEST_HEADER_SIZE + aligned_size + sizeof(uint64_t);
   std::size_t mutex_start =
       align_up_consteval(version_end, PTHREAD_MUTEX_ALIGN);
   std::size_t cond_start =
       align_up_consteval(mutex_start + PTHREAD_MUTEX_SIZE, PTHREAD_COND_ALIGN);
   return cond_start + PTHREAD_COND_SIZE;
}

/// Offset of the version field from the base offset of the Latest structure.
constexpr std::size_t latest_version_offset(std::size_t base_offset,
                                            std::size_t elem_size) {
   std::size_t aligned_size =
       align_up_consteval(elem_size, LATEST_VERSION_ALIGN);
   return base_offset + LATEST_HEADER_SIZE + aligned_size;
}

/// Offset of the pthread mutex from the base offset of the Latest structure.
constexpr std::size_t latest_mutex_offset(std::size_t base_offset,
                                          std::size_t elem_size) {
   std::size_t aligned_size =
       align_up_consteval(elem_size, LATEST_VERSION_ALIGN);
   std::size_t version_end =
       base_offset + LATEST_HEADER_SIZE + aligned_size + sizeof(uint64_t);
   return align_up_consteval(version_end, PTHREAD_MUTEX_ALIGN);
}

/// Offset of the pthread condvar from the base offset of the Latest structure.
constexpr std::size_t latest_cond_offset(std::size_t base_offset,
                                         std::size_t elem_size) {
   std::size_t mutex_off = latest_mutex_offset(base_offset, elem_size);
   return align_up_consteval(mutex_off + PTHREAD_MUTEX_SIZE,
                             PTHREAD_COND_ALIGN);
}

/// Compile-time validation: version field must be 8-aligned for atomic u64.
static_assert(latest_version_offset(0, 4) % SECTION_ALIGN == 0,
              "version offset must be section-aligned (elem_size=4)");
static_assert(latest_version_offset(0, 8) % SECTION_ALIGN == 0,
              "version offset must be section-aligned (elem_size=8)");
static_assert(latest_version_offset(0, 16) % SECTION_ALIGN == 0,
              "version offset must be section-aligned (elem_size=16)");

}   // namespace zeroshm
