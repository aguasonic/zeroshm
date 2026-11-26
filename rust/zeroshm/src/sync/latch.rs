//! Lock-free one-time countdown latch for cross-process synchronization.
//!
//! Provides the `Latch` type for countdown coordination. A latch counts down
//! from an initial value to zero. Once it reaches zero, it stays at zero
//! (one-time use). Any number of processes can wait for the count to reach
//! zero, and any process can count down to decrement it.
//!
//! # Binary Layout
//! ```text
//! [count: i32][initial_count: i32][padding: i32][padding: i32]
//! ```
//!
//! - count: atomic i32 at offset 0 -- current count (counts down to zero)
//! - initial_count: i32 at offset 4 -- initial count value (immutable)
//! - padding: i32 at offset 8 -- alignment padding
//! - padding: i32 at offset 12 -- alignment padding
//!
//! # Difference from Barrier
//! - Latch: Counts down to 0 and stays there (one-time), any number of waiters
//! - Barrier: Cycles through generations (reusable), exactly N participants required
//!
//! # Algorithm
//! 1. Initialize with `count = initial_count`
//! 2. Workers call `count_down()` to atomically decrement count (CAS loop, saturate at 0)
//! 3. Waiters call `wait()` and spin-wait until count == 0
//! 4. Once count reaches 0, all current and future waiters immediately proceed

use crate::error::{Result, ZeroShmError};
use crate::layout::{LATCH_INITIAL_COUNT_OFFSET, LATCH_PADDING0_OFFSET, LATCH_PADDING1_OFFSET};
use crate::memory::Memory;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Size of the latch header in bytes.
const LATCH_HEADER_SIZE: usize = 16;

/// A lock-free one-time countdown latch stored in shared memory.
///
/// Counts down from an initial value to zero. Once it reaches zero, it stays
/// at zero (one-time use). Useful for start gates (count=1) or completion
/// detection (count=N, each worker counts down when done).
pub struct Latch {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    initial_count: i32,
}

impl std::fmt::Debug for Latch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Latch")
            .field("name", &self.name)
            .field("initial_count", &self.initial_count)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Latch holds an Arc<Memory> which is Send + Sync. The count field
// is atomic, making concurrent access safe.
unsafe impl Send for Latch {}
unsafe impl Sync for Latch {}

impl Latch {
    /// Create a new latch in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the latch (stored in the table)
    /// * `initial_count` -- Initial count value (must be non-negative)
    pub fn new(memory: &Memory, name: &str, initial_count: i32) -> Result<Self> {
        if initial_count < 0 {
            return Err(ZeroShmError::SizeError(
                "initial count must be non-negative".to_string(),
            ));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Allocate space
        let offset = memory.allocate(name, LATCH_HEADER_SIZE)? as usize;

        // Write header: count, initial_count, padding=0, padding=0
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicI32>()).store(initial_count, Ordering::Relaxed);
            std::ptr::write(base.add(LATCH_INITIAL_COUNT_OFFSET).cast::<i32>(), initial_count);
            std::ptr::write(base.add(LATCH_PADDING0_OFFSET).cast::<i32>(), 0);
            std::ptr::write(base.add(LATCH_PADDING1_OFFSET).cast::<i32>(), 0);
        }

