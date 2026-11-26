//! Lock-free MPMC queue using the Vyukov bounded queue algorithm.
//!
//! Provides the `Queue<T>` type for multi-producer multi-consumer lock-free
//! queues stored in shared memory. Uses per-slot sequence numbers for
//! correct concurrent operation.
//!
//! # Binary Layout
//! ```text
//! [header: 16 bytes][data: T * capacity][pad to 8-align][sequence: u32 * capacity]
//! ```
//!
//! Header layout (little-endian):
//! - head: u32 at offset 0 (monotonically increasing)
//! - tail: u32 at offset 4 (monotonically increasing)
//! - capacity: u32 at offset 8 (MUST be a power of two)
//! - elem_size: u32 at offset 12

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::{align8, QUEUE_TAIL_OFFSET, QUEUE_CAPACITY_OFFSET, QUEUE_ELEM_SIZE_OFFSET};
use crate::memory::Memory;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Size of the queue header in bytes.
const QUEUE_HEADER_SIZE: usize = 16;

/// A lock-free multi-producer multi-consumer queue stored in shared memory.
///
/// Uses the Vyukov bounded queue algorithm with per-slot sequence numbers.
/// Capacity is always rounded up to the next power of two.
pub struct Queue<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: u32,
    elem_size: u32,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Queue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Queue")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("elem_size", &self.elem_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Queue holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the element type is safe for concurrent access via raw pointers
// in process-shared memory. The Vyukov algorithm uses atomics for
// synchronization, making concurrent access safe.
unsafe impl<T: Pod> Send for Queue<T> {}
unsafe impl<T: Pod> Sync for Queue<T> {}

/// Round n up to the next power of two.
fn next_power_of_two(n: usize) -> usize {
    let mut p = 1;
    while p < n {
        p <<= 1;
    }
    p
}

impl<T: Pod> Queue<T> {
    /// Create a new queue in shared memory.
    ///
    /// The requested capacity is rounded up to the next power of two.
    /// Initializes the header, zeros the data area, and sets per-slot
    /// sequence numbers to their initial values (sequence[i] = i).
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the queue (stored in the table)
    /// * `capacity` -- Number of elements (will be rounded up to next power of two)
    pub fn new(memory: &Memory, name: &str, capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ZeroShmError::SizeError(
                "capacity must be greater than zero".to_string(),
            ));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Round up to next power of two
        let capacity = next_power_of_two(capacity);
        let elem_size = T::SIZE as u32;

