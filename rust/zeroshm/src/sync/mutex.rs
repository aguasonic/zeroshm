//! Cross-process mutex using a binary semaphore.
//!
//! Provides the `Mutex` type for mutual exclusion stored in shared memory,
//! and `MutexGuard` for RAII-style lock management.
//!
//! Internally implemented as a binary semaphore (initial_count=1, max_count=1).

use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use crate::sync::semaphore::Semaphore;
use std::sync::Arc;

/// A cross-process mutex stored in shared memory.
///
/// Internally wraps a binary semaphore with max_count=1.
/// Provides `lock()` and `try_lock()` for acquiring exclusive access,
/// returning a `MutexGuard` that releases the lock on drop.
pub struct Mutex {
    sem: Arc<Semaphore>,
    name: String,
}

impl std::fmt::Debug for Mutex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mutex")
            .field("name", &self.name)
            .finish()
    }
}

// SAFETY: Mutex holds an Arc<Semaphore> which is Send + Sync.
unsafe impl Send for Mutex {}
unsafe impl Sync for Mutex {}

/// RAII guard that releases the mutex on drop.
pub struct MutexGuard {
    mutex: Arc<Mutex>,
}

impl std::fmt::Debug for MutexGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MutexGuard")
            .field("name", &self.mutex.name)
            .finish()
    }
}

// SAFETY: MutexGuard holds an Arc<Mutex> which is Send + Sync.
unsafe impl Send for MutexGuard {}
unsafe impl Sync for MutexGuard {}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        // Release the semaphore (ignore error -- mutex should never overflow)
        let _ = self.mutex.sem.release();
    }
}

impl Mutex {
    /// Create a new mutex in shared memory.
    ///
    /// Internally creates a binary semaphore with initial_count=1, max_count=1.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the mutex (stored in the table)
    pub fn new(memory: &Memory, name: &str) -> Result<Self> {
        // Create binary semaphore with initial_count=1, max_count=1
        let sem = Semaphore::new(memory, name, 1, 1)?;

        Ok(Mutex {
            sem: Arc::new(sem),
            name: name.to_string(),
        })
    }

    /// Open an existing mutex in shared memory.
    ///
    /// Validates that the underlying semaphore is binary (max_count=1).
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the mutex to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let sem = Semaphore::open(memory, name)?;

        // Verify it's a binary semaphore
        if sem.max_count() != 1 {
            return Err(ZeroShmError::SizeError(format!(
                "not a mutex: max_count={}, expected 1",
                sem.max_count()
            )));
        }

