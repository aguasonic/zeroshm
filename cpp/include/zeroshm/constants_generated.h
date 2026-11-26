#pragma once
// Auto-generated from constants.pkl - DO NOT EDIT
// Run 'make generate' to regenerate.

#include <cstdint>
#include <cstddef>

namespace zeroshm {

/// Barrier header: arrived field offset
constexpr std::size_t BARRIER_ARRIVED_OFFSET = 0;
/// Barrier header: generation field offset
constexpr std::size_t BARRIER_GENERATION_OFFSET = 4;
/// Barrier header: num participants field offset
constexpr std::size_t BARRIER_NUM_PARTICIPANTS_OFFSET = 8;
/// Barrier header: padding field offset
constexpr std::size_t BARRIER_PADDING_OFFSET = 12;

/// Channel header: capacity field offset
constexpr std::size_t CHANNEL_CAPACITY_OFFSET = 0;
/// Channel header: closed field offset
constexpr std::size_t CHANNEL_CLOSED_OFFSET = 12;
/// Channel header: receivers field offset
constexpr std::size_t CHANNEL_RECEIVERS_OFFSET = 8;
/// Channel header: receive sequence field offset
constexpr std::size_t CHANNEL_RECV_SEQ_OFFSET = 24;
/// Channel header: send sequence field offset
constexpr std::size_t CHANNEL_SEND_SEQ_OFFSET = 16;
/// Channel header: senders field offset
constexpr std::size_t CHANNEL_SENDERS_OFFSET = 4;

/// Size of each table entry in bytes
constexpr std::size_t ENTRY_SIZE = 384;

/// Event state: mode field offset
constexpr std::size_t EVENT_MODE_OFFSET = 4;
/// Event state: signaled field offset
constexpr std::size_t EVENT_SIGNALED_OFFSET = 0;
/// Event state: waiting field offset
constexpr std::size_t EVENT_WAITING_OFFSET = 8;

/// Future header: completion time field offset
constexpr std::size_t FUTURE_COMPLETION_TIME_OFFSET = 8;
/// Future header: state field offset
constexpr std::size_t FUTURE_STATE_OFFSET = 0;
/// Future header: waiters field offset
constexpr std::size_t FUTURE_WAITERS_OFFSET = 4;

/// Size of the table header in bytes
constexpr std::size_t HEADER_SIZE = 32;

/// Latch header: count field offset
constexpr std::size_t LATCH_COUNT_OFFSET = 0;
/// Latch header: initial count field offset
constexpr std::size_t LATCH_INITIAL_COUNT_OFFSET = 4;
/// Latch header: padding 0 field offset
constexpr std::size_t LATCH_PADDING0_OFFSET = 8;
/// Latch header: padding 1 field offset
constexpr std::size_t LATCH_PADDING1_OFFSET = 12;

/// Lazy header: completion time field offset
constexpr std::size_t LAZY_COMPLETION_TIME_OFFSET = 8;
/// Lazy header: compute count field offset
constexpr std::size_t LAZY_COMPUTE_COUNT_OFFSET = 4;
/// Lazy header: state field offset
constexpr std::size_t LAZY_STATE_OFFSET = 0;

/// Magic number identifying a ZeroSHM table header ('ZSHM')
constexpr uint32_t TABLE_MAGIC = 0x5A53484D;

/// Map header: capacity field offset
constexpr std::size_t MAP_CAPACITY_OFFSET = 4;
/// Map header: key size field offset
constexpr std::size_t MAP_KEY_SIZE_OFFSET = 8;
/// Map header: size field offset
constexpr std::size_t MAP_SIZE_OFFSET = 0;
/// Map header: value size field offset
constexpr std::size_t MAP_VALUE_SIZE_OFFSET = 12;

/// Name field size in table entries (includes null terminator + padding)
constexpr std::size_t NAME_FIELD_SIZE = 256;

/// Once flag: state field offset
constexpr std::size_t ONCE_STATE_OFFSET = 0;

/// Pool header: allocated field offset
constexpr std::size_t POOL_ALLOCATED_OFFSET = 8;
/// Pool header: capacity field offset
constexpr std::size_t POOL_CAPACITY_OFFSET = 16;
/// Pool header: element size field offset
constexpr std::size_t POOL_ELEM_SIZE_OFFSET = 20;
/// Pool header: free head field offset
constexpr std::size_t POOL_FREE_HEAD_OFFSET = 0;
/// Pool header: padding field offset
constexpr std::size_t POOL_PADDING_OFFSET = 12;

/// Queue header: capacity field offset
constexpr std::size_t QUEUE_CAPACITY_OFFSET = 8;
/// Queue header: element size field offset
constexpr std::size_t QUEUE_ELEM_SIZE_OFFSET = 12;
/// Queue header: head field offset
constexpr std::size_t QUEUE_HEAD_OFFSET = 0;
/// Queue header: tail field offset
constexpr std::size_t QUEUE_TAIL_OFFSET = 4;

/// Ring header: capacity field offset
constexpr std::size_t RING_CAPACITY_OFFSET = 16;
/// Ring header: element size field offset
constexpr std::size_t RING_ELEM_SIZE_OFFSET = 20;
/// Ring header: read position field offset
constexpr std::size_t RING_READ_POS_OFFSET = 8;
/// Ring header: write position field offset
constexpr std::size_t RING_WRITE_POS_OFFSET = 0;

/// RWLock state: readers field offset
constexpr std::size_t RWLOCK_READERS_OFFSET = 0;
/// RWLock state: writer active field offset
constexpr std::size_t RWLOCK_WRITER_ACTIVE_OFFSET = 4;

/// Section alignment (8-byte boundaries for all structure sections)
constexpr std::size_t SECTION_ALIGN = 8;

/// Semaphore header: count field offset
constexpr std::size_t SEMAPHORE_COUNT_OFFSET = 0;
/// Semaphore header: max count field offset
constexpr std::size_t SEMAPHORE_MAX_COUNT_OFFSET = 8;
/// Semaphore header: padding field offset
constexpr std::size_t SEMAPHORE_PADDING_OFFSET = 12;
/// Semaphore header: waiting field offset
constexpr std::size_t SEMAPHORE_WAITING_OFFSET = 4;

/// Set header: capacity field offset
constexpr std::size_t SET_CAPACITY_OFFSET = 4;
/// Set header: element size field offset
constexpr std::size_t SET_ELEM_SIZE_OFFSET = 8;
/// Set header: reserved field offset
constexpr std::size_t SET_RESERVED_OFFSET = 12;
/// Set header: size field offset
constexpr std::size_t SET_SIZE_OFFSET = 0;

/// Stack header: capacity field offset
constexpr std::size_t STACK_CAPACITY_OFFSET = 4;
/// Stack header: element size field offset
constexpr std::size_t STACK_ELEM_SIZE_OFFSET = 8;
/// Stack header: reserved field offset
constexpr std::size_t STACK_RESERVED_OFFSET = 12;
/// Stack header: top field offset
constexpr std::size_t STACK_TOP_OFFSET = 0;

/// Stream header: buffer capacity field offset
constexpr std::size_t STREAM_BUFFER_CAPACITY_OFFSET = 16;
/// Stream header: closed field offset
constexpr std::size_t STREAM_CLOSED_OFFSET = 12;
/// Stream header: padding field offset
constexpr std::size_t STREAM_PADDING_OFFSET = 20;
/// Stream header: sequence field offset
constexpr std::size_t STREAM_SEQUENCE_OFFSET = 0;
/// Stream header: subscribers field offset
constexpr std::size_t STREAM_SUBSCRIBERS_OFFSET = 8;
/// Stream header: transform name field offset
constexpr std::size_t STREAM_TRANSFORM_NAME_OFFSET = 24;

/// Table header: entry count field offset
constexpr std::size_t TABLE_ENTRY_COUNT_OFFSET = 8;
/// Table header: magic field offset
constexpr std::size_t TABLE_MAGIC_OFFSET = 0;
/// Table header: max entries field offset
constexpr std::size_t TABLE_MAX_ENTRIES_OFFSET = 12;
/// Table header: memory size field offset
constexpr std::size_t TABLE_MEMORY_SIZE_OFFSET = 16;
/// Table header: next offset field
constexpr std::size_t TABLE_NEXT_OFFSET_OFFSET = 24;
/// Table header: version field offset
constexpr std::size_t TABLE_VERSION_OFFSET = 4;

/// Size of u32 primitive
constexpr std::size_t U32_SIZE = 4;
/// Size of u64 primitive
constexpr std::size_t U64_SIZE = 8;

/// Current table format version
constexpr uint32_t TABLE_VERSION = 3;
/// Seed value for xxhash3-64 name hashing
constexpr uint64_t XXHASH_SEED = 0x2FE7;

} // namespace zeroshm
