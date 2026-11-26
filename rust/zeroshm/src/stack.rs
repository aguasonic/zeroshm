//! Lock-free LIFO stack using a 4-state slot CAS protocol.
//!
//! Provides the `Stack<T>` type for lock-free last-in-first-out stacks stored
//! in shared memory. Uses a 4-state per-slot protocol for crash-safe concurrent
//! access.
//!
//! # Binary Layout
//! ```text
//! [header: 16 bytes][data: T * capacity][pad to 8-align][state: u32 * capacity]
//! ```
//!
//! Header layout (little-endian):
//! - top: i32 at offset 0 (index of top element, -1 when empty)
//! - capacity: u32 at offset 4
//! - elem_size: u32 at offset 8
//! - reserved: u32 at offset 12 (pads header to 16 bytes)
//!
//! # State Machine
//! ```text
//! EMPTY(0) -> WRITING(1) -> READY(2) -> READING(3) -> EMPTY(0)
//! ```
//!
//! Push: CAS(top) to reserve slot, CAS(slot: EMPTY->WRITING), write data, store(READY)
//! Pop:  CAS(top) to reserve slot, CAS(slot: READY->READING), read data, store(EMPTY)

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::{align8, STACK_CAPACITY_OFFSET, STACK_ELEM_SIZE_OFFSET, STACK_RESERVED_OFFSET};
use crate::memory::Memory;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::Arc;

/// Size of the stack header in bytes.
const STACK_HEADER_SIZE: usize = 16;

/// Per-slot state: empty (no data, available for push).
const SLOT_EMPTY: u32 = 0;
/// Per-slot state: a pusher has claimed the slot and is writing data.
const SLOT_WRITING: u32 = 1;
/// Per-slot state: data is fully written and available for pop.
const SLOT_READY: u32 = 2;
/// Per-slot state: a popper has claimed the slot and is reading data.
const SLOT_READING: u32 = 3;

/// Bound on every slot-state spin loop. A peer that crashes mid-operation
/// leaves its slot permanently claimed; an unbounded spin would then hang
/// every later operation that lands on that slot. Bailing out makes
/// push/pop best-effort: they may fail spuriously if a peer died (or under
/// pathological contention), but they never hang.
const MAX_SPINS: usize = 10000;

/// A lock-free LIFO stack stored in shared memory.
///
/// Uses a 4-state per-slot CAS protocol for correct concurrent operation
/// with crash safety via bounded spin loops.
pub struct Stack<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: u32,
    elem_size: u32,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Stack<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stack")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("elem_size", &self.elem_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Stack holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the element type is safe for concurrent access via raw pointers
// in process-shared memory. The 4-state CAS protocol uses atomics for
// synchronization, making concurrent access safe.
unsafe impl<T: Pod> Send for Stack<T> {}
unsafe impl<T: Pod> Sync for Stack<T> {}

impl<T: Pod> Stack<T> {
    /// Create a new stack in shared memory.
    ///
    /// Allocates space for the header, data area, padding, and per-slot
    /// state array. Initializes all slots to EMPTY and sets top to -1.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the stack (stored in the table)
    /// * `capacity` -- Maximum number of elements
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

        let capacity = capacity as u32;
        let elem_size = T::SIZE as u32;

        // Total size: header + aligned data + state array
        let total_size =
            STACK_HEADER_SIZE + align8(capacity as usize * T::SIZE) + capacity as usize * 4;

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        unsafe {
            let base = memory.at_mut(offset);
            // Write header: top=-1 (empty), capacity, elem_size, reserved=0
            (*base.cast::<AtomicI32>()).store(-1, Ordering::Relaxed);
            std::ptr::write(base.add(STACK_CAPACITY_OFFSET).cast::<u32>(), capacity);
            std::ptr::write(base.add(STACK_ELEM_SIZE_OFFSET).cast::<u32>(), elem_size);
            std::ptr::write(base.add(STACK_RESERVED_OFFSET).cast::<u32>(), 0);
        }

        // Zero-initialize data area, padding, and state area (state[i] = EMPTY = 0)
        let data_start = offset + STACK_HEADER_SIZE;
        unsafe {
            std::ptr::write_bytes(
                memory.at_mut(data_start),
                0,
                align8(capacity as usize * T::SIZE) + capacity as usize * 4,
            );
        }

        // Release fence to ensure all initialization is visible before
        // other threads can see the table entry
        std::sync::atomic::fence(Ordering::Release);

