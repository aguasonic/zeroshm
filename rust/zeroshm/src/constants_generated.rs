// Auto-generated from constants.pkl - DO NOT EDIT
// Run 'make generate' to regenerate.

/// Barrier header: arrived field offset
pub const BARRIER_ARRIVED_OFFSET: usize = 0;

/// Barrier header: generation field offset
pub const BARRIER_GENERATION_OFFSET: usize = 4;

/// Barrier header: num participants field offset
pub const BARRIER_NUM_PARTICIPANTS_OFFSET: usize = 8;

/// Barrier header: padding field offset
pub const BARRIER_PADDING_OFFSET: usize = 12;

/// Channel header: capacity field offset
pub const CHANNEL_CAPACITY_OFFSET: usize = 0;

/// Channel header: closed field offset
pub const CHANNEL_CLOSED_OFFSET: usize = 12;

/// Channel header: receivers field offset
pub const CHANNEL_RECEIVERS_OFFSET: usize = 8;

/// Channel header: receive sequence field offset
pub const CHANNEL_RECV_SEQ_OFFSET: usize = 24;

/// Channel header: send sequence field offset
pub const CHANNEL_SEND_SEQ_OFFSET: usize = 16;

/// Channel header: senders field offset
pub const CHANNEL_SENDERS_OFFSET: usize = 4;

/// Size of each table entry in bytes
pub const ENTRY_SIZE: usize = 384;

/// Event state: mode field offset
pub const EVENT_MODE_OFFSET: usize = 4;

/// Event state: signaled field offset
pub const EVENT_SIGNALED_OFFSET: usize = 0;

/// Event state: waiting field offset
pub const EVENT_WAITING_OFFSET: usize = 8;

/// Future header: completion time field offset
pub const FUTURE_COMPLETION_TIME_OFFSET: usize = 8;

/// Future header: state field offset
pub const FUTURE_STATE_OFFSET: usize = 0;

/// Future header: waiters field offset
pub const FUTURE_WAITERS_OFFSET: usize = 4;

/// Size of the table header in bytes
pub const HEADER_SIZE: usize = 32;

/// Latch header: count field offset
pub const LATCH_COUNT_OFFSET: usize = 0;

/// Latch header: initial count field offset
pub const LATCH_INITIAL_COUNT_OFFSET: usize = 4;

/// Latch header: padding 0 field offset
pub const LATCH_PADDING0_OFFSET: usize = 8;

/// Latch header: padding 1 field offset
pub const LATCH_PADDING1_OFFSET: usize = 12;

/// Lazy header: completion time field offset
pub const LAZY_COMPLETION_TIME_OFFSET: usize = 8;

/// Lazy header: compute count field offset
pub const LAZY_COMPUTE_COUNT_OFFSET: usize = 4;

/// Lazy header: state field offset
pub const LAZY_STATE_OFFSET: usize = 0;

/// Magic number identifying a ZeroSHM table header ('ZSHM')
pub const TABLE_MAGIC: u32 = 0x5A53484D;

/// Map header: capacity field offset
pub const MAP_CAPACITY_OFFSET: usize = 4;

/// Map header: key size field offset
pub const MAP_KEY_SIZE_OFFSET: usize = 8;

/// Map header: size field offset
pub const MAP_SIZE_OFFSET: usize = 0;

/// Map header: value size field offset
pub const MAP_VALUE_SIZE_OFFSET: usize = 12;

/// Name field size in table entries (includes null terminator + padding)
pub const NAME_FIELD_SIZE: usize = 256;

/// Once flag: state field offset
pub const ONCE_STATE_OFFSET: usize = 0;

/// Pool header: allocated field offset
pub const POOL_ALLOCATED_OFFSET: usize = 8;

/// Pool header: capacity field offset
pub const POOL_CAPACITY_OFFSET: usize = 16;

/// Pool header: element size field offset
pub const POOL_ELEM_SIZE_OFFSET: usize = 20;

/// Pool header: free head field offset
pub const POOL_FREE_HEAD_OFFSET: usize = 0;

/// Pool header: padding field offset
pub const POOL_PADDING_OFFSET: usize = 12;

