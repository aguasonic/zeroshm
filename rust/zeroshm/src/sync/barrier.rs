//! Lock-free barrier for cross-process synchronization.
//!
//! Provides the `Barrier` type for synchronizing N processes at a checkpoint.
//! All participants must call `wait()` before any can proceed. Once all arrive,
//! the barrier releases all waiters simultaneously and automatically resets
//! for the next cycle.
//!
//! # Binary Layout
//! ```text
//! [arrived: i32][generation: i32][num_participants: i32][padding: i32]
//! ```
//!
//! - arrived: atomic i32 at offset 0 -- number of processes that have arrived
//! - generation: atomic i32 at offset 4 -- generation counter (incremented on each pass)
//! - num_participants: i32 at offset 8 -- total participants required
//! - padding: i32 at offset 12 -- alignment padding
//!
//! # Algorithm
//! 1. Process calls `wait()` and captures the current generation
//! 2. Atomically increments `arrived` counter
//! 3. If this was the last arrival (`arrived == num_participants`):
//!    reset `arrived` to 0 and increment `generation`
//! 4. Otherwise: spin-wait until `generation` changes

use crate::error::{Result, ZeroShmError};
use crate::layout::{BARRIER_GENERATION_OFFSET, BARRIER_NUM_PARTICIPANTS_OFFSET, BARRIER_PADDING_OFFSET};
use crate::memory::Memory;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Size of the barrier header in bytes.
const BARRIER_HEADER_SIZE: usize = 16;

/// A lock-free barrier stored in shared memory.
///
/// Synchronizes N processes at a checkpoint. All participants must reach
/// the barrier before any can proceed. The barrier automatically resets
/// after all participants pass through, allowing it to be reused.
pub struct Barrier {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    num_participants: i32,
}

impl std::fmt::Debug for Barrier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Barrier")
            .field("name", &self.name)
            .field("num_participants", &self.num_participants)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Barrier holds an Arc<Memory> which is Send + Sync. The arrived
// and generation fields are atomic, making concurrent access safe.
unsafe impl Send for Barrier {}
unsafe impl Sync for Barrier {}

impl Barrier {
    /// Create a new barrier in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the barrier (stored in the table)
    /// * `num_participants` -- Number of processes that must arrive before release (must be > 0)
    pub fn new(memory: &Memory, name: &str, num_participants: i32) -> Result<Self> {
        if num_participants <= 0 {
            return Err(ZeroShmError::SizeError(
                "number of participants must be positive".to_string(),
            ));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Allocate space
        let offset = memory.allocate(name, BARRIER_HEADER_SIZE)? as usize;

        // Write header: arrived=0, generation=0, num_participants, padding=0
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicI32>()).store(0, Ordering::Relaxed);
            base.add(BARRIER_GENERATION_OFFSET)
                .cast::<AtomicI32>()
                .as_ref()
                .unwrap()
                .store(0, Ordering::Relaxed);
            std::ptr::write(base.add(BARRIER_NUM_PARTICIPANTS_OFFSET).cast::<i32>(), num_participants);
            std::ptr::write(base.add(BARRIER_PADDING_OFFSET).cast::<i32>(), 0);
        }

