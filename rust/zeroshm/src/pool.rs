//! Object pool with lock-free free list and ABA protection.
//!
//! Provides the `Pool<T>` type for fixed-size object pools stored in shared
//! memory. Uses a tagged pointer (generation + index) free list with CAS for
//! ABA-safe concurrent allocation and deallocation.
//!
//! # Binary Layout
//! ```text
//! [header: 24 bytes][nodes: Node * capacity]
//! ```
//!
//! Header layout (little-endian):
//! - free_head: u64 at offset 0 (tagged pointer: [generation:32][index:32], atomic)
//! - allocated: u32 at offset 8 (number of allocated items, atomic)
//! - padding: u32 at offset 12 (alignment padding)
//! - capacity: u32 at offset 16 (total number of slots)
//! - elem_size: u32 at offset 20 (size of each element in bytes)
//!
//! Node layout:
//! - data: T (raw bytes)
//! - next: u32 (index of next free node, atomic)

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::{align8, POOL_ALLOCATED_OFFSET, POOL_PADDING_OFFSET, POOL_CAPACITY_OFFSET, POOL_ELEM_SIZE_OFFSET};
use crate::memory::Memory;
#[cfg(test)]
use crate::platform;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Size of the pool header in bytes.
const POOL_HEADER_SIZE: usize = 24;

/// Sentinel value for end of free list.
const NULL_INDEX: u32 = 0xFFFFFFFF;

/// Pack a tagged pointer: [generation:32][index:32].
fn pack_tagged(index: u32, generation: u32) -> u64 {
    ((generation as u64) << 32) | (index as u64)
}

/// Extract the index from a tagged pointer.
fn unpack_index(tagged: u64) -> u32 {
    (tagged & 0xFFFFFFFF) as u32
}

/// Extract the generation from a tagged pointer.
fn unpack_generation(tagged: u64) -> u32 {
    (tagged >> 32) as u32
}