        Ok(Latch {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            initial_count,
        })
    }

    /// Open an existing latch in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the latch to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "latch '{}' not found",
                    name
                )))
            }
        };

        if size != LATCH_HEADER_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid latch size: {}, expected {}",
                size, LATCH_HEADER_SIZE
            )));
        }

        let offset = offset as usize;
        let initial_count = unsafe { *memory.at(offset + 4).cast::<i32>() };

        Ok(Latch {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            initial_count,
        })
    }

    /// Decrement the count by `n` (default 1).
    ///
    /// Saturates at 0 (never goes negative). Returns `Ok(())` on success.
    ///
    /// # Arguments
    /// * `n` -- Amount to decrement (must be positive)
    pub fn count_down(&self, n: i32) -> Result<()> {
        if n <= 0 {
            return Err(ZeroShmError::SizeError(
                "count_down amount must be positive".to_string(),
            ));
        }

        let count = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };

        loop {
            let current = count.load(Ordering::Acquire);

            if current <= 0 {
                return Ok(()); // Already at 0
            }

            let new_count = if current < n { 0 } else { current - n };

            if count.compare_exchange_weak(current, new_count, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return Ok(());
            }
            // CAS failed, retry
        }
    }

    /// Decrement the count by 1.
    pub fn count_down_one(&self) -> Result<()> {
        self.count_down(1)
    }

    /// Wait for the count to reach zero.
    ///
    /// Blocks until count reaches 0. If already at 0, returns immediately.
    pub fn wait(&self) -> Result<()> {
        let count = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };

        let mut backoff = Duration::from_micros(1);
        let max_backoff = Duration::from_millis(1);

        while count.load(Ordering::Acquire) > 0 {
            thread::sleep(backoff);
            if backoff < max_backoff {
                backoff *= 2;
            }
        }

        Ok(())
    }

    /// Check if the count has reached zero without blocking.
    ///
    /// Returns `true` if count is 0, `false` otherwise.
    pub fn try_wait(&self) -> bool {
        let count = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        count.load(Ordering::Acquire) == 0
    }

    /// Wait for the count to reach zero with a timeout.
    ///
    /// Returns `Ok(true)` if count reached 0, `Ok(false)` if timed out.
    pub fn wait_timeout(&self, timeout: Duration) -> Result<bool> {
        let deadline = std::time::Instant::now() + timeout;

        let count = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };

        let mut backoff = Duration::from_micros(1);
        let max_backoff = Duration::from_millis(1);

        while count.load(Ordering::Acquire) > 0 {
            if std::time::Instant::now() > deadline {
                return Ok(false);
            }

            thread::sleep(backoff);
            if backoff < max_backoff {
                backoff *= 2;
            }
        }

        Ok(true)
    }

    /// Get the current count value.
    pub fn count(&self) -> i32 {
        let count = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        count.load(Ordering::Acquire)
    }

    /// Get the initial count value.
    pub fn initial_count(&self) -> i32 {
        self.initial_count
    }

    /// Get the latch name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Get the offset of the latch in shared memory.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Reset the latch to its initial count.
    ///
    /// # Warning
    /// This is NOT thread-safe if other threads/processes are waiting or
    /// counting down. Only use with exclusive access.
    pub fn reset(&self) {
        let count = unsafe { self.memory.at(self.offset).cast::<AtomicI32>().as_ref().unwrap() };
        count.store(self.initial_count, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Barrier;
    use crate::memory::Memory;
    use std::sync::Arc;
    use std::thread;
    use crate::platform;
    use std::time::Duration;

    fn setup_memory(suffix: &str) -> (Memory, String) {
        let name = format!("test_latch_{}_{}", suffix, std::process::id());
        let _ = platform::unlink_shm(&name);
        let mem = Memory::create(&name, 64 * 1024, 64).expect("failed to create memory");
        (mem, name)
    }

    fn cleanup_memory(mem: &Memory, name: &str) {
        let _ = mem.unlink();
        let _ = Memory::open(name, 64).map(|m| m.unlink());
    }

    #[test]
    fn test_latch_basic() {
        let (mem, name) = setup_memory("basic");
        let latch = Latch::new(&mem, "latch", 4).expect("failed to create latch");

        assert_eq!(latch.initial_count(), 4);
        assert_eq!(latch.count(), 4);
        assert!(!latch.try_wait());

        // Count down one by one
        latch.count_down_one().expect("count_down failed");
        assert_eq!(latch.count(), 3);

        latch.count_down_one().expect("count_down failed");
        assert_eq!(latch.count(), 2);

        latch.count_down_one().expect("count_down failed");
        assert_eq!(latch.count(), 1);

        latch.count_down_one().expect("count_down failed");
        assert_eq!(latch.count(), 0);
        assert!(latch.try_wait());

        // Counting down below 0 saturates at 0
        latch.count_down_one().expect("count_down failed");
        assert_eq!(latch.count(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_count_down_multi() {
        let (mem, name) = setup_memory("multi");
        let latch = Latch::new(&mem, "latch", 5).expect("failed to create latch");

        // Count down by 3
        latch.count_down(3).expect("count_down failed");
        assert_eq!(latch.count(), 2);

        // Count down by 5 (saturates at 0)
        latch.count_down(5).expect("count_down failed");
        assert_eq!(latch.count(), 0);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_multithreaded() {
        let (mem, name) = setup_memory("multithreaded");
        let latch = Arc::new(Latch::new(&mem, "latch", 4).expect("failed to create latch"));

        // Spawn 4 workers that each count down once
        let mut handles = Vec::new();
        for _ in 0..4 {
            let latch2 = Arc::clone(&latch);
            handles.push(thread::spawn(move || {
                // Simulate work
                thread::sleep(Duration::from_millis(10));
                latch2.count_down_one().expect("count_down failed");
            }));
        }

        // Main thread waits for all workers to complete
        latch.wait().expect("wait failed");
        assert_eq!(latch.count(), 0);

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_start_gate() {
        let (mem, name) = setup_memory("startgate");
        // Start gate: count=1, workers wait, coordinator counts down
        let latch = Arc::new(Latch::new(&mem, "gate", 1).expect("failed to create latch"));

        // Spawn workers that wait for the gate
        // Barrier needs 4 participants: 3 workers + 1 main thread
        let barrier = Arc::new(Barrier::new(&mem, "barrier", 4).expect("failed to create barrier"));
        let mut handles = Vec::new();

        for _ in 0..3 {
            let latch2 = Arc::clone(&latch);
            let barrier2 = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                // Wait at barrier to ensure all workers are ready before gate is released
                barrier2.wait().expect("barrier wait failed");
                // Now wait for the gate
                latch2.wait().expect("latch wait failed");
            }));
        }

        // All workers are at the barrier now; main thread passes the barrier
        barrier.wait().expect("barrier wait failed");

        // Workers are now waiting on the latch. Release the gate.
        latch.count_down_one().expect("count_down failed");

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_timeout() {
        let (mem, name) = setup_memory("timeout");
        let latch = Latch::new(&mem, "latch", 2).expect("failed to create latch");

        // Wait with timeout (count is 2, so it will time out)
        let result = latch
            .wait_timeout(Duration::from_millis(50))
            .expect("wait_timeout failed");
        assert!(!result, "expected timeout");

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_open_persistence() {
        let (mem, name) = setup_memory("persistence");
        let latch = Latch::new(&mem, "latch", 3).expect("failed to create latch");
        latch.count_down_one().expect("count_down failed");
        assert_eq!(latch.count(), 2);

        // Drop and reopen
        drop(latch);
        let latch2 = Latch::open(&mem, "latch").expect("failed to open latch");
        assert_eq!(latch2.count(), 2);
        assert_eq!(latch2.initial_count(), 3);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_reset() {
        let (mem, name) = setup_memory("reset");
        let latch = Latch::new(&mem, "latch", 3).expect("failed to create latch");
        latch.count_down(3).expect("count_down failed");
        assert_eq!(latch.count(), 0);

        // Reset (only safe with exclusive access)
        latch.reset();
        assert_eq!(latch.count(), 3);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_not_found() {
        let (mem, name) = setup_memory("notfound");

        let result = Latch::open(&mem, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(_) => {}
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_duplicate_name() {
        let (mem, name) = setup_memory("duplicate");

        Latch::new(&mem, "latch", 4).expect("failed to create latch");
        let result = Latch::new(&mem, "latch", 4);
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(_) => {}
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_latch_invalid_count() {
        let (mem, name) = setup_memory("invalid");

        let result = Latch::new(&mem, "latch", -1);
        assert!(result.is_err());

        cleanup_memory(&mem, &name);
    }
}