        Ok(Barrier {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            num_participants,
        })
    }

    /// Open an existing barrier in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the barrier to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "barrier '{}' not found",
                    name
                )))
            }
        };

        if size != BARRIER_HEADER_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid barrier size: {}, expected {}",
                size, BARRIER_HEADER_SIZE
            )));
        }

        let offset = offset as usize;
        let num_participants = unsafe { *memory.at(offset + 8).cast::<i32>() };

        Ok(Barrier {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            num_participants,
        })
    }

    /// Wait for all participants to arrive at the barrier.
    ///
    /// Blocks until all `num_participants` processes have called `wait()`.
    /// Once all arrive, all waiters are released and the barrier resets
    /// for the next cycle.
    pub fn wait(&self) -> Result<()> {
        // Get atomic pointers to the barrier fields
        let arrived = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        let generation = unsafe {
            self.memory
                .at(self.offset + 4)
                .cast::<AtomicI32>()
                .as_ref()
                .unwrap()
        };

        // Capture current generation before arriving
        let my_generation = generation.load(Ordering::Acquire);

        // Atomically increment arrived counter
        let arrived_count = arrived.fetch_add(1, Ordering::AcqRel);

        if arrived_count + 1 == self.num_participants {
            // Last to arrive -- reset and release everyone
            arrived.store(0, Ordering::Release);
            generation.fetch_add(1, Ordering::Release);
        } else {
            // Not last -- wait for generation to change
            let mut backoff = Duration::from_micros(1);
            let max_backoff = Duration::from_millis(1);

            while generation.load(Ordering::Acquire) == my_generation {
                thread::sleep(backoff);
                if backoff < max_backoff {
                    backoff *= 2;
                }
            }
        }

        Ok(())
    }

    /// Wait for all participants with a timeout.
    ///
    /// Returns `Ok(true)` if the barrier released, `Ok(false)` if timed out.
    ///
    /// # Warning
    /// If a timeout occurs, the barrier state may be inconsistent.
    /// The `arrived` counter is decremented on timeout to attempt recovery.
    pub fn wait_timeout(&self, timeout: Duration) -> Result<bool> {
        let deadline = std::time::Instant::now() + timeout;

        let arrived = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        let generation = unsafe {
            self.memory
                .at(self.offset + 4)
                .cast::<AtomicI32>()
                .as_ref()
                .unwrap()
        };

        // Capture current generation before arriving
        let my_generation = generation.load(Ordering::Acquire);

        // Atomically increment arrived counter
        let arrived_count = arrived.fetch_add(1, Ordering::AcqRel);

        if arrived_count + 1 == self.num_participants {
            // Last to arrive -- reset and release everyone
            arrived.store(0, Ordering::Release);
            generation.fetch_add(1, Ordering::Release);
            return Ok(true);
        }

        // Not last -- wait for generation to change or timeout
        let mut backoff = Duration::from_micros(1);
        let max_backoff = Duration::from_millis(1);

        while generation.load(Ordering::Acquire) == my_generation {
            if std::time::Instant::now() > deadline {
                // Timeout -- decrement arrived count to attempt recovery
                arrived.fetch_sub(1, Ordering::AcqRel);
                return Ok(false);
            }

            thread::sleep(backoff);
            if backoff < max_backoff {
                backoff *= 2;
            }
        }

        Ok(true)
    }

    /// Get the number of processes currently waiting at the barrier.
    pub fn arrived(&self) -> i32 {
        let arrived = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        arrived.load(Ordering::Acquire)
    }

    /// Get the current generation number.
    /// The generation increments each time all participants pass through.
    pub fn generation(&self) -> i32 {
        let generation = unsafe {
            self.memory
                .at(self.offset + 4)
                .cast::<AtomicI32>()
                .as_ref()
                .unwrap()
        };
        generation.load(Ordering::Acquire)
    }

    /// Get the number of participants required.
    pub fn num_participants(&self) -> i32 {
        self.num_participants
    }

    /// Get the barrier name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Get the offset of the barrier in shared memory.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Memory;
    use std::sync::Arc;
    use crate::platform;
    use std::thread;

    fn setup_memory(suffix: &str) -> (Memory, String) {
        let name = format!("test_barrier_{}_{}", suffix, std::process::id());
        let _ = platform::unlink_shm(&name);
        let mem = Memory::create(&name, 64 * 1024, 64).expect("failed to create memory");
        (mem, name)
    }

    fn cleanup_memory(mem: &Memory, name: &str) {
        let _ = mem.unlink();
        let _ = Memory::open(name, 64).map(|m| m.unlink());
    }

    #[test]
    fn test_barrier_basic() {
        let (mem, name) = setup_memory("basic");
        let barrier = Barrier::new(&mem, "barrier", 4).expect("failed to create barrier");

        assert_eq!(barrier.num_participants(), 4);
        assert_eq!(barrier.arrived(), 0);
        assert_eq!(barrier.generation(), 0);

        // Spawn 3 threads that wait at the barrier
        let barrier2 = Arc::new(Barrier::open(&mem, "barrier").expect("failed to open barrier"));
        let barrier3 = Arc::clone(&barrier2);
        let barrier4 = Arc::clone(&barrier2);

        let handle1 = thread::spawn(move || {
            barrier2.wait().expect("wait failed");
        });

        let handle2 = thread::spawn(move || {
            barrier3.wait().expect("wait failed");
        });

        let handle3 = thread::spawn(move || {
            barrier4.wait().expect("wait failed");
        });

        // Give threads time to reach the barrier
        thread::sleep(Duration::from_millis(10));

        // Main thread arrives last -- should release everyone
        barrier.wait().expect("main wait failed");

        handle1.join().expect("thread 1 panicked");
        handle2.join().expect("thread 2 panicked");
        handle3.join().expect("thread 3 panicked");

        // Verify generation incremented and arrived reset
        assert_eq!(barrier.generation(), 1);
        assert_eq!(barrier.arrived(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_barrier_multicycle() {
        let (mem, name) = setup_memory("multicycle");
        let barrier = Arc::new(Barrier::new(&mem, "barrier", 2).expect("failed to create barrier"));

        // Run 100 cycles with 2 participants
        let barrier2 = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            for i in 0..100 {
                // Small stagger to ensure we're not perfectly synchronized
                thread::sleep(Duration::from_micros(i as u64 % 10));
                barrier2.wait().expect("wait failed");
            }
        });

        for _ in 0..100 {
            barrier.wait().expect("main wait failed");
        }

        handle.join().expect("thread panicked");

        // Verify generation is 100 after 100 cycles
        assert_eq!(barrier.generation(), 100);
        assert_eq!(barrier.arrived(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_barrier_timeout() {
        let (mem, name) = setup_memory("timeout");
        // Barrier needs 2 participants, but only 1 arrives  timeout
        let barrier = Barrier::new(&mem, "barrier", 2).expect("failed to create barrier");

        // Main thread is the only participant -- it should time out
        let result = barrier
            .wait_timeout(Duration::from_millis(50))
            .expect("wait_timeout failed");
        assert!(!result, "expected timeout");

        // After timeout, arrived should be reset to 0
        assert_eq!(barrier.arrived(), 0);
        // Generation should not have changed (barrier never completed)
        assert_eq!(barrier.generation(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_barrier_open_persistence() {
        let (mem, name) = setup_memory("persistence");
        let barrier = Barrier::new(&mem, "barrier", 3).expect("failed to create barrier");
        assert_eq!(barrier.generation(), 0);

        // Drop and reopen
        drop(barrier);
        let barrier2 = Barrier::open(&mem, "barrier").expect("failed to open barrier");
        assert_eq!(barrier2.num_participants(), 3);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_barrier_not_found() {
        let (mem, name) = setup_memory("notfound");

        let result = Barrier::open(&mem, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(_) => {}
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_barrier_duplicate_name() {
        let (mem, name) = setup_memory("duplicate");

        Barrier::new(&mem, "barrier", 4).expect("failed to create barrier");
        let result = Barrier::new(&mem, "barrier", 4);
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(_) => {}
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_barrier_invalid_participants() {
        let (mem, name) = setup_memory("invalid");

        let result = Barrier::new(&mem, "barrier", 0);
        assert!(result.is_err());

        let result = Barrier::new(&mem, "barrier", -1);
        assert!(result.is_err());

        cleanup_memory(&mem, &name);
    }
}

