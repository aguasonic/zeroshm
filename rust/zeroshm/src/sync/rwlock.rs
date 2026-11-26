//! Read-write lock for cross-process synchronization.
//!
//! Provides the `RWLock` type that allows multiple concurrent readers OR one
//! exclusive writer. Optimized for read-heavy workloads where reads vastly
//! outnumber writes.
//!
//! # Binary Layout
//! ```text
//! [readers: i32][writer_active: i32]
//! ```
//! Plus additional table entries:
//! - `name_rmtx`: Mutex for reader coordination (16 bytes)
//! - `name_wmtx`: Mutex for writer exclusion (16 bytes)
//! - `name`: 1-byte marker entry
//!
//! - readers: atomic i32 at offset 0 -- number of active readers
//! - writer_active: atomic i32 at offset 4 -- 1 if writer active, 0 otherwise
//!
//! # Algorithm
//! - `reader_lock()`: Lock reader mutex, wait for writer_active==0, increment readers, unlock reader mutex
//! - `writer_lock()`: Lock writer mutex, lock reader mutex, set writer_active=1, unlock reader mutex, wait for readers==0
//! - `reader_unlock()`: Decrement readers
//! - `writer_unlock()`: Set writer_active=0, unlock writer mutex
//!
//! # RAII Guards
//! - `RwLockReadGuard`: Acquires reader lock on construction, releases on drop
//! - `RwLockWriteGuard`: Acquires writer lock on construction, releases on drop

use crate::error::{Result, ZeroShmError};
use crate::layout::RWLOCK_WRITER_ACTIVE_OFFSET;
use crate::memory::Memory;
use crate::sync::mutex::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;

/// Size of the RWLock state in bytes.
const RWLOCK_STATE_SIZE: usize = 8;

/// Size of the marker entry in bytes.
const RWLOCK_MARKER_SIZE: usize = 1;

/// A read-write lock stored in shared memory.
///
/// Allows multiple concurrent readers OR one exclusive writer.
/// Readers do not block each other, but writers block all readers and other writers.
pub struct RWLock {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    reader_mutex: Arc<Mutex>,
    writer_mutex: Arc<Mutex>,
}

impl std::fmt::Debug for RWLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RWLock")
            .field("name", &self.name)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: RWLock holds Arc<Memory> and Arc<Mutex> which are Send + Sync.
// The readers and writer_active fields are atomic, making concurrent access safe.
unsafe impl Send for RWLock {}
unsafe impl Sync for RWLock {}

/// RAII guard that releases the reader lock on drop.
pub struct RwLockReadGuard {
    rwlock: Arc<RWLock>,
}

impl std::fmt::Debug for RwLockReadGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RwLockReadGuard")
            .field("name", &self.rwlock.name)
            .finish()
    }
}

unsafe impl Send for RwLockReadGuard {}
unsafe impl Sync for RwLockReadGuard {}

impl Drop for RwLockReadGuard {
    fn drop(&mut self) {
        self.rwlock.reader_unlock().ok();
    }
}

/// RAII guard that releases the writer lock on drop.
pub struct RwLockWriteGuard {
    rwlock: Arc<RWLock>,
    /// Holds the writer mutex until dropped, ensuring exclusive writer access.
    _writer_guard: MutexGuard,
}

impl std::fmt::Debug for RwLockWriteGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RwLockWriteGuard")
            .field("name", &self.rwlock.name)
            .finish()
    }
}

unsafe impl Send for RwLockWriteGuard {}
unsafe impl Sync for RwLockWriteGuard {}

impl Drop for RwLockWriteGuard {
    fn drop(&mut self) {
        self.rwlock.writer_unlock().ok();
    }
}

impl RWLock {
    /// Create a new RWLock in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the RWLock (stored in the table as a marker)
    pub fn new(memory: &Memory, name: &str) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Create state entry
        let offset = memory.allocate(&format!("{}_state", name), RWLOCK_STATE_SIZE)? as usize;

