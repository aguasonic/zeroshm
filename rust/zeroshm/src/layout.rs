//! Layout constants and alignment utilities matching the ZeroSHM v3.0 specification.
//! Core constants (magic, version, seed, sizes) are defined in constants_generated.rs
//! (auto-generated from constants.pkl).

pub use crate::constants_generated::{
    TABLE_MAGIC, TABLE_VERSION, XXHASH_SEED, HEADER_SIZE, ENTRY_SIZE,
    SECTION_ALIGN, NAME_FIELD_SIZE,
    // Table header field offsets
    TABLE_MAGIC_OFFSET, TABLE_VERSION_OFFSET, TABLE_ENTRY_COUNT_OFFSET,
    TABLE_MAX_ENTRIES_OFFSET, TABLE_MEMORY_SIZE_OFFSET, TABLE_NEXT_OFFSET_OFFSET,
    // Queue header field offsets
    QUEUE_HEAD_OFFSET, QUEUE_TAIL_OFFSET, QUEUE_CAPACITY_OFFSET, QUEUE_ELEM_SIZE_OFFSET,
    // Ring header field offsets
    RING_WRITE_POS_OFFSET, RING_READ_POS_OFFSET, RING_CAPACITY_OFFSET, RING_ELEM_SIZE_OFFSET,
    // Stack header field offsets
    STACK_TOP_OFFSET, STACK_CAPACITY_OFFSET, STACK_ELEM_SIZE_OFFSET, STACK_RESERVED_OFFSET,
    // Map header field offsets
    MAP_SIZE_OFFSET, MAP_CAPACITY_OFFSET, MAP_KEY_SIZE_OFFSET, MAP_VALUE_SIZE_OFFSET,
    // Set header field offsets
    SET_SIZE_OFFSET, SET_CAPACITY_OFFSET, SET_ELEM_SIZE_OFFSET, SET_RESERVED_OFFSET,
    // Pool header field offsets
    POOL_FREE_HEAD_OFFSET, POOL_ALLOCATED_OFFSET, POOL_PADDING_OFFSET,
    POOL_CAPACITY_OFFSET, POOL_ELEM_SIZE_OFFSET,
    // Barrier header field offsets
    BARRIER_ARRIVED_OFFSET, BARRIER_GENERATION_OFFSET, BARRIER_NUM_PARTICIPANTS_OFFSET,
    BARRIER_PADDING_OFFSET,
    // Semaphore header field offsets
    SEMAPHORE_COUNT_OFFSET, SEMAPHORE_WAITING_OFFSET, SEMAPHORE_MAX_COUNT_OFFSET,
    SEMAPHORE_PADDING_OFFSET,
    // Latch header field offsets
    LATCH_COUNT_OFFSET, LATCH_INITIAL_COUNT_OFFSET, LATCH_PADDING0_OFFSET, LATCH_PADDING1_OFFSET,
    // Event state field offsets
    EVENT_SIGNALED_OFFSET, EVENT_MODE_OFFSET, EVENT_WAITING_OFFSET,
    // Once flag field offsets
    ONCE_STATE_OFFSET,
    // RWLock state field offsets
    RWLOCK_READERS_OFFSET, RWLOCK_WRITER_ACTIVE_OFFSET,
    // Channel header field offsets
    CHANNEL_CAPACITY_OFFSET, CHANNEL_SENDERS_OFFSET, CHANNEL_RECEIVERS_OFFSET,
    CHANNEL_CLOSED_OFFSET, CHANNEL_SEND_SEQ_OFFSET, CHANNEL_RECV_SEQ_OFFSET,
    // Stream header field offsets
    STREAM_SEQUENCE_OFFSET, STREAM_SUBSCRIBERS_OFFSET, STREAM_CLOSED_OFFSET,
    STREAM_BUFFER_CAPACITY_OFFSET, STREAM_PADDING_OFFSET, STREAM_TRANSFORM_NAME_OFFSET,
    // Future header field offsets
    FUTURE_STATE_OFFSET, FUTURE_WAITERS_OFFSET, FUTURE_COMPLETION_TIME_OFFSET,
    // Lazy header field offsets
    LAZY_STATE_OFFSET, LAZY_COMPUTE_COUNT_OFFSET, LAZY_COMPLETION_TIME_OFFSET,
};

/// Minimum name length in characters.
pub const MIN_NAME_SIZE: usize = 3;

/// Maximum name length in characters (255 bytes including null terminator).
pub const MAX_NAME_SIZE: usize = 254;

/// Align `n` to the next 8-byte boundary.
/// Matches the spec: `(n + 7) & ~7`.
pub fn align8(n: usize) -> usize {
    align_up(n, SECTION_ALIGN)
}

/// Align `n` up to the next multiple of `a`.
/// `a` must be a power of two.
pub fn align_up(n: usize, a: usize) -> usize {
    (n + a - 1) & !(a - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align8() {
        assert_eq!(align8(0), 0);
        assert_eq!(align8(1), 8);
        assert_eq!(align8(7), 8);
        assert_eq!(align8(8), 8);
        assert_eq!(align8(9), 16);
        assert_eq!(align8(15), 16);
        assert_eq!(align8(16), 16);
        assert_eq!(align8(17), 24);
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 8), 0);
        assert_eq!(align_up(1, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(4, 4), 4);
        assert_eq!(align_up(5, 4), 8);
    }

    #[test]
    fn test_constants() {
        // Verify constants match SPECIFICATION.md values
        assert_eq!(TABLE_MAGIC, 0x5A53484D); // 'ZSHM'
        assert_eq!(TABLE_VERSION, 3);
        assert_eq!(XXHASH_SEED, 0x2FE7);
        assert_eq!(HEADER_SIZE, 32);
        assert_eq!(ENTRY_SIZE, 384);
        assert_eq!(MIN_NAME_SIZE, 3);
        assert_eq!(MAX_NAME_SIZE, 254);

        // Table size formula: 32 + max_entries * 384
        assert_eq!(HEADER_SIZE + 64 * ENTRY_SIZE, 32 + 64 * 384);
    }
}

