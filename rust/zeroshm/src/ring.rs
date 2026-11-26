//! Multiple-producer single-consumer ring buffer for streaming data.
//!
//! Provides the `Ring<T>` type for MPSC ring buffers stored in shared memory.
//! Uses byte-based positions with CAS for multiple producers and atomic
//! loads/stores for the single consumer.
//!
//! # Binary Layout
//! ```text
//! [header: 24 bytes][data: capacity bytes]
//! ```
//!
//! Header layout (little-endian):
//! - write_pos: u64 at offset 0 (total bytes written, monotonically increasing)
//! - read_pos: u64 at offset 8 (total bytes read, monotonically increasing)
//! - capacity: u32 at offset 16 (ring buffer capacity in bytes)
//! - elem_size: u32 at offset 20 (size of each element in bytes)

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::{RING_READ_POS_OFFSET, RING_CAPACITY_OFFSET, RING_ELEM_SIZE_OFFSET};
use crate::memory::Memory;
#[cfg(test)]
use crate::platform;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Size of the ring header in bytes.
const RING_HEADER_SIZE: usize = 24;

/// A multiple-producer single-consumer ring buffer stored in shared memory.
///
/// Uses byte-based write/read positions. Multiple producers can write safely
/// via CAS on the write position. Only one consumer is allowed (MPSC).
pub struct Ring<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: u32, // in bytes
    elem_size: u32,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Ring<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ring")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("elem_size", &self.elem_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Ring holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the element type is safe for concurrent access via raw pointers
// in process-shared memory. The SPSC protocol uses atomics for synchronization.
unsafe impl<T: Pod> Send for Ring<T> {}
unsafe impl<T: Pod> Sync for Ring<T> {}

impl<T: Pod> Ring<T> {
    /// Create a new ring buffer in shared memory.
    ///
    /// The capacity is rounded down to a multiple of the element size.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the ring (stored in the table)
    /// * `capacity` -- Number of elements
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

        let elem_size = T::SIZE as u32;

        // Capacity is in elements; convert to bytes
        let byte_capacity = capacity * T::SIZE;

        // Check for overflow
        if byte_capacity > usize::MAX - RING_HEADER_SIZE {
            return Err(ZeroShmError::SizeError("capacity too large".to_string()));
        }