        // Initialize state: readers=0, writer_active=0
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicI32>()).store(0, Ordering::Relaxed);
            base.add(RWLOCK_WRITER_ACTIVE_OFFSET)
                .cast::<AtomicI32>()
                .as_ref()
                .unwrap()
                .store(0, Ordering::Relaxed);
        }

        // Create reader mutex and writer mutex
        let reader_mutex = Arc::new(Mutex::new(memory, &format!("{}_rmtx", name))?);
        let writer_mutex = Arc::new(Mutex::new(memory, &format!("{}_wmtx", name))?);

        // Add marker entry
        memory.allocate(name, RWLOCK_MARKER_SIZE)?;

        Ok(RWLock {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            reader_mutex,
            writer_mutex,
        })
    }

    /// Open an existing RWLock in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the RWLock to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        // Check if marker exists
        if memory.find(name)?.is_none() {
            return Err(ZeroShmError::NotFoundError(format!(
                "rwlock '{}' not found",
                name
            )));
        }

        // Find state entry
        let (offset, size) = match memory.find(&format!("{}_state", name))? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "rwlock state for '{}' not found",
                    name
                )))
            }
        };

        if size != RWLOCK_STATE_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid rwlock state size: {}, expected {}",
                size, RWLOCK_STATE_SIZE
            )));
        }

        // Open mutexes
        let reader_mutex = Arc::new(Mutex::open(memory, &format!("{}_rmtx", name))?);
        let writer_mutex = Arc::new(Mutex::open(memory, &format!("{}_wmtx", name))?);

        Ok(RWLock {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: offset as usize,
            reader_mutex,
            writer_mutex,
        })
    }

    /// Acquire the reader lock (shared access).
    ///
    /// Multiple threads can hold reader locks simultaneously.
    /// Blocks if a writer currently holds the lock.
    ///
    /// Returns a `RwLockReadGuard` that releases the lock on drop.
    pub fn reader_lock(&self) -> Result<RwLockReadGuard> {
        let writer_active =
            unsafe { self.memory.at(self.offset + 4).cast::<AtomicI32>().as_ref().unwrap() };
        let readers = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };

        // Lock reader mutex, check for writer, increment readers, unlock
        loop {
            let guard = self.reader_mutex.lock()?;

            // Check if writer is active
            if writer_active.load(Ordering::Acquire) == 0 {
                // No writer active, increment readers and proceed
                readers.fetch_add(1, Ordering::Release);
                drop(guard);
                break;
            }

            // Writer is active, release and retry
            drop(guard);
            thread::yield_now();
        }

        Ok(RwLockReadGuard {
            rwlock: Arc::new(RWLock {
                memory: Arc::clone(&self.memory),
                name: self.name.clone(),
                offset: self.offset,
                reader_mutex: Arc::clone(&self.reader_mutex),
                writer_mutex: Arc::clone(&self.writer_mutex),
            }),
        })
    }

    /// Try to acquire the reader lock without blocking.
    ///
    /// Returns `Ok(Some(guard))` if the lock was acquired, `Ok(None)` otherwise.
    pub fn try_reader_lock(&self) -> Result<Option<RwLockReadGuard>> {
        // Try to lock reader mutex
        let guard = match self.reader_mutex.try_lock()? {
            Some(g) => g,
            None => return Ok(None),
        };

        // Check if writer is active
        let writer_active =
            unsafe { self.memory.at(self.offset + 4).cast::<AtomicI32>().as_ref().unwrap() };

        if writer_active.load(Ordering::Acquire) != 0 {
            drop(guard);
            return Ok(None);
        }

        // Increment readers count
        let readers = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        readers.fetch_add(1, Ordering::Release);

        // Unlock reader mutex
        drop(guard);

        Ok(Some(RwLockReadGuard {
            rwlock: Arc::new(RWLock {
                memory: Arc::clone(&self.memory),
                name: self.name.clone(),
                offset: self.offset,
                reader_mutex: Arc::clone(&self.reader_mutex),
                writer_mutex: Arc::clone(&self.writer_mutex),
            }),
        }))
    }

    /// Release the reader lock.
    pub fn reader_unlock(&self) -> Result<()> {
        let readers = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        let prev = readers.fetch_sub(1, Ordering::Release);
        debug_assert!(prev > 0, "reader_unlock without a matching reader_lock");
        Ok(())
    }

    /// Acquire the writer lock (exclusive access).
    ///
    /// Only one writer can hold the lock. Blocks until all readers release
    /// their locks and no other writer holds the lock.
    ///
    /// Returns a `RwLockWriteGuard` that releases the lock on drop.
    pub fn writer_lock(&self) -> Result<RwLockWriteGuard> {
        // Lock writer mutex to ensure exclusive writer access
        // Keep the guard alive -- it will be released when RwLockWriteGuard is dropped
        let writer_guard = self.writer_mutex.lock()?;

        // Lock reader mutex to atomically set writer_active with respect to readers
        let _rguard = self.reader_mutex.lock()?;

        // Mark writer as active
        let writer_active =
            unsafe { self.memory.at(self.offset + 4).cast::<AtomicI32>().as_ref().unwrap() };
        writer_active.store(1, Ordering::Release);

        // Unlock reader mutex (writers can set the flag, readers see it)
        drop(_rguard);

        // Wait for all readers to finish
        let readers = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };

        while readers.load(Ordering::Acquire) > 0 {
            thread::yield_now();
        }

        Ok(RwLockWriteGuard {
            rwlock: Arc::new(RWLock {
                memory: Arc::clone(&self.memory),
                name: self.name.clone(),
                offset: self.offset,
                reader_mutex: Arc::clone(&self.reader_mutex),
                writer_mutex: Arc::clone(&self.writer_mutex),
            }),
            _writer_guard: writer_guard,
        })
    }

    /// Try to acquire the writer lock without blocking.
    ///
    /// Returns `Ok(Some(guard))` if the lock was acquired, `Ok(None)` otherwise.
    pub fn try_writer_lock(&self) -> Result<Option<RwLockWriteGuard>> {
        // Try to lock writer mutex
        let wguard = match self.writer_mutex.try_lock()? {
            Some(g) => g,
            None => return Ok(None),
        };

        // Try to lock reader mutex
        let rguard = match self.reader_mutex.try_lock()? {
            Some(g) => g,
            None => {
                drop(wguard);
                return Ok(None);
            }
        };

        // Check if any readers are active
        let readers = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };

        if readers.load(Ordering::Acquire) > 0 {
            drop(rguard);
            drop(wguard);
            return Ok(None);
        }

        // Mark writer as active
        let writer_active =
            unsafe { self.memory.at(self.offset + 4).cast::<AtomicI32>().as_ref().unwrap() };
        writer_active.store(1, Ordering::Release);

        // Unlock reader mutex
        drop(rguard);

        Ok(Some(RwLockWriteGuard {
            rwlock: Arc::new(RWLock {
                memory: Arc::clone(&self.memory),
                name: self.name.clone(),
                offset: self.offset,
                reader_mutex: Arc::clone(&self.reader_mutex),
                writer_mutex: Arc::clone(&self.writer_mutex),
            }),
            _writer_guard: wguard,
        }))
    }

    /// Release the writer lock.
    ///
    /// Clears the writer_active flag so readers can proceed.
    /// The writer mutex is released by the RwLockWriteGuard's drop.
    pub fn writer_unlock(&self) -> Result<()> {
        let writer_active =
            unsafe { self.memory.at(self.offset + 4).cast::<AtomicI32>().as_ref().unwrap() };
        writer_active.store(0, Ordering::Release);
        Ok(())
    }

    /// Get the number of active readers.
    pub fn readers(&self) -> i32 {
        let readers = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        readers.load(Ordering::Acquire)
    }

    /// Check if a writer currently holds the lock.
    pub fn is_writer_locked(&self) -> bool {
        let writer_active =
            unsafe { self.memory.at(self.offset + 4).cast::<AtomicI32>().as_ref().unwrap() };
        writer_active.load(Ordering::Acquire) != 0
    }

    /// Get the RWLock name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Get the offset of the RWLock state in shared memory.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::Array;
    use crate::memory::Memory;
    use std::sync::Arc;
    use std::thread;
    use crate::platform;
    use std::time::Duration;

    fn setup_memory(suffix: &str) -> (Memory, String) {
        let name = format!("test_rwlock_{}_{}", suffix, std::process::id());
        let _ = platform::unlink_shm(&name);
        let mem = Memory::create(&name, 64 * 1024, 64).expect("failed to create memory");
        (mem, name)
    }

    fn cleanup_memory(mem: &Memory, name: &str) {
        let _ = mem.unlink();
        let _ = Memory::open(name, 64).map(|m| m.unlink());
    }

    #[test]
    fn test_rwlock_basic_read() {
        let (mem, name) = setup_memory("basic_read");
        let rwlock = RWLock::new(&mem, "rwlock").expect("failed to create rwlock");

        // Acquire reader lock
        let guard = rwlock.reader_lock().expect("reader_lock failed");
        assert_eq!(rwlock.readers(), 1);
        assert!(!rwlock.is_writer_locked());

        // Release reader lock
        drop(guard);
        assert_eq!(rwlock.readers(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_concurrent_readers() {
        let (mem, name) = setup_memory("creaders");
        let rwlock = Arc::new(RWLock::new(&mem, "rwlock").expect("failed to create rwlock"));

        // Spawn multiple readers
        let mut handles = Vec::new();
        for _ in 0..4 {
            let rwlock2 = Arc::clone(&rwlock);
            handles.push(thread::spawn(move || {
                let guard = rwlock2.reader_lock().expect("reader_lock failed");
                // All readers should be able to hold the lock simultaneously
                assert!(rwlock2.readers() > 0);
                thread::sleep(Duration::from_millis(10));
                drop(guard);
            }));
        }

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        assert_eq!(rwlock.readers(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_writer_exclusive() {
        let (mem, name) = setup_memory("wexcl");
        let rwlock = Arc::new(RWLock::new(&mem, "rwlock").expect("failed to create rwlock"));

        // Acquire writer lock
        let guard = rwlock.writer_lock().expect("writer_lock failed");
        assert!(rwlock.is_writer_locked());
        assert_eq!(rwlock.readers(), 0);

        // Try to acquire reader lock while writer holds it -- should fail
        let result = rwlock.try_reader_lock();
        match result {
            Ok(None) => {} // Expected
            Ok(Some(_)) => panic!("reader should not be able to lock while writer holds lock"),
            Err(e) => panic!("unexpected error: {:?}", e),
        }

        // Release writer lock
        drop(guard);
        assert!(!rwlock.is_writer_locked());

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_writer_waits_for_readers() {
        let (mem, name) = setup_memory("wwaits");
        let rwlock = Arc::new(RWLock::new(&mem, "rwlock").expect("failed to create rwlock"));

        // Acquire a reader lock
        let rguard = rwlock.reader_lock().expect("reader_lock failed");

        // Writer should block until reader releases
        let handle = thread::spawn({
            let rwlock2 = Arc::clone(&rwlock);
            move || {
                thread::sleep(Duration::from_millis(10));
                rwlock2.writer_lock().expect("writer_lock failed")
            }
        });

        // Give writer time to start waiting
        thread::sleep(Duration::from_millis(20));

        // Release reader lock -- writer should now acquire
        drop(rguard);

        let wguard = handle.join().expect("thread panicked");
        assert!(rwlock.is_writer_locked());
        drop(wguard);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_multithreaded_counter() {
        let (mem, name) = setup_memory("multi");
        let rwlock = Arc::new(RWLock::new(&mem, "rwlock").expect("failed to create rwlock"));

        // Create a shared counter array
        let counter = Arc::new(Array::new(&mem, "counter", 1).expect("failed to create array"));
        counter.set(0, 0i32);

        // Spawn readers and writers
        let mut handles = Vec::new();

        // 4 readers that read the counter 1000 times each
        for _ in 0..4 {
            let rwlock2 = Arc::clone(&rwlock);
            let counter2 = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    let guard = rwlock2.reader_lock().expect("reader_lock failed");
                    let _ = counter2.get(0);
                    drop(guard);
                }
            }));
        }

        // 2 writers that increment the counter 1000 times each
        for _ in 0..2 {
            let rwlock2 = Arc::clone(&rwlock);
            let counter2 = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    let guard = rwlock2.writer_lock().expect("writer_lock failed");
                    counter2.set(0, counter2.get(0) + 1);
                    drop(guard);
                }
            }));
        }

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        // Final counter should be 2000 (2 writers  1000 increments)
        let final_count = counter.get(0);
        assert_eq!(final_count, 2000, "expected 2000, got {}", final_count);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_open_persistence() {
        let (mem, name) = setup_memory("persistence");
        let rwlock = RWLock::new(&mem, "rwlock").expect("failed to create rwlock");

        // Acquire writer lock, then drop rwlock
        let guard = rwlock.writer_lock().expect("writer_lock failed");
        assert!(rwlock.is_writer_locked());
        drop(guard);

        // Reopen
        let rwlock2 = RWLock::open(&mem, "rwlock").expect("failed to open rwlock");
        assert!(!rwlock2.is_writer_locked());

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_not_found() {
        let (mem, name) = setup_memory("notfound");

        let result = RWLock::open(&mem, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(_) => {}
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_duplicate_name() {
        let (mem, name) = setup_memory("duplicate");

        RWLock::new(&mem, "rwlock").expect("failed to create rwlock");
        let result = RWLock::new(&mem, "rwlock");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(_) => {}
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_read_guard_raii() {
        let (mem, name) = setup_memory("rdguard");
        let rwlock = RWLock::new(&mem, "rwlock").expect("failed to create rwlock");

        assert_eq!(rwlock.readers(), 0);

        {
            let _guard = rwlock.reader_lock().expect("reader_lock failed");
            assert_eq!(rwlock.readers(), 1);
        }

        // Guard dropped, readers should be 0
        assert_eq!(rwlock.readers(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_rwlock_write_guard_raii() {
        let (mem, name) = setup_memory("wgguard");
        let rwlock = RWLock::new(&mem, "rwlock").expect("failed to create rwlock");

        assert!(!rwlock.is_writer_locked());

        {
            let _guard = rwlock.writer_lock().expect("writer_lock failed");
            assert!(rwlock.is_writer_locked());
        }

        // Guard dropped, writer should be unlocked
        assert!(!rwlock.is_writer_locked());

        cleanup_memory(&mem, &name);
    }
}

