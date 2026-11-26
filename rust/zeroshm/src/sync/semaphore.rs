//! Lock-free counting semaphore for cross-process synchronization.
//!
//! Provides the `Semaphore` type for permit-based synchronization stored in
//! shared memory. Uses CAS loops for lock-free acquire/release operations.
//!
//! # Binary Layout
//! ```text
//! [count: i32][waiting: i32][max_count: i32][padding: i32]
//! ```
//!
//! - count: atomic i32 at offset 0 -- current permit count
//! - waiting: atomic i32 at offset 4 -- number of processes currently waiting
//! - max_count: i32 at offset 8 -- maximum count (0 = unbounded)
//! - padding: i32 at offset 12 -- alignment padding

use crate::error::{Result, ZeroShmError};
use crate::layout::{SEMAPHORE_WAITING_OFFSET, SEMAPHORE_MAX_COUNT_OFFSET, SEMAPHORE_PADDING_OFFSET};
use crate::memory::Memory;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

/// Size of the semaphore header in bytes.
pub const SEMAPHORE_HEADER_SIZE: usize = 16;

/// A lock-free counting semaphore stored in shared memory.
///
/// Supports three modes:
/// - Binary semaphore (max_count=1): acts as a mutex
/// - Counting semaphore (max_count=N): resource pool with N permits
/// - Unbounded semaphore (max_count=0): no upper limit on count
pub struct Semaphore {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    max_count: i32,
}

impl std::fmt::Debug for Semaphore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Semaphore")
            .field("name", &self.name)
            .field("max_count", &self.max_count)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Semaphore holds an Arc<Memory> which is Send + Sync. The count
// and waiting fields are atomic, making concurrent access safe.
unsafe impl Send for Semaphore {}
unsafe impl Sync for Semaphore {}

impl Semaphore {
    /// Create a new semaphore in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the semaphore (stored in the table)
    /// * `initial_count` -- Initial permit count (must be non-negative)
    /// * `max_count` -- Maximum count (0 = unbounded, 1 = binary/mutex-like)
    pub fn new(
        memory: &Memory,
        name: &str,
        initial_count: i32,
        max_count: i32,
    ) -> Result<Self> {
        if initial_count < 0 {
            return Err(ZeroShmError::SizeError(
                "initial count must be non-negative".to_string(),
            ));
        }

        if max_count < 0 {
            return Err(ZeroShmError::SizeError(
                "max count must be non-negative (0 for unbounded)".to_string(),
            ));
        }

        if max_count > 0 && initial_count > max_count {
            return Err(ZeroShmError::SizeError(format!(
                "initial count {} cannot exceed max count {}",
                initial_count, max_count
            )));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Allocate space
        let offset = memory.allocate(name, SEMAPHORE_HEADER_SIZE)? as usize;

        // Write header: count, waiting=0, max_count, padding=0
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicI32>()).store(initial_count, Ordering::Relaxed);
            base.add(SEMAPHORE_WAITING_OFFSET)
                .cast::<AtomicI32>()
                .as_ref()
                .unwrap()
                .store(0, Ordering::Relaxed);
            std::ptr::write(base.add(SEMAPHORE_MAX_COUNT_OFFSET).cast::<i32>(), max_count);
            std::ptr::write(base.add(SEMAPHORE_PADDING_OFFSET).cast::<i32>(), 0);
        }

