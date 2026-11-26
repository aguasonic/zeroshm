//! Lock-free hash set with linear probing.
//!
//! Provides the `Set<T>` type for lock-free hash sets stored in shared memory.
//! Uses a 4-state per-slot protocol (empty/occupied/deleted/inserting) with
//! bounded spin loops for crash safety.
//!
//! # Binary Layout
//! ```text
//! [header: 16 bytes][entries: Entry * capacity]
//! ```
//!
//! Header layout (little-endian):
//! - size: u32 at offset 0 (current number of elements, atomic)
//! - capacity: u32 at offset 4 (total number of slots)
//! - elem_size: u32 at offset 8
//! - reserved: u32 at offset 12 (pads header to 16 bytes)
//!
//! Entry layout:
//! - state: u32 at offset 0 (atomic: 0=empty, 1=occupied, 2=deleted, 3=inserting)
//! - value: T (raw bytes)

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::{align8, SET_CAPACITY_OFFSET, SET_ELEM_SIZE_OFFSET, SET_RESERVED_OFFSET};
use crate::map::trivial_hash;
use crate::memory::Memory;
#[cfg(test)]
use crate::platform;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Size of the set header in bytes.
const SET_HEADER_SIZE: usize = 16;

/// Slot state: empty (no data, chain terminator).
const SLOT_EMPTY: u32 = 0;
/// Slot state: occupied (value is valid).
const SLOT_OCCUPIED: u32 = 1;
/// Slot state: deleted (slot was occupied, now reusable but not a chain terminator).
const SLOT_DELETED: u32 = 2;
/// Slot state: inserting (a thread is writing to this slot).
const SLOT_INSERTING: u32 = 3;

/// Bound on spin loops waiting for a slot stuck in INSERTING.
const MAX_SPINS: usize = 10000;

/// A lock-free hash set stored in shared memory.
///
/// Uses linear probing with a 4-state per-slot protocol for concurrent access.
pub struct Set<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: u32,
    elem_size: u32,
    entry_size: u32,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Set<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Set")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("elem_size", &self.elem_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Set holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the type is safe for concurrent access via raw pointers.
// The 4-state CAS protocol uses atomics for synchronization.
unsafe impl<T: Pod> Send for Set<T> {}
unsafe impl<T: Pod> Sync for Set<T> {}

impl<T: Pod> Set<T> {
    /// Create a new set in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the set (stored in the table)
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

        // Entry: state(u32) at 0, value at 8 (8-byte aligned)
        // Total entry size is aligned to 8 bytes so each entry starts 8-byte aligned
        let entry_size = align8(4 + T::SIZE) as u32;

        // Check for overflow
        if capacity as usize > (usize::MAX - SET_HEADER_SIZE) / entry_size as usize {
            return Err(ZeroShmError::SizeError("capacity too large".to_string()));
        }