        let total_size = RING_HEADER_SIZE + byte_capacity;

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        // Write header: write_pos=0, read_pos=0, capacity (bytes), elem_size
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicU64>()).store(0, Ordering::Relaxed); // write_pos
            base.add(RING_READ_POS_OFFSET).cast::<AtomicU64>().as_ref().unwrap().store(0, Ordering::Relaxed); // read_pos
            std::ptr::write(base.add(RING_CAPACITY_OFFSET).cast::<u32>(), byte_capacity as u32); // capacity
            std::ptr::write(base.add(RING_ELEM_SIZE_OFFSET).cast::<u32>(), elem_size); // elem_size
        }

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity: byte_capacity as u32,
            elem_size,
            _marker: PhantomData,
        })
    }

    /// Open an existing ring buffer from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the ring (must exist in the table)
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "ring '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;
        let elem_size = T::SIZE as u32;

        unsafe {
            let base = memory.at(offset);
            let stored_elem_size = std::ptr::read(base.add(RING_ELEM_SIZE_OFFSET).cast::<u32>());
            if stored_elem_size != elem_size {
                return Err(ZeroShmError::SizeError(format!(
                    "ring element size mismatch: expected {}, found {}",
                    elem_size, stored_elem_size
                )));
            }
            let capacity = std::ptr::read(base.add(RING_CAPACITY_OFFSET).cast::<u32>());

            Ok(Self {
                memory: Arc::new(memory.clone_shallow()),
                name: name.to_string(),
                offset,
                capacity,
                elem_size,
                _marker: PhantomData,
            })
        }
    }

    /// Write a single element to the ring buffer.
    ///
    /// Returns `true` if the element was written, `false` if the buffer is full.
    /// MPSC-safe: multiple producers can write concurrently via CAS.
    pub fn write(&self, value: &T) -> bool {
        unsafe {
            let base = self.memory.at_mut(self.offset);
            let write_pos_atomic = base.cast::<AtomicU64>();
            let read_pos_atomic = base
                .add(8)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap();

            loop {
                let write_pos = (*write_pos_atomic).load(Ordering::Relaxed);
                let read_pos = read_pos_atomic.load(Ordering::Acquire);

                // Check if there's space
                if write_pos - read_pos + T::SIZE as u64 > self.capacity as u64 {
                    return false; // Buffer full
                }

                let buffer = base.add(RING_HEADER_SIZE);
                let offset = (write_pos % self.capacity as u64) as usize;

                if offset + T::SIZE <= self.capacity as usize {
                    // Normal case - contiguous write
                    std::ptr::copy_nonoverlapping(
                        value as *const T as *const u8,
                        buffer.add(offset),
                        T::SIZE,
                    );
                } else {
                    // Wrap around case - split write
                    let first_part = self.capacity as usize - offset;
                    std::ptr::copy_nonoverlapping(
                        value as *const T as *const u8,
                        buffer.add(offset),
                        first_part,
                    );
                    std::ptr::copy_nonoverlapping(
                        (value as *const T as *const u8).add(first_part),
                        buffer,
                        T::SIZE - first_part,
                    );
                }

                // Try to claim the slot with CAS; retry if another producer won
                match (*write_pos_atomic).compare_exchange(
                    write_pos,
                    write_pos + T::SIZE as u64,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return true,
                    Err(_) => continue, // CAS failed, retry
                }
            }
        }
    }

    /// Read a single element from the ring buffer.
    ///
    /// Returns `Some(value)` if an element was read, `None` if the buffer is empty.
    /// SPSC only: exactly one consumer at a time.
    pub fn read(&self) -> Option<T> {
        unsafe {
            let base = self.memory.at(self.offset);
            let write_pos = (*base.cast::<AtomicU64>()).load(Ordering::Acquire);
            let read_pos = base
                .add(8)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap()
                .load(Ordering::Relaxed);

            // Check if there's data
            if read_pos + T::SIZE as u64 > write_pos {
                return None; // Buffer empty
            }

            let buffer = base.add(RING_HEADER_SIZE);
            let offset = (read_pos % self.capacity as u64) as usize;
            let mut value: T = std::mem::zeroed();

            if offset + T::SIZE <= self.capacity as usize {
                // Normal case - contiguous read
                std::ptr::copy_nonoverlapping(
                    buffer.add(offset),
                    &mut value as *mut T as *mut u8,
                    T::SIZE,
                );
            } else {
                // Wrap around case - split read
                let first_part = self.capacity as usize - offset;
                std::ptr::copy_nonoverlapping(
                    buffer.add(offset),
                    &mut value as *mut T as *mut u8,
                    first_part,
                );
                std::ptr::copy_nonoverlapping(
                    buffer as *mut u8,
                    (&mut value as *mut T as *mut u8).add(first_part),
                    T::SIZE - first_part,
                );
            }

            // Update read position
            base.add(RING_READ_POS_OFFSET)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap()
                .store(read_pos + T::SIZE as u64, Ordering::Release);
            Some(value)
        }
    }

    /// Write multiple elements (bulk operation, more efficient).
    ///
    /// Returns the number of elements actually written.
    /// MPSC-safe: multiple producers can write concurrently via CAS.
    pub fn write_bulk(&self, data: &[T]) -> usize {
        unsafe {
            let base = self.memory.at_mut(self.offset);
            let write_pos_atomic = base.cast::<AtomicU64>();
            let read_pos_atomic = base
                .add(8)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap();

            loop {
                let write_pos = (*write_pos_atomic).load(Ordering::Relaxed);
                let read_pos = read_pos_atomic.load(Ordering::Acquire);

                // Calculate how many we can write
                let available = self.capacity as u64 - (write_pos - read_pos);
                let to_write = (data.len() as u64).min(available / T::SIZE as u64) as usize;

                if to_write == 0 {
                    return 0;
                }

                let bytes_to_write = to_write * T::SIZE;
                let offset = (write_pos % self.capacity as u64) as usize;
                let buffer = base.add(RING_HEADER_SIZE);

                if offset + bytes_to_write <= self.capacity as usize {
                    // Normal case - contiguous write
                    std::ptr::copy_nonoverlapping(
                        data.as_ptr() as *const u8,
                        buffer.add(offset),
                        bytes_to_write,
                    );
                } else {
                    // Wrap around case - split write
                    let first_part = self.capacity as usize - offset;
                    std::ptr::copy_nonoverlapping(
                        data.as_ptr() as *const u8,
                        buffer.add(offset),
                        first_part,
                    );
                    std::ptr::copy_nonoverlapping(
                        (data.as_ptr() as *const u8).add(first_part),
                        buffer,
                        bytes_to_write - first_part,
                    );
                }

                // Try to claim the slots with CAS; retry if another producer won
                match (*write_pos_atomic).compare_exchange(
                    write_pos,
                    write_pos + bytes_to_write as u64,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return to_write,
                    Err(_) => continue, // CAS failed, retry
                }
            }
        }
    }

    /// Read multiple elements (bulk operation).
    ///
    /// Returns the number of elements actually read.
    /// SPSC only: exactly one consumer at a time.
    pub fn read_bulk(&self, buf: &mut [T]) -> usize {
        unsafe {
            let base = self.memory.at(self.offset);
            let write_pos = (*base.cast::<AtomicU64>()).load(Ordering::Acquire);
            let read_pos = base
                .add(8)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap()
                .load(Ordering::Relaxed);

            // Calculate how many we can read
            let available = write_pos - read_pos;
            let to_read = (buf.len() as u64).min(available / T::SIZE as u64) as usize;

            if to_read == 0 {
                return 0;
            }

            let bytes_to_read = to_read * T::SIZE;
            let offset = (read_pos % self.capacity as u64) as usize;
            let buffer = base.add(RING_HEADER_SIZE);

            if offset + bytes_to_read <= self.capacity as usize {
                // Normal case - contiguous read
                std::ptr::copy_nonoverlapping(
                    buffer.add(offset),
                    buf.as_mut_ptr() as *mut u8,
                    bytes_to_read,
                );
            } else {
                // Wrap around case - split read
                let first_part = self.capacity as usize - offset;
                std::ptr::copy_nonoverlapping(
                    buffer.add(offset),
                    buf.as_mut_ptr() as *mut u8,
                    first_part,
                );
                std::ptr::copy_nonoverlapping(
                    buffer as *mut u8,
                    (buf.as_mut_ptr() as *mut u8).add(first_part),
                    bytes_to_read - first_part,
                );
            }

            // Update read position
            base.add(RING_READ_POS_OFFSET)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap()
                .store(read_pos + bytes_to_read as u64, Ordering::Release);
            to_read
        }
    }

    /// Get number of elements available to read.
    pub fn available(&self) -> usize {
        unsafe {
            let base = self.memory.at(self.offset);
            let write_pos = (*base.cast::<AtomicU64>()).load(Ordering::Acquire);
            let read_pos = base
                .add(8)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap()
                .load(Ordering::Relaxed);
            ((write_pos - read_pos) / T::SIZE as u64) as usize
        }
    }

    /// Get free space in elements.
    pub fn free_space(&self) -> usize {
        unsafe {
            let base = self.memory.at(self.offset);
            let write_pos = (*base.cast::<AtomicU64>()).load(Ordering::Acquire);
            let read_pos = base
                .add(8)
                .cast::<AtomicU64>()
                .as_ref()
                .unwrap()
                .load(Ordering::Relaxed);
            ((self.capacity as u64 - (write_pos - read_pos)) / T::SIZE as u64) as usize
        }
    }

    /// Get capacity in elements.
    pub fn capacity(&self) -> usize {
        (self.capacity as u64 / T::SIZE as u64) as usize
    }

    /// Check if the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.available() == 0
    }

    /// Check if the ring is full.
    pub fn is_full(&self) -> bool {
        self.free_space() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::Pod;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn test_ring_create_and_write_read() {
        let name = "/test_ring_write_read";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let ring = Ring::<i32>::new(&memory, "ring_test", 16).unwrap();

        // Write elements
        assert!(ring.write(&42));
        assert!(ring.write(&100));
        assert!(ring.write(&200));

        // Read elements back
        assert_eq!(ring.read(), Some(42));
        assert_eq!(ring.read(), Some(100));
        assert_eq!(ring.read(), Some(200));

        // Buffer should be empty now
        assert!(ring.is_empty());
        assert_eq!(ring.read(), None);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_wraparound() {
        let name = "/test_ring_wrap";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let ring = Ring::<i32>::new(&memory, "ring_wrap", 4).unwrap();

        // Fill the buffer
        for i in 0..4 {
            assert!(ring.write(&i));
        }
        assert!(ring.is_full());
        assert!(!ring.write(&99)); // Should fail, buffer full

        // Read 2 elements
        assert_eq!(ring.read(), Some(0));
        assert_eq!(ring.read(), Some(1));

        // Now we can write 2 more
        assert!(ring.write(&10));
        assert!(ring.write(&11));

        // Read remaining: 2, 3, 10, 11
        assert_eq!(ring.read(), Some(2));
        assert_eq!(ring.read(), Some(3));
        assert_eq!(ring.read(), Some(10));
        assert_eq!(ring.read(), Some(11));
        assert!(ring.is_empty());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_bulk_operations() {
        let name = "/test_ring_bulk";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let ring = Ring::<i32>::new(&memory, "ring_bulk", 8).unwrap();

        // Write bulk
        let data: Vec<i32> = (0..8).collect();
        let written = ring.write_bulk(&data);
        assert_eq!(written, 8);

        // Read bulk
        let mut buf = vec![0i32; 8];
        let read = ring.read_bulk(&mut buf);
        assert_eq!(read, 8);
        assert_eq!(buf, data);

        // Try to read more than available
        let written = ring.write_bulk(&data);
        assert_eq!(written, 8);
        let mut buf = vec![0i32; 16];
        let read = ring.read_bulk(&mut buf);
        assert_eq!(read, 8);
        assert_eq!(&buf[..8], &data[..]);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_capacity_methods() {
        let name = "/test_ring_cap";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let ring = Ring::<i32>::new(&memory, "ring_cap", 4).unwrap();

        assert_eq!(ring.capacity(), 4);
        assert_eq!(ring.available(), 0);
        assert_eq!(ring.free_space(), 4);
        assert!(ring.is_empty());
        assert!(!ring.is_full());

        ring.write(&1);
        assert_eq!(ring.available(), 1);
        assert_eq!(ring.free_space(), 3);
        assert!(!ring.is_empty());
        assert!(!ring.is_full());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_open() {
        let name = "/test_ring_open";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let ring = Ring::<i32>::new(&memory, "ring_open", 8).unwrap();
            assert!(ring.write(&42));
        }

        let ring = Ring::<i32>::open(&memory, "ring_open").unwrap();
        assert_eq!(ring.read(), Some(42));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_open_not_found() {
        let name = "/test_ring_notfound";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Ring::<i32>::open(&memory, "nonexistent");
        assert!(matches!(result, Err(ZeroShmError::NotFoundError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_spsc_concurrent() {
        let name = "/test_ring_spsc";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let ring = Arc::new(Ring::<i64>::new(&memory, "ring_spsc", 64).unwrap());

        let (tx, rx) = mpsc::channel();
        let producer_ring = Arc::clone(&ring);
        let producer = thread::spawn(move || {
            for i in 0..1000i64 {
                while !producer_ring.write(&i) {
                    thread::yield_now();
                }
            }
            tx.send(()).unwrap();
        });

        let consumer_ring = Arc::clone(&ring);
        let mut count = 0;
        while count < 1000 {
            if let Some(val) = consumer_ring.read() {
                assert_eq!(val, count as i64, "values must arrive in order");
                count += 1;
            } else {
                thread::yield_now();
            }
        }

        producer.join().unwrap();
        rx.recv().unwrap();

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_large_element() {
        #[derive(Clone, Copy)]
        struct BigData {
            a: i64,
            b: i64,
            c: i64,
            d: i64,
        }

        // SAFETY: BigData is a plain struct with only i64 fields, trivially copyable.
        unsafe impl Pod for BigData {
            const SIZE: usize = 32;
            const ALIGN: usize = 8;
        }

        let name = "/test_ring_big";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let ring = Ring::<BigData>::new(&memory, "ring_big", 8).unwrap();

        let val = BigData {
            a: 1,
            b: 2,
            c: 3,
            d: 4,
        };
        assert!(ring.write(&val));

        let read = ring.read().unwrap();
        assert_eq!(read.a, 1);
        assert_eq!(read.b, 2);
        assert_eq!(read.c, 3);
        assert_eq!(read.d, 4);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_zero_capacity_error() {
        let name = "/test_ring_zero";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Ring::<i32>::new(&memory, "ring_zero", 0);
        assert!(matches!(result, Err(ZeroShmError::SizeError(_))));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_ring_already_exists_error() {
        let name = "/test_ring_dup";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        Ring::<i32>::new(&memory, "ring_dup", 4).unwrap();
        let result = Ring::<i32>::new(&memory, "ring_dup", 4);
        assert!(matches!(result, Err(ZeroShmError::AlreadyExists(_))));

        memory.unlink().unwrap();
        drop(memory);
    }
}