/// Queue header: capacity field offset
pub const QUEUE_CAPACITY_OFFSET: usize = 8;

/// Queue header: element size field offset
pub const QUEUE_ELEM_SIZE_OFFSET: usize = 12;

/// Queue header: head field offset
pub const QUEUE_HEAD_OFFSET: usize = 0;

/// Queue header: tail field offset
pub const QUEUE_TAIL_OFFSET: usize = 4;

/// Ring header: capacity field offset
pub const RING_CAPACITY_OFFSET: usize = 16;

/// Ring header: element size field offset
pub const RING_ELEM_SIZE_OFFSET: usize = 20;

/// Ring header: read position field offset
pub const RING_READ_POS_OFFSET: usize = 8;

/// Ring header: write position field offset
pub const RING_WRITE_POS_OFFSET: usize = 0;

/// RWLock state: readers field offset
pub const RWLOCK_READERS_OFFSET: usize = 0;

/// RWLock state: writer active field offset
pub const RWLOCK_WRITER_ACTIVE_OFFSET: usize = 4;

/// Section alignment (8-byte boundaries for all structure sections)
pub const SECTION_ALIGN: usize = 8;

/// Semaphore header: count field offset
pub const SEMAPHORE_COUNT_OFFSET: usize = 0;

/// Semaphore header: max count field offset
pub const SEMAPHORE_MAX_COUNT_OFFSET: usize = 8;

/// Semaphore header: padding field offset
pub const SEMAPHORE_PADDING_OFFSET: usize = 12;

/// Semaphore header: waiting field offset
pub const SEMAPHORE_WAITING_OFFSET: usize = 4;

/// Set header: capacity field offset
pub const SET_CAPACITY_OFFSET: usize = 4;

/// Set header: element size field offset
pub const SET_ELEM_SIZE_OFFSET: usize = 8;

/// Set header: reserved field offset
pub const SET_RESERVED_OFFSET: usize = 12;

/// Set header: size field offset
pub const SET_SIZE_OFFSET: usize = 0;

/// Stack header: capacity field offset
pub const STACK_CAPACITY_OFFSET: usize = 4;

/// Stack header: element size field offset
pub const STACK_ELEM_SIZE_OFFSET: usize = 8;

/// Stack header: reserved field offset
pub const STACK_RESERVED_OFFSET: usize = 12;

/// Stack header: top field offset
pub const STACK_TOP_OFFSET: usize = 0;

/// Stream header: buffer capacity field offset
pub const STREAM_BUFFER_CAPACITY_OFFSET: usize = 16;

/// Stream header: closed field offset
pub const STREAM_CLOSED_OFFSET: usize = 12;

/// Stream header: padding field offset
pub const STREAM_PADDING_OFFSET: usize = 20;

/// Stream header: sequence field offset
pub const STREAM_SEQUENCE_OFFSET: usize = 0;

/// Stream header: subscribers field offset
pub const STREAM_SUBSCRIBERS_OFFSET: usize = 8;

/// Stream header: transform name field offset
pub const STREAM_TRANSFORM_NAME_OFFSET: usize = 24;

/// Table header: entry count field offset
pub const TABLE_ENTRY_COUNT_OFFSET: usize = 8;

/// Table header: magic field offset
pub const TABLE_MAGIC_OFFSET: usize = 0;

/// Table header: max entries field offset
pub const TABLE_MAX_ENTRIES_OFFSET: usize = 12;

/// Table header: memory size field offset
pub const TABLE_MEMORY_SIZE_OFFSET: usize = 16;

/// Table header: next offset field
pub const TABLE_NEXT_OFFSET_OFFSET: usize = 24;

/// Table header: version field offset
pub const TABLE_VERSION_OFFSET: usize = 4;

/// Size of u32 primitive
pub const U32_SIZE: usize = 4;

/// Size of u64 primitive
pub const U64_SIZE: usize = 8;

/// Current table format version
pub const TABLE_VERSION: u32 = 3;

/// Seed value for xxhash3-64 name hashing
pub const XXHASH_SEED: u64 = 0x2FE7;