        let total_size = SET_HEADER_SIZE + entry_size as usize * capacity as usize;

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        // Write header: size=0, capacity, elem_size, reserved=0
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicU32>()).store(0, Ordering::Relaxed); // size
            base.add(SET_CAPACITY_OFFSET).cast::<AtomicU32>().as_ref().unwrap().store(capacity, Ordering::Relaxed); // capacity
            std::ptr::write(base.add(SET_ELEM_SIZE_OFFSET).cast::<u32>(), elem_size); // elem_size
            std::ptr::write(base.add(SET_RESERVED_OFFSET).cast::<u32>(), 0); // reserved
        }

        // Initialize all entries as empty
        unsafe {
            for i in 0..capacity as usize {
                let entry_state =
                    offset + SET_HEADER_SIZE + i * entry_size as usize;
                *memory.at_mut(entry_state).cast::<AtomicU32>() = AtomicU32::new(SLOT_EMPTY);
            }
        }

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            elem_size,
            entry_size,
            _marker: PhantomData,
        })
    }

    /// Open an existing set from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the set (must exist in the table)
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "set '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;
        let elem_size = T::SIZE as u32;

        unsafe {
            let base = memory.at(offset);
            let stored_elem_size = std::ptr::read(base.add(SET_ELEM_SIZE_OFFSET).cast::<u32>());

            if stored_elem_size != elem_size {
                return Err(ZeroShmError::SizeError(format!(
                    "set element size mismatch: expected {}, found {}",
                    elem_size, stored_elem_size
                )));
            }

            let capacity = std::ptr::read(base.add(SET_CAPACITY_OFFSET).cast::<u32>());
            let entry_size = align8(4 + T::SIZE) as u32;

            Ok(Self {
                memory: Arc::new(memory.clone_shallow()),
                name: name.to_string(),
                offset,
                capacity,
                elem_size,
                entry_size,
                _marker: PhantomData,
            })
        }
    }

    /// Returns a pointer to the state of entry i.
    fn entry_state_ptr(&self, i: usize) -> *const AtomicU32 {
        let ptr = self.offset + SET_HEADER_SIZE + i * self.entry_size as usize;
        self.memory.at(ptr).cast::<AtomicU32>()
    }

    /// Returns a mutable pointer to the state of entry i.
    fn entry_state_ptr_mut(&self, i: usize) -> *mut AtomicU32 {
        let ptr = self.offset + SET_HEADER_SIZE + i * self.entry_size as usize;
        self.memory.at_mut(ptr).cast::<AtomicU32>()
    }

    /// Returns a pointer to the value of entry i (at offset 8 within entry).
    fn entry_value_ptr(&self, i: usize) -> *const T {
        let ptr = self.offset + SET_HEADER_SIZE + i * self.entry_size as usize + 8;
        self.memory.at(ptr).cast::<T>()
    }

    /// Returns a mutable pointer to the value of entry i.
    fn entry_value_ptr_mut(&self, i: usize) -> *mut T {
        let ptr = self.offset + SET_HEADER_SIZE + i * self.entry_size as usize + 8;
        self.memory.at_mut(ptr).cast::<T>()
    }

    /// Insert an element into the set (lock-free with linear probing).
    ///
    /// Returns `true` if the element was inserted, `false` if it already
    /// exists or the set is full.
    pub fn insert(&self, value: &T) -> bool {
        let hash = trivial_hash(value);
        let capacity = self.capacity as usize;

        // Two-phase insert. Phase 1 scans the probe chain for the value
        // (returning false if present), remembering the first reusable
        // DELETED slot; the chain ends at the first EMPTY slot.
        // Phase 2 claims the remembered slot.
        loop {
            let mut deleted_target: Option<usize> = None;
            let mut empty_target: Option<usize> = None;

            for i in 0..capacity {
                if empty_target.is_some() {
                    break;
                }
                let idx = ((hash as usize) + i) % capacity;
                let state_ptr = self.entry_state_ptr(idx);

                let mut spins = 0;
                loop {
                    let state = unsafe { (*state_ptr).load(Ordering::Acquire) };

                    if state == SLOT_INSERTING {
                        // Mid-write by another thread; wait bounded, then skip
                        if spins >= MAX_SPINS {
                            break;
                        }
                        spins += 1;
                        std::thread::yield_now();
                        continue;
                    }

                    if state == SLOT_OCCUPIED {
                        let entry_val = unsafe { &*self.entry_value_ptr(idx) };
                        if !trivial_equal_raw(entry_val, value) {
                            break; // next slot
                        }

                        // Value already exists
                        return false;
                    }

                    if state == SLOT_DELETED {
                        if deleted_target.is_none() {
                            deleted_target = Some(idx);
                        }
                        break; // keep scanning: the value may live further on
                    }

                    // EMPTY: the probe chain ends here
                    empty_target = Some(idx);
                    break;
                }
            }

            let target = deleted_target.or(empty_target);
            let Some(target) = target else {
                break; // set is full
            };

            let expected = if deleted_target.is_some() {
                SLOT_DELETED
            } else {
                SLOT_EMPTY
            };

            if unsafe {
                (*self.entry_state_ptr_mut(target))
                    .compare_exchange(expected, SLOT_INSERTING, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            } {
                // We exclusively own this slot; write value
                unsafe {
                    *self.entry_value_ptr_mut(target) = *value;
                }
                // Publish: INSERTING -> OCCUPIED
                unsafe {
                    (*self.entry_state_ptr(target)).store(SLOT_OCCUPIED, Ordering::Release);
                }
                // Increment size
                unsafe {
                    (*self.memory.at(self.offset).cast::<AtomicU32>())
                        .fetch_add(1, Ordering::Relaxed);
                }
                return true;
            }
            // Lost CAS: rescan from the top
        }

        false // Set is full
    }

    /// Check if an element is in the set.
    ///
    /// Returns `true` if the element exists, `false` otherwise.
    pub fn contains(&self, value: &T) -> bool {
        let hash = trivial_hash(value);
        let capacity = self.capacity as usize;

        for i in 0..capacity {
            let idx = ((hash as usize) + i) % capacity;
            let state_ptr = self.entry_state_ptr(idx);

            let mut spins = 0;
            loop {
                let state = unsafe { (*state_ptr).load(Ordering::Acquire) };

                if state == SLOT_EMPTY {
                    return false; // chain ends; element not found
                }

                if state == SLOT_INSERTING {
                    // Mid-write; wait bounded, then skip
                    if spins >= MAX_SPINS {
                        break;
                    }
                    spins += 1;
                    std::thread::yield_now();
                    continue;
                }

                if state == SLOT_OCCUPIED {
                    let entry_val = unsafe { &*self.entry_value_ptr(idx) };
                    if trivial_equal_raw(entry_val, value) {
                        return true;
                    }
                }

                break; // DELETED or different value: next slot
            }
        }

        false
    }

    /// Remove an element from the set (mark as deleted).
    ///
    /// Returns `true` if the element was removed, `false` if not found.
    pub fn remove(&self, value: &T) -> bool {
        let hash = trivial_hash(value);
        let capacity = self.capacity as usize;

        for i in 0..capacity {
            let idx = ((hash as usize) + i) % capacity;
            let state_ptr = self.entry_state_ptr(idx);

            let mut spins = 0;
            loop {
                let state = unsafe { (*state_ptr).load(Ordering::Acquire) };

                if state == SLOT_EMPTY {
                    return false; // chain ends; element not found
                }

                if state == SLOT_INSERTING {
                    // Mid-write; wait bounded, then skip
                    if spins >= MAX_SPINS {
                        break;
                    }
                    spins += 1;
                    std::thread::yield_now();
                    continue;
                }

                if state == SLOT_OCCUPIED {
                    let entry_val = unsafe { &*self.entry_value_ptr(idx) };
                    if trivial_equal_raw(entry_val, value) {
                        // CAS from OCCUPIED to DELETED
                        if unsafe {
                            (*state_ptr)
                                .compare_exchange(SLOT_OCCUPIED, SLOT_DELETED, Ordering::Release, Ordering::Acquire)
                                .is_ok()
                        } {
                            unsafe {
                                (*self.memory.at(self.offset).cast::<AtomicU32>())
                                    .fetch_sub(1, Ordering::Relaxed);
                            }
                            return true;
                        }
                        // Lost CAS: re-examine
                        continue;
                    }
                }

                break; // DELETED or different value: next slot
            }
        }

        false
    }

    /// Get current size (number of occupied slots).
    pub fn len(&self) -> usize {
        unsafe {
            (*self.memory.at(self.offset).cast::<AtomicU32>()).load(Ordering::Relaxed)
                as usize
        }
    }

    /// Get capacity (number of slots).
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Check if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all entries (not thread-safe with concurrent operations).
    pub fn clear(&self) {
        unsafe {
            for i in 0..self.capacity as usize {
                (*self.entry_state_ptr_mut(i)).store(SLOT_EMPTY, Ordering::Relaxed);
            }
            (*self.memory.at(self.offset).cast::<AtomicU32>()).store(0, Ordering::Relaxed);
        }
    }
}