/// An object pool with a lock-free free list stored in shared memory.
///
/// Uses tagged pointers (generation + index) with CAS to prevent the ABA
/// problem in concurrent allocation/deallocation.
pub struct Pool<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: u32,
    elem_size: u32,
    node_size: usize,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Pool<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("elem_size", &self.elem_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Pool holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the type is safe for concurrent access via raw pointers.
// The tagged pointer CAS protocol prevents ABA problems.
unsafe impl<T: Pod> Send for Pool<T> {}
unsafe impl<T: Pod> Sync for Pool<T> {}

impl<T: Pod> Pool<T> {
    /// Create a new pool in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the pool (stored in the table)
    /// * `capacity` -- Number of slots
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

        // Node: data at 0, next at align8(T::SIZE). Node size aligned to 8 bytes.
        let node_size = align8(align8(T::SIZE) + 4);

        // Check for overflow
        if capacity as usize > (usize::MAX - POOL_HEADER_SIZE) / node_size {
            return Err(ZeroShmError::SizeError("capacity too large".to_string()));
        }

        let total_size = POOL_HEADER_SIZE + node_size * capacity as usize;

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        // Write header: free_head=tagged(0,0), allocated=0, padding=0, capacity, elem_size
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicU64>()).store(pack_tagged(0, 0), Ordering::Relaxed); // free_head
            base.add(POOL_ALLOCATED_OFFSET).cast::<AtomicU32>().as_ref().unwrap().store(0, Ordering::Relaxed); // allocated
            std::ptr::write(base.add(POOL_PADDING_OFFSET).cast::<u32>(), 0); // padding
            std::ptr::write(base.add(POOL_CAPACITY_OFFSET).cast::<u32>(), capacity); // capacity
            std::ptr::write(base.add(POOL_ELEM_SIZE_OFFSET).cast::<u32>(), elem_size); // elem_size
        }

        // Initialize free list: nodes[i].next = i+1, last node = NULL_INDEX
        unsafe {
            for i in 0..capacity as usize {
                let next_ptr = offset + POOL_HEADER_SIZE + i * node_size + align8(T::SIZE);
                let next_val = if i < capacity as usize - 1 {
                    (i + 1) as u32
                } else {
                    NULL_INDEX
                };
                *memory.at_mut(next_ptr).cast::<AtomicU32>() = AtomicU32::new(next_val);
            }
        }

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            elem_size,
            node_size,
            _marker: PhantomData,
        })
    }

    /// Open an existing pool from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the pool (must exist in the table)
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "pool '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;
        let elem_size = T::SIZE as u32;

        unsafe {
            let base = memory.at(offset);
            let stored_elem_size = std::ptr::read(base.add(POOL_ELEM_SIZE_OFFSET).cast::<u32>());

            if stored_elem_size != elem_size {
                return Err(ZeroShmError::SizeError(format!(
                    "pool element size mismatch: expected {}, found {}",
                    elem_size, stored_elem_size
                )));
            }

            let capacity = std::ptr::read(base.add(POOL_CAPACITY_OFFSET).cast::<u32>());
            let node_size = align8(align8(T::SIZE) + 4);

            Ok(Self {
                memory: Arc::new(memory.clone_shallow()),
                name: name.to_string(),
                offset,
                capacity,
                elem_size,
                node_size,
                _marker: PhantomData,
            })
        }
    }

    /// Returns a pointer to the node data at index i.
    fn node_data_ptr(&self, i: usize) -> *mut T {
        let ptr = self.offset + POOL_HEADER_SIZE + i * self.node_size;
        self.memory.at_mut(ptr).cast::<T>()
    }

    /// Returns a pointer to the next field of node i.
    fn node_next_ptr(&self, i: usize) -> *mut AtomicU32 {
        let ptr =
            self.offset + POOL_HEADER_SIZE + i * self.node_size + align8(T::SIZE);
        self.memory.at_mut(ptr).cast::<AtomicU32>()
    }

    /// Allocate an object from the pool (lock-free, ABA-safe).
    ///
    /// Returns `Some` with a mutable reference to the allocated slot's data,
    /// or `None` if the pool is full.
    pub fn allocate(&self) -> Option<PoolSlot<'_, T>> {
        loop {
            let old_head = unsafe {
                (*self.memory.at(self.offset).cast::<AtomicU64>()).load(Ordering::Acquire)
            };
            let free_index = unpack_index(old_head);
            let generation = unpack_generation(old_head);

            if free_index == NULL_INDEX {
                return None; // Pool is full
            }

            let next = unsafe { (*self.node_next_ptr(free_index as usize)).load(Ordering::Relaxed) };

            // Pack new head with bumped generation to prevent ABA
            let new_head = pack_tagged(next, generation + 1);

            if unsafe {
                (*self.memory.at(self.offset).cast::<AtomicU64>())
                    .compare_exchange_weak(old_head, new_head, Ordering::Release, Ordering::Acquire)
                    .is_ok()
            } {
                // Success - we got the node
                unsafe {
                    (*self.memory.at(self.offset + 8).cast::<AtomicU32>())
                        .fetch_add(1, Ordering::Relaxed);
                }
                return Some(PoolSlot {
                    pool: self,
                    index: free_index as usize,
                });
            }
        }
    }

    /// Get number of allocated objects.
    pub fn allocated(&self) -> usize {
        unsafe {
            (*self.memory.at(self.offset + 8).cast::<AtomicU32>())
                .load(Ordering::Relaxed) as usize
        }
    }

    /// Get number of free objects.
    pub fn available(&self) -> usize {
        self.capacity as usize - self.allocated()
    }

    /// Get pool capacity.
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Check if pool is empty (all objects are free).
    pub fn is_empty(&self) -> bool {
        self.allocated() == 0
    }

    /// Check if pool is full (no free objects).
    pub fn is_full(&self) -> bool {
        self.allocated() == self.capacity as usize
    }
}

/// A slot allocated from a pool. Automatically returns the slot to the pool
/// when dropped.
pub struct PoolSlot<'p, T: Pod> {
    pool: &'p Pool<T>,
    index: usize,
}

impl<'p, T: Pod> PoolSlot<'p, T> {
    /// Get a mutable reference to the slot's data.
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.pool.node_data_ptr(self.index) }
    }

    /// Get a shared reference to the slot's data.
    pub fn get(&self) -> &T {
        unsafe { &*self.pool.node_data_ptr(self.index) }
    }
}

