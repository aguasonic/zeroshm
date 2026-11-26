//! One-time initialization primitive for shared memory.
//!
//! Provides the `Once` type for ensuring a function is executed exactly once
//! across all processes, similar to `std::sync::Once` but for shared memory.
//!
//! # Binary Layout
//! ```text
//! [state: u32]
//! ```
//!
//! # State Machine (3-state protocol)
//! ```text
//! PENDING(0) -> EXECUTING(1) -> DONE(2)
//! ```
//!
//! - PENDING: No one has started execution yet
//! - EXECUTING: A thread/process is currently running the function
//! - DONE: The function has completed execution

use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Size of the Once flag in bytes.
pub const ONCE_HEADER_SIZE: usize = 4;

/// Once state: no one has started execution yet.
const ONCE_PENDING: u32 = 0;
/// Once state: a thread/process is currently running the function.
const ONCE_EXECUTING: u32 = 1;
/// Once state: the function has completed execution.
const ONCE_DONE: u32 = 2;

/// A one-time initialization primitive stored in shared memory.
///
/// Ensures a function is executed exactly once across all processes,
/// similar to `std::sync::Once`. Thread-safe and process-safe.
pub struct Once {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
}

impl std::fmt::Debug for Once {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Once")
            .field("name", &self.name)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Once holds an Arc<Memory> which is Send + Sync. The state field
// is atomic, making concurrent access safe.
unsafe impl Send for Once {}
unsafe impl Sync for Once {}

impl Once {
    /// Create a new Once flag in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the Once flag (stored in the table)
    pub fn new(memory: &Memory, name: &str) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Allocate space
        let offset = memory.allocate(name, ONCE_HEADER_SIZE)? as usize;

        // Initialize state to 0 (pending)
        unsafe {
            (*memory.at_mut(offset).cast::<AtomicU32>()).store(0, Ordering::Relaxed);
        }

        Ok(Once {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
        })
    }

    /// Open an existing Once flag in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the Once flag to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "once flag '{}' not found",
                    name
                )))
            }
        };

        if size != ONCE_HEADER_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid once flag size: {}, expected {}",
                size, ONCE_HEADER_SIZE
            )));
        }

        Ok(Once {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: offset as usize,
        })
    }

    /// Returns a pointer to the atomic state.
    fn state_ptr(&self) -> *const AtomicU32 {
        self.memory.at(self.offset).cast::<AtomicU32>()
    }

    /// Execute the function exactly once across all processes.
    ///
    /// If this is the first call, executes `f`. All other callers block until
    /// `f` completes, then return without executing `f`.
    ///
    /// Note: If `f` panics, the Once flag is still marked as done
    /// (matching `std::sync::Once` semantics).
    pub fn call<F: FnOnce()>(&self, f: F) {
        // Fast path: already done
        if (unsafe { (*self.state_ptr()).load(Ordering::Acquire) }) == ONCE_DONE {
            return;
        }

        // Try to transition from PENDING to EXECUTING
        if unsafe {
            (*self.state_ptr())
                .compare_exchange(
                    ONCE_PENDING,
                    ONCE_EXECUTING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        } {
            // We won the race -- execute the function
            // Use defer-like pattern: ensure state is set to DONE even if f panics
            struct DoneGuard {
                ptr: *const AtomicU32,
            }
            impl Drop for DoneGuard {
                fn drop(&mut self) {
                    unsafe { (*self.ptr).store(ONCE_DONE, Ordering::Release) };
                }
            }
            let _guard = DoneGuard {
                ptr: self.state_ptr(),
            };
            f();
        } else {
            // We lost the race -- spin-wait until the executor finishes
            loop {
                let state = unsafe { (*self.state_ptr()).load(Ordering::Acquire) };
                if state != ONCE_EXECUTING {
                    break;
                }
                std::thread::yield_now();
            }
        }
    }

    /// Returns true if the function has been executed.
    pub fn is_called(&self) -> bool {
        (unsafe { (*self.state_ptr()).load(Ordering::Acquire) }) == ONCE_DONE
    }

    /// Returns the Once flag name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset of the Once flag in the memory segment.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as SyncOrdering};
    use std::thread;

    #[test]
    fn test_once_basic() {
        let name = "/test_step10_once_basic";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let once = Once::new(&memory, "basic_once").expect("create should succeed");
        assert!(!once.is_called());

        let executed = Arc::new(AtomicUsize::new(0));
        let executed_clone = Arc::clone(&executed);

        // First call should execute the function
        once.call(|| {
            executed_clone.fetch_add(1, SyncOrdering::SeqCst);
        });

        assert!(once.is_called());
        assert_eq!(executed.load(SyncOrdering::SeqCst), 1);

        // Second call should NOT execute the function
        once.call(|| {
            executed.fetch_add(1, SyncOrdering::SeqCst);
        });

        assert_eq!(executed.load(SyncOrdering::SeqCst), 1); // still 1

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_once_multithreaded() {
        let name = "/test_step10_once_mt";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let once = Arc::new(Once::new(&memory, "mt_once").expect("create should succeed"));

        let executed: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

        let num_threads = 4;
        let mut handles = Vec::new();

        for _ in 0..num_threads {
            let once = Arc::clone(&once);
            let executed = Arc::clone(&executed);
            let flag = Arc::clone(&flag);
            let handle = thread::spawn(move || {
                once.call(|| {
                    executed.fetch_add(1, SyncOrdering::SeqCst);
                    flag.store(true, SyncOrdering::SeqCst);
                });
                // After call returns, flag must be true (either we set it or waited for others)
                assert!(flag.load(SyncOrdering::SeqCst));
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        // Exactly one thread should have executed the function
        assert_eq!(executed.load(SyncOrdering::SeqCst), 1);
        assert!(once.is_called());

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_once_open_persistence() {
        let name = "/test_step10_once_persist";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create Once and execute
        {
            let once = Once::new(&memory, "persist_once").expect("create should succeed");
            once.call(|| {});
            assert!(once.is_called());
        }

        // Reopen and verify it's already called
        let memory2 = Memory::open(name, 64).expect("reopen should succeed");
        let once = Once::open(&memory2, "persist_once").expect("open should succeed");
        assert!(once.is_called());

        // Call should not execute
        let executed = Arc::new(AtomicUsize::new(0));
        once.call(|| {
            executed.fetch_add(1, SyncOrdering::SeqCst);
        });
        assert_eq!(executed.load(SyncOrdering::SeqCst), 0);

        // Clean up
        memory2.unlink().expect("unlink should succeed");
        drop(memory2);
        drop(memory);
    }

    #[test]
    fn test_once_not_found() {
        let name = "/test_step10_once_notfound";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let result = Once::open(&memory, "nonexistent_once");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(msg) => {
                assert!(msg.contains("nonexistent_once"));
            }
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_once_duplicate_name() {
        let name = "/test_step10_once_dup";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let _once = Once::new(&memory, "dup_once").expect("create should succeed");

        let result = Once::new(&memory, "dup_once");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(msg) => {
                assert!(msg.contains("dup_once"));
            }
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }
}