        // Total size: header + aligned data + sequence array
        let total_size =
            QUEUE_HEADER_SIZE + align8(capacity * T::SIZE) + capacity * std::mem::size_of::<u32>();

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        // Write header: head=0, tail=0, capacity, elem_size (all little-endian)
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicU32>()).store(0, Ordering::Relaxed); // head
            base.add(QUEUE_TAIL_OFFSET).cast::<AtomicU32>().as_ref().unwrap().store(0, Ordering::Relaxed); // tail
            std::ptr::write(base.add(QUEUE_CAPACITY_OFFSET).cast::<u32>(), capacity as u32); // capacity (non-atomic write during init)
            std::ptr::write(base.add(QUEUE_ELEM_SIZE_OFFSET).cast::<u32>(), elem_size); // elem_size
        }

        // Zero-initialize data area
        let data_start = offset + QUEUE_HEADER_SIZE;
        unsafe {
            std::ptr::write_bytes(memory.at_mut(data_start), 0, capacity * T::SIZE);
        }

        // Initialize per-slot sequence numbers: sequence[i] = i
        let seq_start = data_start + align8(capacity * T::SIZE);
        unsafe {
            for i in 0..capacity {
                let seq_ptr = memory.at_mut(seq_start + i * 4).cast::<u32>();
                std::ptr::write(seq_ptr, i as u32);
            }
        }

        Ok(Queue {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity: capacity as u32,
            elem_size,
            _marker: PhantomData,
        })
    }

    /// Open an existing queue in shared memory.
    ///
    /// Validates that the stored capacity is a power of two and that the
    /// element size matches the type T.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the queue to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "queue '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;

        // Read header fields
        let capacity = unsafe {
            memory.at(offset + 8).cast::<u32>().read_unaligned()
        };
        let stored_elem_size = unsafe {
            memory.at(offset + 12).cast::<u32>().read_unaligned()
        };

        // Validate element size
        if stored_elem_size != T::SIZE as u32 {
            return Err(ZeroShmError::SizeError(format!(
                "element size mismatch for '{}': stored {}, expected {} (wrong element type?)",
                name, stored_elem_size, T::SIZE
            )));
        }

        // Validate capacity is a power of two
        if capacity & (capacity - 1) != 0 {
            return Err(ZeroShmError::SizeError(format!(
                "queue '{}' capacity {} is not a power of two (created by an old implementation?)",
                name, capacity
            )));
        }

        Ok(Queue {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            elem_size: stored_elem_size,
            _marker: PhantomData,
        })
    }

    /// Returns a pointer to the atomic head counter.
    fn head_ptr(&self) -> *const AtomicU32 {
        self.memory.at(self.offset).cast::<AtomicU32>()
    }

    /// Returns a pointer to the atomic tail counter.
    fn tail_ptr(&self) -> *const AtomicU32 {
        self.memory.at(self.offset + 4).cast::<AtomicU32>()
    }

    /// Returns a pointer to the sequence number for a given slot.
    fn seq_ptr(&self, slot: u32) -> *const AtomicU32 {
        let seq_start =
            self.offset + QUEUE_HEADER_SIZE + align8(self.capacity as usize * T::SIZE);
        let ptr = seq_start + (slot as usize) * 4;
        self.memory.at(ptr).cast::<AtomicU32>()
    }

    /// Try to push a value onto the queue (non-blocking).
    ///
    /// Returns `Ok(())` if successful, or `Err(ZeroShmError::Timeout)` if the queue is full.
    ///
    /// # Algorithm (Vyukov MPMC push)
    /// 1. Load tail
    /// 2. Compute slot = tail % capacity
    /// 3. Load slot sequence number
    /// 4. If seq == tail: CAS tail to tail+1, write data, store seq = tail+1
    /// 5. If seq < tail: queue is full, return error
    /// 6. Otherwise: retry
    pub fn try_push(&self, value: T) -> Result<()> {
        let cap = self.capacity;
        let tail_ptr = self.tail_ptr();

        loop {
            let tail = unsafe { (*tail_ptr).load(Ordering::Acquire) };
            let slot = tail % cap;
            let seq = unsafe { (*self.seq_ptr(slot)).load(Ordering::Acquire) };
            let diff = seq as i64 - tail as i64;

            if diff == 0 {
                // Slot is ready for writing; try to claim it
                if unsafe {
                    (*tail_ptr)
                        .compare_exchange_weak(tail, tail + 1, Ordering::Release, Ordering::Relaxed)
                        .is_ok()
                } {
                    // We own this slot -- write the data
                    let data_offset = self.offset + QUEUE_HEADER_SIZE + (slot as usize) * T::SIZE;
                    unsafe {
                        *self.memory.at_mut(data_offset).cast::<T>() = value;
                    }
                    // Publish: set sequence to tail+1 so consumers can see it
                    unsafe {
                        (*self.seq_ptr(slot)).store(tail + 1, Ordering::Release);
                    }
                    return Ok(());
                }
                // CAS failed, another producer got it; retry
            } else if diff < 0 {
                // Queue is full
                return Err(ZeroShmError::Timeout);
            }
            // diff > 0: stale tail or another producer in progress; retry
            std::thread::yield_now();
        }
    }

    /// Try to pop a value from the queue (non-blocking).
    ///
    /// Returns `Ok(Some(value))` if successful, or `Ok(None)` if the queue is empty.
    ///
    /// # Algorithm (Vyukov MPMC pop)
    /// 1. Load head
    /// 2. Compute slot = head % capacity
    /// 3. Load slot sequence number
    /// 4. If seq == head+1: CAS head to head+1, read data, store seq = head+capacity
    /// 5. If seq < head+1: queue is empty, return None
    /// 6. Otherwise: retry
    pub fn try_pop(&self) -> Result<Option<T>> {
        let cap = self.capacity;
        let head_ptr = self.head_ptr();

        loop {
            let head = unsafe { (*head_ptr).load(Ordering::Acquire) };
            let slot = head % cap;
            let seq = unsafe { (*self.seq_ptr(slot)).load(Ordering::Acquire) };
            let diff = seq as i64 - (head as i64 + 1);

            if diff == 0 {
                // Slot contains data; try to claim it
                if unsafe {
                    (*head_ptr)
                        .compare_exchange_weak(head, head + 1, Ordering::Release, Ordering::Relaxed)
                        .is_ok()
                } {
                    // We own this slot -- read the data
                    let data_offset = self.offset + QUEUE_HEADER_SIZE + (slot as usize) * T::SIZE;
                    let value = unsafe { *self.memory.at(data_offset).cast::<T>() };
                    // Release: set sequence to head+capacity so producers can reuse
                    unsafe {
                        (*self.seq_ptr(slot)).store(head + cap, Ordering::Release);
                    }
                    return Ok(Some(value));
                }
                // CAS failed, another consumer got it; retry
            } else if diff < 0 {
                // Queue is empty
                return Ok(None);
            }
            // diff > 0: stale head or another consumer in progress; retry
            std::thread::yield_now();
        }
    }

    /// Push a value onto the queue, spinning until successful.
    ///
    /// This is a blocking version of `try_push` that spin-waits when the
    /// queue is full.
    pub fn push(&self, value: T) -> Result<()> {
        loop {
            match self.try_push(value) {
                Ok(()) => return Ok(()),
                Err(ZeroShmError::Timeout) => {
                    // Queue is full, spin-wait and retry
                    std::thread::yield_now();
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Pop a value from the queue, spinning until one is available.
    ///
    /// This is a blocking version of `try_pop` that spin-waits when the
    /// queue is empty.
    pub fn pop(&self) -> Result<T> {
        loop {
            match self.try_pop()? {
                Some(value) => return Ok(value),
                None => {
                    // Queue is empty, spin-wait and retry
                    std::thread::yield_now();
                }
            }
        }
    }

    /// Returns the approximate number of elements in the queue.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn len(&self) -> usize {
        let head = unsafe { (*self.head_ptr()).load(Ordering::Acquire) };
        let tail = unsafe { (*self.tail_ptr()).load(Ordering::Acquire) };
        // u32 subtraction handles wraparound correctly
        (tail.wrapping_sub(head)) as usize
    }

    /// Returns true if the queue appears empty.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if the queue appears full.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity as usize
    }

    /// Returns the queue capacity (always a power of two).
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Returns the queue name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset of the queue in the memory segment.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::sync::atomic::{AtomicUsize, Ordering as SyncOrdering};
    use std::thread;

    #[test]
    fn test_queue_push_pop_fifo() {
        let name = "/test_step8_fifo";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create queue with capacity 16 (rounds to 16)
        let queue: Queue<u32> =
            Queue::new(&memory, "fifo_queue", 16).expect("create queue should succeed");
        assert_eq!(queue.capacity(), 16);

        // Push 100 items (queue will fill and block, but we use try_push)
        // Actually, let's push and pop interleaved to test FIFO
        for i in 0..100 {
            queue.try_push(i).expect("push should succeed");
            let val = queue.try_pop().expect("pop should succeed");
            assert_eq!(val, Some(i), "item {} should be popped in order", i);
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_fill_and_drain() {
        let name = "/test_step8_fill";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create queue with capacity 8
        let queue: Queue<i32> =
            Queue::new(&memory, "fill_queue", 8).expect("create queue should succeed");
        assert_eq!(queue.capacity(), 8);

        // Push 8 items (fill the queue)
        for i in 0..8 {
            queue.try_push(i).expect("push should succeed");
        }

        // Queue should be full now
        assert!(queue.is_full());
        let result = queue.try_push(99);
        assert!(matches!(result, Err(ZeroShmError::Timeout)), "push to full queue should fail");

        // Pop all 8 items and verify FIFO order
        for i in 0..8 {
            let val = queue.try_pop().expect("pop should succeed");
            assert_eq!(val, Some(i), "item {} should be popped in FIFO order", i);
        }

        // Queue should be empty now
        assert!(queue.is_empty());
        let val = queue.try_pop().expect("pop should succeed");
        assert_eq!(val, None, "pop from empty queue should return None");

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_capacity_rounding() {
        let name = "/test_step8_round";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Request capacity 5, should round to 8
        let queue: Queue<u64> =
            Queue::new(&memory, "round_queue", 5).expect("create queue should succeed");
        assert_eq!(queue.capacity(), 8, "capacity 5 should round to 8");

        // Request capacity 100, should round to 128
        let queue2: Queue<f32> =
            Queue::new(&memory, "round_queue2", 100).expect("create queue should succeed");
        assert_eq!(queue2.capacity(), 128, "capacity 100 should round to 128");

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_multithreaded_producers_consumers() {
        let name = "/test_step8_mt";
        let size = 1024 * 1024; // 1MB for larger queue

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create queue with capacity 1024
        let queue: Queue<u32> =
            Queue::new(&memory, "mt_queue", 1024).expect("create queue should succeed");

        let queue = Arc::new(queue);
        let num_producers = 4;
        let num_consumers = 4;
        let items_per_producer = 10_000;
        let total_items = num_producers * items_per_producer;

        // Shared counter array to track which items were received
        let counters: Vec<Arc<AtomicUsize>> = (0..total_items)
            .map(|_| Arc::new(AtomicUsize::new(0)))
            .collect();

        // Spawn producers
        let mut producer_handles = vec![];
        for p in 0..num_producers {
            let q = Arc::clone(&queue);
            let handle = thread::spawn(move || {
                for i in 0..items_per_producer {
                    let item = p * items_per_producer + i;
                    // Use push (blocking) to avoid losing items
                    q.push(item as u32).expect("push should succeed");
                }
            });
            producer_handles.push(handle);
        }

        // Spawn consumers
        let mut consumer_handles = vec![];
        for _ in 0..num_consumers {
            let q = Arc::clone(&queue);
            let counters = counters
                .iter()
                .map(|c| Arc::clone(c))
                .collect::<Vec<_>>();
            let handle = thread::spawn(move || {
                loop {
                    match q.try_pop() {
                        Ok(Some(item)) => {
                            let idx = item as usize;
                            counters[idx].fetch_add(1, SyncOrdering::SeqCst);
                        }
                        Ok(None) => {
                            // Queue empty, check if all items received
                            let total_received: usize =
                                counters.iter().map(|c| c.load(SyncOrdering::SeqCst)).sum();
                            if total_received >= total_items {
                                break;
                            }
                            std::thread::yield_now();
                        }
                        Err(_) => {
                            std::thread::yield_now();
                        }
                    }
                }
            });
            consumer_handles.push(handle);
        }

        // Wait for all producers to finish
        for handle in producer_handles {
            handle.join().expect("producer thread should not panic");
        }

        // Wait for all consumers to finish
        for handle in consumer_handles {
            handle.join().expect("consumer thread should not panic");
        }

        // Verify all items were received exactly once
        for (i, counter) in counters.iter().enumerate() {
            assert_eq!(
                counter.load(SyncOrdering::SeqCst),
                1,
                "item {} was received {} times (expected 1)",
                i,
                counter.load(SyncOrdering::SeqCst)
            );
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_open_persistence() {
        let name = "/test_step8_persist";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create queue and push some items
        let queue: Queue<f32> =
            Queue::new(&memory, "persist_queue", 16).expect("create queue should succeed");
        queue.try_push(1.0).expect("push should succeed");
        queue.try_push(2.0).expect("push should succeed");
        queue.try_push(3.0).expect("push should succeed");

        // Pop one item
        let val = queue.try_pop().expect("pop should succeed");
        assert_eq!(val, Some(1.0));

        drop(queue);

        // Reopen the queue
        let queue2: Queue<f32> =
            Queue::open(&memory, "persist_queue").expect("reopen should succeed");
        assert_eq!(queue2.capacity(), 16);

        // Remaining items should still be there
        let val = queue2.try_pop().expect("pop should succeed");
        assert_eq!(val, Some(2.0), "second item should be 2.0");
        let val = queue2.try_pop().expect("pop should succeed");
        assert_eq!(val, Some(3.0), "third item should be 3.0");

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_type_mismatch() {
        let name = "/test_step8_mismatch";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create queue of u32 (elem_size=4)
        let _queue: Queue<u32> =
            Queue::new(&memory, "mismatch_queue", 8).expect("create queue should succeed");

        // Try to open as u64 (elem_size=8) -- should fail
        let result: Result<Queue<u64>> = Queue::open(&memory, "mismatch_queue");
        assert!(
            matches!(result, Err(ZeroShmError::SizeError(_))),
            "opening u32 queue as u64 should fail with SizeError, got {:?}",
            result
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_not_found() {
        let name = "/test_step8_notfound";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Try to open non-existent queue
        let result: Result<Queue<u32>> = Queue::open(&memory, "nonexistent");
        assert!(
            matches!(result, Err(ZeroShmError::NotFoundError(_))),
            "opening non-existent queue should fail with NotFoundError, got {:?}",
            result
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_queue_stress_1m_operations() {
        let name = "/test_step8_stress";
        let size = 4 * 1024 * 1024; // 4MB

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create large queue
        let queue: Queue<u32> =
            Queue::new(&memory, "stress_queue", 4096).expect("create queue should succeed");

        let queue = Arc::new(queue);
        let num_producers = 8;
        let num_consumers = 8;
        let items_per_producer = 100_000; // 800K total, ~1M operations with retries

        let received_count = Arc::new(AtomicUsize::new(0));

        // Spawn producers
        let mut producer_handles = vec![];
        for p in 0..num_producers {
            let q = Arc::clone(&queue);
            let handle = thread::spawn(move || {
                for i in 0..items_per_producer {
                    let item = (p * items_per_producer + i) as u32;
                    q.push(item).expect("push should succeed");
                }
            });
            producer_handles.push(handle);
        }

        // Spawn consumers
        let mut consumer_handles = vec![];
        let total_items = num_producers * items_per_producer;
        for _ in 0..num_consumers {
            let q = Arc::clone(&queue);
            let count = Arc::clone(&received_count);
            let handle = thread::spawn(move || {
                loop {
                    match q.try_pop() {
                        Ok(Some(_)) => {
                            count.fetch_add(1, SyncOrdering::SeqCst);
                        }
                        Ok(None) => {
                            if count.load(SyncOrdering::SeqCst) >= total_items {
                                break;
                            }
                            std::thread::yield_now();
                        }
                        Err(_) => {
                            std::thread::yield_now();
                        }
                    }
                }
            });
            consumer_handles.push(handle);
        }

        // Wait for all producers
        for handle in producer_handles {
            handle.join().expect("producer thread should not panic");
        }

        // Wait for all consumers
        for handle in consumer_handles {
            handle.join().expect("consumer thread should not panic");
        }

        // Verify all items received
        assert_eq!(
            received_count.load(SyncOrdering::SeqCst),
            total_items,
            "expected {} items, received {}",
            total_items,
            received_count.load(SyncOrdering::SeqCst)
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }
}