        Ok(Semaphore {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            max_count,
        })
    }

    /// Open an existing semaphore in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the semaphore to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "semaphore '{}' not found",
                    name
                )))
            }
        };

        if size != SEMAPHORE_HEADER_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid semaphore size: {}, expected {}",
                size, SEMAPHORE_HEADER_SIZE
            )));
        }

        let offset = offset as usize;
        let max_count = unsafe { memory.at(offset + 8).cast::<i32>().read_unaligned() };

        Ok(Semaphore {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            max_count,
        })
    }

    /// Returns a pointer to the atomic count.
    fn count_ptr(&self) -> *const AtomicI32 {
        self.memory.at(self.offset).cast::<AtomicI32>()
    }

    /// Returns a pointer to the atomic waiting counter.
    fn waiting_ptr(&self) -> *const AtomicI32 {
        self.memory.at(self.offset + 4).cast::<AtomicI32>()
    }

    /// Acquire one permit from the semaphore.
    ///
    /// Blocks (spin-waits) until a permit is available.
    pub fn acquire(&self) -> Result<()> {
        // Increment waiting count
        unsafe { (*self.waiting_ptr()).fetch_add(1, Ordering::SeqCst) };

        // Ensure we decrement waiting even if we return early
        struct WaitingGuard<'a> {
            ptr: *const AtomicI32,
            _phantom: std::marker::PhantomData<&'a AtomicI32>,
        }
        impl Drop for WaitingGuard<'_> {
            fn drop(&mut self) {
                unsafe { (*self.ptr).fetch_sub(1, Ordering::SeqCst) };
            }
        }
        let _guard = WaitingGuard {
            ptr: self.waiting_ptr(),
            _phantom: std::marker::PhantomData,
        };

        loop {
            let current = unsafe { (*self.count_ptr()).load(Ordering::Acquire) };

            if current > 0 {
                if unsafe {
                    (*self.count_ptr())
                        .compare_exchange_weak(
                            current,
                            current - 1,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                } {
                    return Ok(());
                }
            }

            // Spin-wait and retry
            std::thread::yield_now();
        }
    }

    /// Try to acquire one permit without blocking.
    ///
    /// Returns `Ok(true)` if permit was acquired, `Ok(false)` if count was 0.
    pub fn try_acquire(&self) -> Result<bool> {
        loop {
            let current = unsafe { (*self.count_ptr()).load(Ordering::Acquire) };

            if current <= 0 {
                return Ok(false);
            }

            if unsafe {
                (*self.count_ptr())
                    .compare_exchange_weak(
                        current,
                        current - 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
            } {
                return Ok(true);
            }
            // CAS failed, retry
        }
    }

    /// Try to acquire one permit with a timeout.
    ///
    /// Returns `Ok(true)` if permit was acquired, `Ok(false)` if timeout
    /// elapsed without acquiring a permit.
    pub fn try_acquire_timeout(&self, timeout: std::time::Duration) -> Result<bool> {
        let deadline = std::time::Instant::now() + timeout;

        // Increment waiting count
        unsafe { (*self.waiting_ptr()).fetch_add(1, Ordering::SeqCst) };

        // Ensure we decrement waiting even if we return early
        struct WaitingGuard<'a> {
            ptr: *const AtomicI32,
            _phantom: std::marker::PhantomData<&'a AtomicI32>,
        }
        impl Drop for WaitingGuard<'_> {
            fn drop(&mut self) {
                unsafe { (*self.ptr).fetch_sub(1, Ordering::SeqCst) };
            }
        }
        let _guard = WaitingGuard {
            ptr: self.waiting_ptr(),
            _phantom: std::marker::PhantomData,
        };

        loop {
            let current = unsafe { (*self.count_ptr()).load(Ordering::Acquire) };

            if current > 0 {
                if unsafe {
                    (*self.count_ptr())
                        .compare_exchange_weak(
                            current,
                            current - 1,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                } {
                    return Ok(true);
                }
            }

            if std::time::Instant::now() > deadline {
                return Ok(false);
            }

            // Spin-wait and retry
            std::thread::yield_now();
        }
    }

    /// Release one permit back to the semaphore.
    ///
    /// Returns an error if max_count would be exceeded.
    /// Uses a CAS loop to atomically check and increment, preventing TOCTOU races.
    pub fn release(&self) -> Result<()> {
        loop {
            let current = unsafe { (*self.count_ptr()).load(Ordering::Acquire) };

            if self.max_count > 0 && current >= self.max_count {
                return Err(ZeroShmError::SizeError(
                    "semaphore count would exceed maximum".to_string(),
                ));
            }

            if unsafe {
                (*self.count_ptr())
                    .compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
            } {
                return Ok(());
            }
            // CAS failed, retry
        }
    }

    /// Returns the current semaphore count.
    pub fn count(&self) -> i32 {
        unsafe { (*self.count_ptr()).load(Ordering::Acquire) }
    }

    /// Returns the number of processes currently waiting.
    pub fn waiting(&self) -> i32 {
        unsafe { (*self.waiting_ptr()).load(Ordering::Acquire) }
    }

    /// Returns the maximum count (0 = unbounded).
    pub fn max_count(&self) -> i32 {
        self.max_count
    }

    /// Returns the semaphore name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset of the semaphore in the memory segment.
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
    fn test_semaphore_acquire_release() {
        let name = "/test_step10_sem_basic";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create semaphore with count=5, max=10
        let sem = Semaphore::new(&memory, "basic_sem", 5, 10).expect("create should succeed");
        assert_eq!(sem.count(), 5);
        assert_eq!(sem.max_count(), 10);

        // Acquire 5 permits
        for _ in 0..5 {
            assert!(sem.try_acquire().expect("try_acquire should succeed"));
        }
        assert_eq!(sem.count(), 0);

        // Try to acquire when empty -- should fail
        assert!(!sem.try_acquire().expect("try_acquire should succeed"));

        // Release 3 permits
        for _ in 0..3 {
            sem.release().expect("release should succeed");
        }
        assert_eq!(sem.count(), 3);

        // Acquire 3 permits
        for _ in 0..3 {
            assert!(sem.try_acquire().expect("try_acquire should succeed"));
        }
        assert_eq!(sem.count(), 0);

        // Release to max
        for _ in 0..10 {
            sem.release().expect("release should succeed");
        }
        assert_eq!(sem.count(), 10);

        // Release beyond max -- should fail
        assert!(sem.release().is_err());

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_semaphore_unbounded() {
        let name = "/test_step10_sem_unbounded";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create unbounded semaphore (max_count=0)
        let sem = Semaphore::new(&memory, "unbounded_sem", 0, 0).expect("create should succeed");
        assert_eq!(sem.max_count(), 0);

        // Can release beyond any limit
        for _ in 0..100 {
            sem.release().expect("release should succeed");
        }
        assert_eq!(sem.count(), 100);

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_semaphore_multithreaded() {
        let name = "/test_step10_sem_mt";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create semaphore with count=5
        let sem = Arc::new(Semaphore::new(&memory, "mt_sem", 5, 5).expect("create should succeed"));

        let acquired: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let released: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        // 10 threads each acquire and release
        let mut handles = Vec::new();
        for _ in 0..10 {
            let sem = Arc::clone(&sem);
            let acquired = Arc::clone(&acquired);
            let released = Arc::clone(&released);
            let handle = thread::spawn(move || {
                sem.acquire().expect("acquire should succeed");
                acquired.fetch_add(1, SyncOrdering::SeqCst);

                // Simulate critical section work
                thread::sleep(std::time::Duration::from_micros(100));

                // At most 5 threads should be in the critical section at once
                // (we can't easily verify this from outside, but the count should never go negative)
                assert!(sem.count() >= 0);

                released.fetch_add(1, SyncOrdering::SeqCst);
                sem.release().expect("release should succeed");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        // All 10 threads should have acquired and released
        assert_eq!(acquired.load(SyncOrdering::SeqCst), 10);
        assert_eq!(released.load(SyncOrdering::SeqCst), 10);
        assert_eq!(sem.count(), 5); // back to initial

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_semaphore_open_persistence() {
        let name = "/test_step10_sem_persist";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create semaphore and acquire one
        {
            let sem = Semaphore::new(&memory, "persist_sem", 3, 10).expect("create should succeed");
            sem.try_acquire().expect("try_acquire should succeed");
            assert_eq!(sem.count(), 2);
        }

        // Reopen and verify count persists
        let memory2 = Memory::open(name, 64).expect("reopen should succeed");
        let sem = Semaphore::open(&memory2, "persist_sem").expect("open should succeed");
        assert_eq!(sem.count(), 2);

        // Clean up
        memory2.unlink().expect("unlink should succeed");
        drop(memory2);
        drop(memory);
    }

    #[test]
    fn test_semaphore_not_found() {
        let name = "/test_step10_sem_notfound";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let result = Semaphore::open(&memory, "nonexistent_sem");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(msg) => {
                assert!(msg.contains("nonexistent_sem"));
            }
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_semaphore_duplicate_name() {
        let name = "/test_step10_sem_dup";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let _sem = Semaphore::new(&memory, "dup_sem", 1, 5).expect("create should succeed");

        let result = Semaphore::new(&memory, "dup_sem", 1, 5);
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(msg) => {
                assert!(msg.contains("dup_sem"));
            }
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }
}
