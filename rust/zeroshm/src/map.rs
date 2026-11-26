//! Lock-free hash map with linear probing.
//!
//! Provides the `Map<K, V>` type for lock-free hash maps stored in shared memory.
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
//! - key_size: u32 at offset 8
//! - value_size: u32 at offset 12
//!
//! Entry layout:
//! - state: u32 at offset 0 (atomic: 0=empty, 1=occupied, 2=deleted, 3=inserting)
//! - key: K (raw bytes)
//! - value: V (raw bytes)
//!
//! # Constraints
//! - Key must be trivially copyable (Pod)
//! - Value must be trivially copyable (Pod)
//! - Value must be <= 8 bytes (for the 4-state protocol to work correctly)

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::{align8, MAP_CAPACITY_OFFSET, MAP_KEY_SIZE_OFFSET, MAP_VALUE_SIZE_OFFSET};
use crate::memory::Memory;
#[cfg(test)]
use crate::platform;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Size of the map header in bytes.
const MAP_HEADER_SIZE: usize = 16;

/// Slot state: empty (no data, chain terminator).
const SLOT_EMPTY: u32 = 0;
/// Slot state: occupied (key and value are valid).
const SLOT_OCCUPIED: u32 = 1;
/// Slot state: deleted (slot was occupied, now reusable but not a chain terminator).
const SLOT_DELETED: u32 = 2;
/// Slot state: inserting (a thread is writing to this slot).
const SLOT_INSERTING: u32 = 3;

/// Bound on spin loops waiting for a slot stuck in INSERTING.
const MAX_SPINS: usize = 10000;

/// Hash function for Pod types in shared memory.
/// Uses multiplicative hash for integers, byte-level hash otherwise.
pub(crate) fn trivial_hash<T: Pod>(val: &T) -> u64 {
    unsafe {
        let bytes = val as *const T as *const u8;
        let size = T::SIZE;
        if size == 1 {
            let v = *bytes as u64;
            return v.wrapping_mul(2654435761);
        } else if size == 2 {
            let v = u16::from_ne_bytes([*bytes, *bytes.add(1)]) as u64;
            return v.wrapping_mul(2654435761);
        } else if size == 4 {
            let v = u32::from_ne_bytes([
                *bytes,
                *bytes.add(1),
                *bytes.add(2),
                *bytes.add(3),
            ]) as u64;
            return v.wrapping_mul(2654435761);
        } else if size == 8 {
            let v = u64::from_ne_bytes([
                *bytes,
                *bytes.add(1),
                *bytes.add(2),
                *bytes.add(3),
                *bytes.add(4),
                *bytes.add(5),
                *bytes.add(6),
                *bytes.add(7),
            ]);
            return v.wrapping_mul(2654435761);
        } else {
            // For larger types, use a simple FNV-style hash over bytes
            let mut hash: u64 = 0xcbf29ce484222325;
            for i in 0..size {
                hash ^= *bytes.add(i) as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash
        }
    }
}

/// A lock-free hash map stored in shared memory.
///
/// Uses linear probing with a 4-state per-slot protocol for concurrent access.
pub struct Map<K: Pod, V: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: u32,
    key_size: u32,
    value_size: u32,
    entry_size: u32,
    _marker: PhantomData<(K, V)>,
}