impl<'p, T: Pod> Drop for PoolSlot<'p, T> {
    fn drop(&mut self) {
        // Deallocate: push this node back onto the free list
        let node_next = self.pool.node_next_ptr(self.index);

        loop {
            let old_head = unsafe {
                (*self.pool.memory.at(self.pool.offset).cast::<AtomicU64>())
                    .load(Ordering::Acquire)
            };
            let old_index = unpack_index(old_head);
            let generation = unpack_generation(old_head);

            // Point this node's next to the current head
            unsafe {
                (*node_next).store(old_index, Ordering::Relaxed);
            }

            // Pack new head with bumped generation to prevent ABA
            let new_head = pack_tagged(self.index as u32, generation + 1);

            if unsafe {
                (*self.pool.memory.at(self.pool.offset).cast::<AtomicU64>())
                    .compare_exchange_weak(old_head, new_head, Ordering::Release, Ordering::Acquire)
                    .is_ok()
            } {
                break;
            }
        }

        // Decrement allocated count
        unsafe {
            (*self.pool.memory.at(self.pool.offset + 8).cast::<AtomicU32>())
                .fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_allocate_and_deallocate() {
        let name = "/test_pool_alloc";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let pool = Pool::<i64>::new(&memory, "pool_test", 8).unwrap();

        // Allocate some slots
        let mut slot1 = pool.allocate().unwrap();
        *slot1.get_mut() = 42;
        assert_eq!(pool.allocated(), 1);
        assert_eq!(pool.available(), 7);

        let mut slot2 = pool.allocate().unwrap();
        *slot2.get_mut() = 100;
        assert_eq!(pool.allocated(), 2);
        assert_eq!(pool.available(), 6);

        // Verify values
        assert_eq!(*slot1.get(), 42);
        assert_eq!(*slot2.get(), 100);

        // Drop slots - they should be returned to the pool
        drop(slot1);
        assert_eq!(pool.allocated(), 1);
        assert_eq!(pool.available(), 7);

        drop(slot2);
        assert_eq!(pool.allocated(), 0);
        assert_eq!(pool.available(), 8);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_pool_full() {
        let name = "/test_pool_full";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let pool = Pool::<i64>::new(&memory, "pool_full", 4).unwrap();

        let mut slots = Vec::new();
        for _ in 0..4 {
            slots.push(pool.allocate().unwrap());
        }

        // Pool should be full now
        assert!(pool.is_full());
        assert!(!pool.is_empty());
        assert!(pool.allocate().is_none());

        // Drop one slot, should be able to allocate again
        slots.pop().unwrap();
        assert!(!pool.is_full());
        assert!(pool.allocate().is_some());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_pool_concurrent_allocate_deallocate() {
        let name = "/test_pool_conc";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let pool = Arc::new(Pool::<i64>::new(&memory, "pool_conc", 64).unwrap());

        use std::sync::mpsc;
        use std::thread;

        let (tx, rx) = mpsc::channel();
        for _ in 0..4 {
            let pool = Arc::clone(&pool);
            let tx = tx.clone();
            thread::spawn(move || {
                for _ in 0..100 {
                    if let Some(mut slot) = pool.allocate() {
                        *slot.get_mut() = 42;
                        drop(slot); // deallocate
                    }
                }
                tx.send(()).unwrap();
            });
        }

        drop(tx);
        for _ in 0..4 {
            rx.recv().unwrap();
        }

        // After all threads finish, pool should be empty
        assert_eq!(pool.allocated(), 0);
        assert!(pool.is_empty());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_pool_open() {
        let name = "/test_pool_open";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let pool = Pool::<i64>::new(&memory, "pool_open", 8).unwrap();
            assert!(pool.allocate().is_some());
            // Slot is dropped here, returned to pool
        }

        let pool = Pool::<i64>::open(&memory, "pool_open").unwrap();
        assert_eq!(pool.allocated(), 0);
        assert!(pool.allocate().is_some());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_pool_open_not_found() {
        let name = "/test_pool_notfound";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Pool::<i64>::open(&memory, "nonexistent");
        assert!(matches!(result, Err(ZeroShmError::NotFoundError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_pool_zero_capacity_error() {
        let name = "/test_pool_zero";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Pool::<i64>::new(&memory, "pool_zero", 0);
        assert!(matches!(result, Err(ZeroShmError::SizeError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_pool_i32_elements() {
        let name = "/test_pool_i32";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let pool = Pool::<i32>::new(&memory, "pool_i32", 8).unwrap();

        let mut slot = pool.allocate().unwrap();
        *slot.get_mut() = -100;
        assert_eq!(*slot.get(), -100);
        drop(slot);

        assert_eq!(pool.allocated(), 0);

        memory.unlink().unwrap();
        drop(memory);
    }
}