        Ok(Mutex {
            sem: Arc::new(sem),
            name: name.to_string(),
        })
    }

    /// Acquire the mutex, blocking until available.
    ///
    /// Returns a `MutexGuard` that releases the lock on drop.
    pub fn lock(&self) -> Result<MutexGuard> {
        self.acquire()?;
        Ok(MutexGuard {
            mutex: Arc::new(Mutex {
                sem: Arc::clone(&self.sem),
                name: self.name.clone(),
            }),
        })
    }

    /// Acquire the mutex without creating a guard.
    ///
    /// Used by composite primitives (like Monitor) that manage the lock
    /// lifecycle themselves.
    pub fn acquire(&self) -> Result<()> {
        self.sem.acquire()
    }

    /// Try to acquire the mutex without blocking.
    ///
    /// Returns `Ok(true)` if acquired, `Ok(false)` if already locked.
    pub fn try_acquire(&self) -> Result<bool> {
        self.sem.try_acquire()
    }

    /// Try to acquire the mutex without blocking.
    ///
    /// Returns `Ok(Some(MutexGuard))` if acquired, `Ok(None)` if already locked.
    pub fn try_lock(&self) -> Result<Option<MutexGuard>> {
        if self.sem.try_acquire()? {
            Ok(Some(MutexGuard {
                mutex: Arc::new(Mutex {
                    sem: Arc::clone(&self.sem),
                    name: self.name.clone(),
                }),
            }))
        } else {
            Ok(None)
        }
    }

    /// Release the mutex (for use by composite primitives).
    ///
    /// This releases the underlying semaphore without requiring the caller
    /// to hold a MutexGuard. Used by composite primitives like RWLock and
    /// Monitor that manage the semaphore directly.
    pub fn release(&self) -> Result<()> {
        self.sem.release()
    }

    /// Returns true if the mutex appears to be locked.
    ///
    /// Note: In a concurrent context, this is only an approximation.
    pub fn is_locked(&self) -> bool {
        self.sem.count() == 0
    }

    /// Returns the mutex name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        self.sem.memory()
    }

    /// Returns the byte offset of the mutex in the memory segment.
    pub fn offset(&self) -> usize {
        self.sem.offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::thread;

    #[test]
    fn test_mutex_lock_unlock() {
        let name = "/test_step10_mutex_basic";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let mutex = Mutex::new(&memory, "basic_mutex").expect("create should succeed");
        assert!(!mutex.is_locked());

        // Lock
        let guard = mutex.lock().expect("lock should succeed");
        assert!(mutex.is_locked());

        // Try to lock again from same thread -- should fail (binary semaphore)
        assert!(mutex.try_lock().expect("try_lock should succeed").is_none());

        // Drop guard -- releases lock
        drop(guard);
        assert!(!mutex.is_locked());

        // Lock again
        let guard = mutex.lock().expect("lock should succeed");
        assert!(mutex.is_locked());
        drop(guard);

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_mutex_try_lock() {
        let name = "/test_step10_mutex_trylock";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let mutex = Mutex::new(&memory, "trylock_mutex").expect("create should succeed");

        // First try_lock should succeed
        let guard = mutex
            .try_lock()
            .expect("try_lock should succeed")
            .expect("should acquire lock");
        assert!(mutex.is_locked());

        // Second try_lock should fail
        assert!(mutex.try_lock().expect("try_lock should succeed").is_none());

        drop(guard);
        assert!(!mutex.is_locked());

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_mutex_multithreaded_counter() {
        let name = "/test_step10_mutex_mt";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let mutex = Arc::new(Mutex::new(&memory, "mt_mutex").expect("create should succeed"));

        // Shared counter in shared memory (u32 array with one element)
        let counter = Arc::new(
            crate::array::Array::<u32>::new(&memory, "counter", 1)
                .expect("create counter should succeed"),
        );
        counter.set(0, 0);

        let num_threads = 4;
        let increments_per_thread = 10_000;

        let mut handles = Vec::new();
        for _ in 0..num_threads {
            let mutex = Arc::clone(&mutex);
            let counter = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for _ in 0..increments_per_thread {
                    let guard = mutex.lock().expect("lock should succeed");
                    let current = counter.get(0);
                    counter.set(0, current + 1);
                    drop(guard);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        // Verify final count is exactly num_threads * increments_per_thread
        let final_count = counter.get(0);
        assert_eq!(
            final_count,
            (num_threads * increments_per_thread) as u32,
            "counter should be {} (no races)",
            num_threads * increments_per_thread
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_mutex_open_persistence() {
        let name = "/test_step10_mutex_persist";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create the underlying semaphore directly and acquire it (count goes to 0)
        // This simulates a process that locked the mutex and then crashed without releasing
        {
            let sem = Semaphore::new(&memory, "persist_mutex", 1, 1).expect("create should succeed");
            sem.try_acquire().expect("try_acquire should succeed");
            assert_eq!(sem.count(), 0); // locked
            // sem goes out of scope, but the shared memory state persists
        }

        // Reopen as mutex and verify it's still locked (count=0)
        let memory2 = Memory::open(name, 64).expect("reopen should succeed");
        let mutex = Mutex::open(&memory2, "persist_mutex").expect("open should succeed");
        assert!(mutex.is_locked(), "mutex should still be locked after reopen");

        // try_lock should fail because the semaphore count is 0
        assert!(mutex.try_lock().expect("try_lock should succeed").is_none());

        // Clean up
        memory2.unlink().expect("unlink should succeed");
        drop(memory2);
        drop(memory);
    }

    #[test]
    fn test_mutex_not_a_mutex() {
        let name = "/test_step10_mutex_notmutex";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create a non-binary semaphore
        let _sem = Semaphore::new(&memory, "not_a_mutex", 5, 10).expect("create should succeed");

        // Try to open as mutex -- should fail
        let result = Mutex::open(&memory, "not_a_mutex");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::SizeError(msg) => {
                assert!(msg.contains("not a mutex"));
            }
            other => panic!("expected SizeError, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }
}