impl<K: Pod, V: Pod> std::fmt::Debug for Map<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Map")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("key_size", &self.key_size)
            .field("value_size", &self.value_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Map holds an Arc<Memory> which is Send + Sync, and K/V: Pod
// guarantees the types are safe for concurrent access via raw pointers.
// The 4-state CAS protocol uses atomics for synchronization.
unsafe impl<K: Pod, V: Pod> Send for Map<K, V> {}
unsafe impl<K: Pod, V: Pod> Sync for Map<K, V> {}

impl<K: Pod, V: Pod> Map<K, V> {
    /// Create a new map in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the map (stored in the table)
    /// * `capacity` -- Number of slots
    pub fn new(memory: &Memory, name: &str, capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ZeroShmError::SizeError(
                "capacity must be greater than zero".to_string(),
            ));
        }

        if V::SIZE > 8 {
            return Err(ZeroShmError::SizeError(
                "value size must be <= 8 bytes".to_string(),
            ));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let capacity = capacity as u32;
        let key_size = K::SIZE as u32;
        let value_size = V::SIZE as u32;

        // Entry: state(u32) at 0, key at 8 (8-byte aligned), value at 8+align8(key_size)
        // Total entry size is aligned to 8 bytes so each entry starts 8-byte aligned
        let entry_size = align8(4 + key_size as usize + value_size as usize) as u32;

        // Check for overflow
        if capacity as usize > (usize::MAX - MAP_HEADER_SIZE) / entry_size as usize {
            return Err(ZeroShmError::SizeError("capacity too large".to_string()));
        }

        let total_size = MAP_HEADER_SIZE + entry_size as usize * capacity as usize;

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        // Write header: size=0, capacity, key_size, value_size
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicU32>()).store(0, Ordering::Relaxed); // size
            base.add(MAP_CAPACITY_OFFSET).cast::<AtomicU32>().as_ref().unwrap().store(capacity, Ordering::Relaxed); // capacity
            std::ptr::write(base.add(MAP_KEY_SIZE_OFFSET).cast::<u32>(), key_size); // key_size
            std::ptr::write(base.add(MAP_VALUE_SIZE_OFFSET).cast::<u32>(), value_size); // value_size
        }

        // Initialize all entries as empty
        unsafe {
            for i in 0..capacity as usize {
                let entry_state =
                    offset + MAP_HEADER_SIZE + i * entry_size as usize;
                *memory.at_mut(entry_state).cast::<AtomicU32>() = AtomicU32::new(SLOT_EMPTY);
            }
        }

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            key_size,
            value_size,
            entry_size,
            _marker: PhantomData,
        })
    }

    /// Open an existing map from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the map (must exist in the table)
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "map '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;
        let key_size = K::SIZE as u32;
        let value_size = V::SIZE as u32;

        unsafe {
            let base = memory.at(offset);
            let stored_key_size = std::ptr::read(base.add(MAP_KEY_SIZE_OFFSET).cast::<u32>());
            let stored_value_size = std::ptr::read(base.add(MAP_VALUE_SIZE_OFFSET).cast::<u32>());

            if stored_key_size != key_size || stored_value_size != value_size {
                return Err(ZeroShmError::SizeError(format!(
                    "map type size mismatch: expected key={} value={}, found key={} value={}",
                    key_size, value_size, stored_key_size, stored_value_size
                )));
            }

            let capacity = std::ptr::read(base.add(MAP_CAPACITY_OFFSET).cast::<u32>());
            let entry_size = align8(4 + key_size as usize + value_size as usize) as u32;

            Ok(Self {
                memory: Arc::new(memory.clone_shallow()),
                name: name.to_string(),
                offset,
                capacity,
                key_size,
                value_size,
                entry_size,
                _marker: PhantomData,
            })
        }
    }

    /// Returns a pointer to the state of entry i.
    fn entry_state_ptr(&self, i: usize) -> *const AtomicU32 {
        let ptr = self.offset + MAP_HEADER_SIZE + i * self.entry_size as usize;
        self.memory.at(ptr).cast::<AtomicU32>()
    }

    /// Returns a mutable pointer to the state of entry i.
    fn entry_state_ptr_mut(&self, i: usize) -> *mut AtomicU32 {
        let ptr = self.offset + MAP_HEADER_SIZE + i * self.entry_size as usize;
        self.memory.at_mut(ptr).cast::<AtomicU32>()
    }

    /// Returns a pointer to the key of entry i (at offset 8 within entry).
    fn entry_key_ptr(&self, i: usize) -> *const K {
        let ptr = self.offset + MAP_HEADER_SIZE + i * self.entry_size as usize + 8;
        self.memory.at(ptr).cast::<K>()
    }

    /// Returns a mutable pointer to the key of entry i.
    fn entry_key_ptr_mut(&self, i: usize) -> *mut K {
        let ptr = self.offset + MAP_HEADER_SIZE + i * self.entry_size as usize + 8;
        self.memory.at_mut(ptr).cast::<K>()
    }

    /// Returns a pointer to the value of entry i (at offset 8+align8(key_size) within entry).
    fn entry_value_ptr(&self, i: usize) -> *const V {
        let ptr = self.offset
            + MAP_HEADER_SIZE
            + i * self.entry_size as usize
            + 8
            + align8(K::SIZE);
        self.memory.at(ptr).cast::<V>()
    }

    /// Returns a mutable pointer to the value of entry i.
    fn entry_value_ptr_mut(&self, i: usize) -> *mut V {
        let ptr = self.offset
            + MAP_HEADER_SIZE
            + i * self.entry_size as usize
            + 8
            + align8(K::SIZE);
        self.memory.at_mut(ptr).cast::<V>()
    }

    /// Insert or update a key-value pair (lock-free with linear probing).
    ///
    /// Returns `true` if the key was inserted or updated, `false` if the map is full.
    pub fn insert(&self, key: &K, value: &V) -> bool {
        let hash = trivial_hash(key);
        let capacity = self.capacity as usize;

        // Two-phase insert. Phase 1 scans the probe chain for the key
        // (updating in place if found), remembering the first reusable
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
                        let entry_key = unsafe { &*self.entry_key_ptr(idx) };
                        if !trivial_equal_raw(entry_key, key) {
                            break; // next slot
                        }

                        // Update: CAS OCCUPIED -> INSERTING for exclusive access
                        if unsafe {
                            (*state_ptr)
                                .compare_exchange(SLOT_OCCUPIED, SLOT_INSERTING, Ordering::AcqRel, Ordering::Acquire)
                                .is_ok()
                        } {
                            // We exclusively own this slot; update value
                            unsafe {
                                *self.entry_value_ptr_mut(idx) = *value;
                            }
                            // Publish: INSERTING -> OCCUPIED
                            unsafe {
                                (*state_ptr).store(SLOT_OCCUPIED, Ordering::Release);
                            }
                            return true;
                        }
                        // Lost CAS: re-examine
                        continue;
                    }

                    if state == SLOT_DELETED {
                        if deleted_target.is_none() {
                            deleted_target = Some(idx);
                        }
                        break; // keep scanning: the key may live further on
                    }

                    // EMPTY: the probe chain ends here
                    empty_target = Some(idx);
                    break;
                }
            }

            let target = deleted_target.or(empty_target);
            let Some(target) = target else {
                break; // map is full
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
                // We exclusively own this slot; write key and value
                unsafe {
                    *self.entry_key_ptr_mut(target) = *key;
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

        false // Map is full
    }

    /// Find a value by key.
    ///
    /// Returns `Some(value)` if found, `None` otherwise.
    pub fn get(&self, key: &K) -> Option<V> {
        let hash = trivial_hash(key);
        let capacity = self.capacity as usize;

        for i in 0..capacity {
            let idx = ((hash as usize) + i) % capacity;
            let state_ptr = self.entry_state_ptr(idx);

            let mut spins = 0;
            loop {
                let state = unsafe { (*state_ptr).load(Ordering::Acquire) };

                if state == SLOT_EMPTY {
                    return None; // chain ends; key not found
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
                    let entry_key = unsafe { &*self.entry_key_ptr(idx) };
                    if trivial_equal_raw(entry_key, key) {
                        let value = unsafe { *self.entry_value_ptr(idx) };
                        return Some(value);
                    }
                }

                break; // DELETED or different key: next slot
            }
        }

        None
    }

    /// Remove a key (mark as deleted).
    ///
    /// Returns `true` if the key was removed, `false` if not found.
    pub fn remove(&self, key: &K) -> bool {
        let hash = trivial_hash(key);
        let capacity = self.capacity as usize;

        for i in 0..capacity {
            let idx = ((hash as usize) + i) % capacity;
            let state_ptr = self.entry_state_ptr(idx);

            let mut spins = 0;
            loop {
                let state = unsafe { (*state_ptr).load(Ordering::Acquire) };

                if state == SLOT_EMPTY {
                    return false; // chain ends; key not found
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
                    let entry_key = unsafe { &*self.entry_key_ptr(idx) };
                    if trivial_equal_raw(entry_key, key) {
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

                break; // DELETED or different key: next slot
            }
        }

        false
    }

    /// Check if a key exists.
    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
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

    /// Check if the map is empty.
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
    fn test_map_insert_and_get() {
        let name = "/test_map_insert";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let map = Map::<u64, i64>::new(&memory, "map_test", 1024).unwrap();

        // Insert key-value pairs
        assert!(map.insert(&1, &100));
        assert!(map.insert(&2, &200));
        assert!(map.insert(&3, &300));

        // Get values
        assert_eq!(map.get(&1), Some(100));
        assert_eq!(map.get(&2), Some(200));
        assert_eq!(map.get(&3), Some(300));
        assert_eq!(map.get(&999), None);

        // Size
        assert_eq!(map.len(), 3);
        assert!(!map.is_empty());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_update_existing_key() {
        let name = "/test_map_update";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let map = Map::<u64, i64>::new(&memory, "map_update", 1024).unwrap();

        map.insert(&1, &100);
        assert!(map.insert(&1, &200)); // Update

        assert_eq!(map.get(&1), Some(200));
        assert_eq!(map.len(), 1); // Size should not change on update

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_remove() {
        let name = "/test_map_remove";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let map = Map::<u64, i64>::new(&memory, "map_remove", 1024).unwrap();

        map.insert(&1, &100);
        map.insert(&2, &200);
        map.insert(&3, &300);

        assert!(map.remove(&2));
        assert_eq!(map.get(&2), None);
        assert_eq!(map.len(), 2);
        assert!(map.contains(&1));
        assert!(map.contains(&3));
        assert!(!map.contains(&2));

        // Remove non-existent
        assert!(!map.remove(&999));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_contains() {
        let name = "/test_map_contains";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let map = Map::<u64, i64>::new(&memory, "map_contains", 1024).unwrap();

        assert!(!map.contains(&1));
        map.insert(&1, &100);
        assert!(map.contains(&1));
        assert!(!map.contains(&2));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_many_inserts() {
        let name = "/test_map_many";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let map = Map::<u64, i64>::new(&memory, "map_many", 4096).unwrap();

        // Insert 1000 key-value pairs
        for i in 0..1000u64 {
            assert!(map.insert(&i, &(i as i64 * 10)));
        }

        // Verify all retrievable
        for i in 0..1000u64 {
            assert_eq!(map.get(&i), Some(i as i64 * 10));
        }

        assert_eq!(map.len(), 1000);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_open() {
        let name = "/test_map_open";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let map = Map::<u64, i64>::new(&memory, "map_open", 1024).unwrap();
            map.insert(&42, &1234);
        }

        let map = Map::<u64, i64>::open(&memory, "map_open").unwrap();
        assert_eq!(map.get(&42), Some(1234));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_open_not_found() {
        let name = "/test_map_notfound";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Map::<u64, i64>::open(&memory, "nonexistent");
        assert!(matches!(result, Err(ZeroShmError::NotFoundError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_clear() {
        let name = "/test_map_clear";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let map = Map::<u64, i64>::new(&memory, "map_clear", 1024).unwrap();

        map.insert(&1, &100);
        map.insert(&2, &200);
        map.clear();

        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), None);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_zero_capacity_error() {
        let name = "/test_map_zero";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Map::<u64, i64>::new(&memory, "map_zero", 0);
        assert!(matches!(result, Err(ZeroShmError::SizeError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_map_too_large_value_error() {
        #[derive(Clone, Copy)]
        #[allow(dead_code)]
        struct BigValue {
            a: i64,
            b: i64,
            c: i64,
        }

        // SAFETY: BigValue is a plain struct with only i64 fields, trivially copyable.
        unsafe impl Pod for BigValue {
            const SIZE: usize = 24;
            const ALIGN: usize = 8;
        }

        let name = "/test_map_bigval";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Map::<u64, BigValue>::new(&memory, "map_bigval", 1024);
        assert!(matches!(result, Err(ZeroShmError::SizeError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }
}