        Ok(Stack {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            elem_size,
            _marker: PhantomData,
        })
    }

    /// Open an existing stack in shared memory.
    ///
    /// Validates that the stored element size matches the type T.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the stack to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "stack '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;

        // Read header fields
        let capacity = unsafe {
            memory.at(offset + 4).cast::<u32>().read_unaligned()
        };
        let stored_elem_size = unsafe {
            memory.at(offset + 8).cast::<u32>().read_unaligned()
        };

        // Validate element size
        if stored_elem_size != T::SIZE as u32 {
            return Err(ZeroShmError::SizeError(format!(
                "element size mismatch for '{}': stored {}, expected {} (wrong element type?)",
                name,
                stored_elem_size,
                T::SIZE
            )));
        }

        Ok(Stack {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            elem_size: stored_elem_size,
            _marker: PhantomData,
        })
    }

    /// Returns a pointer to the atomic top counter.
    fn top_ptr(&self) -> *const AtomicI32 {
        self.memory.at(self.offset).cast::<AtomicI32>()
    }

    /// Returns a pointer to the state for slot i.
    fn slot_state_ptr(&self, i: usize) -> *const AtomicU32 {
        let state_start =
            self.offset + STACK_HEADER_SIZE + align8(self.capacity as usize * T::SIZE);
        let ptr = state_start + i * 4;
        self.memory.at(ptr).cast::<AtomicU32>()
    }

    /// Try to push a value onto the stack (non-blocking).
    ///
    /// Returns `Ok(true)` if successful, or `Ok(false)` if the stack is full.
    ///
    /// # Algorithm (4-state CAS push)
    /// 1. CAS top from currentTop to currentTop+1 (reserve slot)
    /// 2. CAS slot state EMPTY -> WRITING (bounded spin)
    /// 3. Write data (we have exclusive ownership)
    /// 4. Store slot state as READY (publish)
    ///
    /// If step 2 fails (slot is stuck), undo the top reservation and return false.
    pub fn try_push(&self, value: T) -> Result<bool> {
        let top_ptr = self.top_ptr();

        // Step 1: Reserve a slot by CAS-advancing top
        let mut current_top = unsafe { (*top_ptr).load(Ordering::Acquire) };
        loop {
            if current_top >= (self.capacity - 1) as i32 {
                return Ok(false); // full
            }
            let new_top = current_top + 1;
            match unsafe {
                (*top_ptr).compare_exchange_weak(
                    current_top,
                    new_top,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
            } {
                Ok(_) => {
                    // Step 2: CAS the slot state EMPTY -> WRITING (bounded spin)
                    let state_ptr = self.slot_state_ptr(new_top as usize);
                    let mut claimed = false;
                    for _ in 0..MAX_SPINS {
                        if unsafe {
                            (*state_ptr)
                                .compare_exchange(SLOT_EMPTY, SLOT_WRITING, Ordering::AcqRel, Ordering::Acquire)
                                .is_ok()
                        } {
                            claimed = true;
                            break;
                        }
                        std::thread::yield_now();
                    }
                    if !claimed {
                        // Best-effort undo of the top reservation
                        unsafe {
                            (*top_ptr)
                                .compare_exchange(new_top, current_top, Ordering::Release, Ordering::Relaxed)
                                .ok();
                        }
                        return Ok(false);
                    }

                    // Step 3: Write data (we have exclusive ownership)
                    let data_offset = self.offset + STACK_HEADER_SIZE + new_top as usize * T::SIZE;
                    unsafe {
                        *self.memory.at_mut(data_offset).cast::<T>() = value;
                    }

                    // Step 4: Publish: WRITING -> READY
                    unsafe {
                        (*state_ptr).store(SLOT_READY, Ordering::Release);
                    }
                    return Ok(true);
                }
                Err(actual) => {
                    current_top = actual;
                }
            }
        }
    }

    /// Try to pop a value from the stack (non-blocking).
    ///
    /// Returns `Ok(Some(value))` if successful, or `Ok(None)` if the stack is empty.
    ///
    /// # Algorithm (4-state CAS pop)
    /// 1. CAS top from currentTop to currentTop-1 (reserve slot)
    /// 2. CAS slot state READY -> READING (bounded spin)
    /// 3. Read data (we have exclusive ownership)
    /// 4. Store slot state as EMPTY (release)
    ///
    /// If step 2 fails (slot is stuck), undo the top reservation and return None.
    pub fn try_pop(&self) -> Result<Option<T>> {
        let top_ptr = self.top_ptr();

        // Step 1: Reserve a slot by CAS-decrementing top
        let mut current_top = unsafe { (*top_ptr).load(Ordering::Acquire) };
        loop {
            if current_top < 0 {
                return Ok(None); // empty
            }
            let new_top = current_top - 1;
            match unsafe {
                (*top_ptr).compare_exchange_weak(
                    current_top,
                    new_top,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
            } {
                Ok(_) => {
                    // Step 2: CAS the slot state READY -> READING (bounded spin)
                    let state_ptr = self.slot_state_ptr(current_top as usize);
                    let mut claimed = false;
                    for _ in 0..MAX_SPINS {
                        if unsafe {
                            (*state_ptr)
                                .compare_exchange(SLOT_READY, SLOT_READING, Ordering::AcqRel, Ordering::Acquire)
                                .is_ok()
                        } {
                            claimed = true;
                            break;
                        }
                        std::thread::yield_now();
                    }
                    if !claimed {
                        // Best-effort undo: put the item back under top so it is not dropped
                        unsafe {
                            (*top_ptr)
                                .compare_exchange(new_top, current_top, Ordering::Release, Ordering::Relaxed)
                                .ok();
                        }
                        return Ok(None);
                    }

                    // Step 3: Read data (we have exclusive ownership)
                    let data_offset = self.offset + STACK_HEADER_SIZE + current_top as usize * T::SIZE;
                    let value = unsafe { *self.memory.at(data_offset).cast::<T>() };

                    // Step 4: Release: READING -> EMPTY
                    unsafe {
                        (*state_ptr).store(SLOT_EMPTY, Ordering::Release);
                    }
                    return Ok(Some(value));
                }
                Err(actual) => {
                    current_top = actual;
                }
            }
        }
    }

    /// Push a value onto the stack, spinning until successful.
    ///
    /// This is a blocking version of `try_push` that spin-waits when the
    /// stack is full.
    pub fn push(&self, value: T) -> Result<()> {
        loop {
            if self.try_push(value)? {
                return Ok(());
            }
            // Stack is full, spin-wait and retry
            std::thread::yield_now();
        }
    }

    /// Pop a value from the stack, spinning until one is available.
    ///
    /// This is a blocking version of `try_pop` that spin-waits when the
    /// stack is empty.
    pub fn pop(&self) -> Result<T> {
        loop {
            if let Some(value) = self.try_pop()? {
                return Ok(value);
            }
            // Stack is empty, spin-wait and retry
            std::thread::yield_now();
        }
    }

    /// Returns the approximate number of elements in the stack.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn len(&self) -> usize {
        let top = unsafe { (*self.top_ptr()).load(Ordering::Acquire) };
        if top < 0 {
            0
        } else {
            (top + 1) as usize
        }
    }

    /// Returns true if the stack appears empty.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if the stack appears full.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn is_full(&self) -> bool {
        (unsafe { (*self.top_ptr()).load(Ordering::Acquire) }) >= (self.capacity - 1) as i32
    }

    /// Returns the stack capacity.
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Returns the stack name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset of the stack in the memory segment.
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
    fn test_stack_push_pop_lifo() {
        let name = "/test_step9_lifo";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create stack with capacity 16
        let stack: Stack<u32> =
            Stack::new(&memory, "lifo_stack", 16).expect("create stack should succeed");
        assert_eq!(stack.capacity(), 16);
        assert!(stack.is_empty());

        // Push 5 items: 1, 2, 3, 4, 5
        for i in 1..=5 {
            stack
                .try_push(i)
                .expect("push should succeed")
                .then_some(())
                .expect("stack should not be full");
        }
        assert_eq!(stack.len(), 5);

        // Pop 5 items, verify LIFO order: 5, 4, 3, 2, 1
        for expected in (1..=5).rev() {
            let val = stack
                .try_pop()
                .expect("pop should succeed")
                .unwrap_or_else(|| panic!("stack should not be empty"));
            assert_eq!(val, expected, "LIFO order violated");
        }
        assert!(stack.is_empty());

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_fill_and_drain() {
        let name = "/test_step9_fill";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create stack with capacity 8
        let stack: Stack<u32> =
            Stack::new(&memory, "fill_stack", 8).expect("create stack should succeed");

        // Fill to capacity
        for i in 0..8 {
            assert!(
                stack.try_push(i).expect("push should not error"),
                "stack should not be full yet"
            );
        }
        assert!(stack.is_full());
        assert_eq!(stack.len(), 8);

        // Push should now fail (full)
        assert!(
            !stack.try_push(99).expect("push should not error"),
            "push to full stack should return false"
        );

        // Drain and verify LIFO order
        for expected in (0..8).rev() {
            let val = stack
                .try_pop()
                .expect("pop should succeed")
                .unwrap_or_else(|| panic!("stack should not be empty"));
            assert_eq!(val, expected, "LIFO order violated");
        }
        assert!(stack.is_empty());

        // Pop should now return None (empty)
        assert!(stack
            .try_pop()
            .expect("pop should not error")
            .is_none(), "pop from empty stack should return None");

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_multithreaded_producers_consumers() {
        let name = "/test_step9_mt";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create stack with capacity 256
        let stack: Arc<Stack<u32>> = Arc::new(
            Stack::new(&memory, "mt_stack", 256).expect("create stack should succeed"),
        );

        let num_producers = 4;
        let items_per_producer = 10_000;
        let num_consumers = 4;
        let total_items = num_producers * items_per_producer;

        // Track which items were received (each item is produced exactly once)
        let received: Arc<Vec<AtomicUsize>> =
            Arc::new((0..total_items).map(|_| AtomicUsize::new(0)).collect());

        // Producers: each pushes items 0..items_per_producer (offset by producer id)
        let mut producer_handles = Vec::new();
        for p in 0..num_producers {
            let stack = Arc::clone(&stack);
            let handle = thread::spawn(move || {
                for i in 0..items_per_producer {
                    let value = p * items_per_producer + i;
                    // Use push() which spin-waits when full
                    stack.push(value as u32).expect("push should succeed");
                }
            });
            producer_handles.push(handle);
        }

        // Consumers: pop until all items received
        let mut consumer_handles = Vec::new();
        for _ in 0..num_consumers {
            let stack = Arc::clone(&stack);
            let received = Arc::clone(&received);
            let handle = thread::spawn(move || {
                loop {
                    match stack.try_pop().expect("pop should not error") {
                        Some(value) => {
                            received[value as usize].fetch_add(1, SyncOrdering::SeqCst);
                        }
                        None => {
                            // Check if all items have been received
                            let total: usize = received
                                .iter()
                                .map(|c| c.load(SyncOrdering::SeqCst))
                                .sum();
                            if total >= total_items {
                                break;
                            }
                            std::thread::yield_now();
                        }
                    }
                }
            });
            consumer_handles.push(handle);
        }

        // Wait for producers to finish first
        for handle in producer_handles {
            handle.join().expect("producer should not panic");
        }

        // Wait for consumers to finish
        for handle in consumer_handles {
            handle.join().expect("consumer should not panic");
        }

        // Verify all items received exactly once
        for i in 0..total_items {
            let count = received[i].load(SyncOrdering::SeqCst);
            assert_eq!(count, 1, "item {} received {} times (expected 1)", i, count);
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_open_persistence() {
        let name = "/test_step9_persist";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create stack and push items
        {
            let stack: Stack<u32> =
                Stack::new(&memory, "persist_stack", 16).expect("create stack should succeed");
            stack
                .try_push(10)
                .expect("push should succeed")
                .then_some(())
                .expect("should not be full");
            stack
                .try_push(20)
                .expect("push should succeed")
                .then_some(())
                .expect("should not be full");
            stack
                .try_push(30)
                .expect("push should succeed")
                .then_some(())
                .expect("should not be full");
            // Pop one item
            let val = stack
                .try_pop()
                .expect("pop should succeed")
                .unwrap_or_else(|| panic!("should not be empty"));
            assert_eq!(val, 30);
        }

        // Reopen stack (simulates another process)
        let memory2 = Memory::open(name, 64).expect("reopen should succeed");
        let stack: Stack<u32> =
            Stack::open(&memory2, "persist_stack").expect("open stack should succeed");
        assert_eq!(stack.len(), 2);

        // Pop remaining items in LIFO order
        let val = stack
            .try_pop()
            .expect("pop should succeed")
            .unwrap_or_else(|| panic!("should not be empty"));
        assert_eq!(val, 20);

        let val = stack
            .try_pop()
            .expect("pop should succeed")
            .unwrap_or_else(|| panic!("should not be empty"));
        assert_eq!(val, 10);

        assert!(stack.is_empty());

        // Clean up
        memory2.unlink().expect("unlink should succeed");
        drop(memory2);
        drop(memory);
    }

    #[test]
    fn test_stack_type_mismatch() {
        let name = "/test_step9_mismatch";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create u32 stack
        let _stack: Stack<u32> =
            Stack::new(&memory, "mismatch_stack", 16).expect("create stack should succeed");

        // Try to open as u64 -- should fail (element size mismatch)
        let result: Result<Stack<u64>> = Stack::open(&memory, "mismatch_stack");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::SizeError(msg) => {
                assert!(msg.contains("element size mismatch"));
            }
            other => panic!("expected SizeError, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_not_found() {
        let name = "/test_step9_notfound";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Try to open non-existent stack
        let result: Result<Stack<u32>> = Stack::open(&memory, "nonexistent_stack");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(msg) => {
                assert!(msg.contains("nonexistent_stack"));
            }
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_duplicate_name() {
        let name = "/test_step9_dup";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create stack
        let _stack: Stack<u32> =
            Stack::new(&memory, "dup_stack", 16).expect("create stack should succeed");

        // Try to create another stack with same name
        let result: Result<Stack<u32>> = Stack::new(&memory, "dup_stack", 16);
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(msg) => {
                assert!(msg.contains("dup_stack"));
            }
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_different_types() {
        let name = "/test_step9_types";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create and use u8 stack
        let stack_u8: Stack<u8> =
            Stack::new(&memory, "u8_stack", 16).expect("create u8 stack should succeed");
        stack_u8
            .try_push(42)
            .expect("push should succeed")
            .then_some(())
            .expect("should not be full");
        let val = stack_u8
            .try_pop()
            .expect("pop should succeed")
            .unwrap_or_else(|| panic!("should not be empty"));
        assert_eq!(val, 42);

        // Create and use i32 stack
        let stack_i32: Stack<i32> =
            Stack::new(&memory, "i32_stack", 16).expect("create i32 stack should succeed");
        stack_i32
            .try_push(-99)
            .expect("push should succeed")
            .then_some(())
            .expect("should not be full");
        let val = stack_i32
            .try_pop()
            .expect("pop should succeed")
            .unwrap_or_else(|| panic!("should not be empty"));
        assert_eq!(val, -99);

        // Create and use f64 stack
        let stack_f64: Stack<f64> =
            Stack::new(&memory, "f64_stack", 16).expect("create f64 stack should succeed");
        stack_f64
            .try_push(3.14)
            .expect("push should succeed")
            .then_some(())
            .expect("should not be full");
        let val = stack_f64
            .try_pop()
            .expect("pop should succeed")
            .unwrap_or_else(|| panic!("should not be empty"));
        assert!((val - 3.14).abs() < 1e-10);

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_stack_stress_1m_operations() {
        let name = "/test_step9_stress";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create stack with capacity 1024
        let stack: Arc<Stack<u32>> = Arc::new(
            Stack::new(&memory, "stress_stack", 1024).expect("create stack should succeed"),
        );

        let num_producers = 8;
        let items_per_producer = 50_000;
        let num_consumers = 8;
        let total_items = num_producers * items_per_producer;

        let received: Arc<Vec<AtomicUsize>> =
            Arc::new((0..total_items).map(|_| AtomicUsize::new(0)).collect());

        // Producers
        let mut producer_handles = Vec::new();
        for p in 0..num_producers {
            let stack = Arc::clone(&stack);
            let handle = thread::spawn(move || {
                for i in 0..items_per_producer {
                    let value = p * items_per_producer + i;
                    stack.push(value as u32).expect("push should succeed");
                }
            });
            producer_handles.push(handle);
        }

        // Consumers
        let mut consumer_handles = Vec::new();
        for _ in 0..num_consumers {
            let stack = Arc::clone(&stack);
            let received = Arc::clone(&received);
            let handle = thread::spawn(move || {
                loop {
                    match stack.try_pop().expect("pop should not error") {
                        Some(value) => {
                            received[value as usize].fetch_add(1, SyncOrdering::SeqCst);
                        }
                        None => {
                            let total: usize = received
                                .iter()
                                .map(|c| c.load(SyncOrdering::SeqCst))
                                .sum();
                            if total >= total_items {
                                break;
                            }
                            std::thread::yield_now();
                        }
                    }
                }
            });
            consumer_handles.push(handle);
        }

        // Wait for producers
        for handle in producer_handles {
            handle.join().expect("producer should not panic");
        }

        // Wait for consumers
        for handle in consumer_handles {
            handle.join().expect("consumer should not panic");
        }

        // Verify no data loss
        let total_received: usize = received
            .iter()
            .map(|c| c.load(SyncOrdering::SeqCst))
            .sum();
        assert_eq!(
            total_received, total_items,
            "expected {} items, got {}",
            total_items, total_received
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }
}