/// Compare two Pod values by their raw bytes (memcmp equivalent).
fn trivial_equal_raw<T: Pod>(a: &T, b: &T) -> bool {
    unsafe {
        std::ptr::addr_eq(a, b)
            || std::slice::from_raw_parts(a as *const T as *const u8, T::SIZE)
                == std::slice::from_raw_parts(b as *const T as *const u8, T::SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_insert_and_contains() {
        let name = "/test_set_insert";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let set = Set::<u64>::new(&memory, "set_test", 1024).unwrap();

        // Insert elements
        assert!(set.insert(&1));
        assert!(set.insert(&2));
        assert!(set.insert(&3));

        // Contains
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
        assert!(!set.contains(&999));

        // Size
        assert_eq!(set.len(), 3);
        assert!(!set.is_empty());

        // Duplicate insert should return false
        assert!(!set.insert(&1));
        assert_eq!(set.len(), 3);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_remove() {
        let name = "/test_set_remove";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let set = Set::<u64>::new(&memory, "set_remove", 1024).unwrap();

        set.insert(&1);
        set.insert(&2);
        set.insert(&3);

        assert!(set.remove(&2));
        assert!(!set.contains(&2));
        assert_eq!(set.len(), 2);
        assert!(set.contains(&1));
        assert!(set.contains(&3));

        // Remove non-existent
        assert!(!set.remove(&999));

        // Can re-insert after removal
        assert!(set.insert(&2));
        assert_eq!(set.len(), 3);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_many_inserts() {
        let name = "/test_set_many";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let set = Set::<u64>::new(&memory, "set_many", 4096).unwrap();

        // Insert 1000 elements
        for i in 0..1000u64 {
            assert!(set.insert(&i));
        }

        // Verify all present
        for i in 0..1000u64 {
            assert!(set.contains(&i));
        }

        assert_eq!(set.len(), 1000);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_open() {
        let name = "/test_set_open";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let set = Set::<u64>::new(&memory, "set_open", 1024).unwrap();
            set.insert(&42);
            set.insert(&100);
        }

        let set = Set::<u64>::open(&memory, "set_open").unwrap();
        assert!(set.contains(&42));
        assert!(set.contains(&100));
        assert!(!set.contains(&999));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_open_not_found() {
        let name = "/test_set_notfound";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Set::<u64>::open(&memory, "nonexistent");
        assert!(matches!(result, Err(ZeroShmError::NotFoundError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_clear() {
        let name = "/test_set_clear";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let set = Set::<u64>::new(&memory, "set_clear", 1024).unwrap();

        set.insert(&1);
        set.insert(&2);
        set.clear();

        assert_eq!(set.len(), 0);
        assert!(set.is_empty());
        assert!(!set.contains(&1));
        assert!(!set.contains(&2));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_zero_capacity_error() {
        let name = "/test_set_zero";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Set::<u64>::new(&memory, "set_zero", 0);
        assert!(matches!(result, Err(ZeroShmError::SizeError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_set_i32_elements() {
        let name = "/test_set_i32";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let set = Set::<i32>::new(&memory, "set_i32", 1024).unwrap();

        assert!(set.insert(&-100));
        assert!(set.insert(&0));
        assert!(set.insert(&100));

        assert!(set.contains(&-100));
        assert!(set.contains(&0));
        assert!(set.contains(&100));
        assert!(!set.contains(&50));

        assert_eq!(set.len(), 3);

        memory.unlink().unwrap();
        drop(memory);
    }
}

