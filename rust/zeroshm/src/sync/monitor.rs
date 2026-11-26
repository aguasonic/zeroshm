//! Monitor for condition variable synchronization across processes.
//!
//! Provides the `Monitor` type that combines a mutex with condition variable
//! semantics. Allows threads/processes to wait for conditions to become true.
//! Similar to `std::sync::Condvar` but works across shared memory.
//!
//! # Binary Layout
//! Plus additional table entries:
//! - `name_mtx`: Mutex for lock/unlock (16 bytes)
//! - `name_cond`: Semaphore for condition signaling (16 bytes)
//! - `name_count`: Atomic u32 waiting counter (4 bytes)
//! - `name`: 1-byte marker entry
//!
//! # Operations
//! - `lock()` / `unlock()`: Mutex operations
//! - `wait(predicate)`: Atomically unlock mutex, wait for signal, re-lock
//! - `notify_one()`: Wake one waiting process
//! - `notify_all()`: Wake all waiting processes
//!
//! # Algorithm
//! - `wait()`: Increment waiting_count, unlock mutex, acquire cond_sem, lock mutex, decrement waiting_count
//! - `notify_one()`: Release cond_sem if waiters exist
//! - `notify_all()`: Release cond_sem for each waiter
//!
//! # Usage
//! - Producer-consumer patterns
//! - Predicate-based waiting (spurious wakeup handling)
//! - Bounded buffer synchronization

use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use crate::sync::mutex::Mutex;
use crate::sync::semaphore::Semaphore;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Size of the waiting counter in bytes.
const MONITOR_COUNTER_SIZE: usize = 4;

/// Size of the marker entry in bytes.
const MONITOR_MARKER_SIZE: usize = 1;

/// A monitor for condition variable synchronization stored in shared memory.
///
/// Combines a mutex with condition variable semantics, allowing
/// threads/processes to wait for conditions to become true.
pub struct Monitor {
    memory: Arc<Memory>,
    name: String,
    mutex: Arc<Mutex>,
    cond_sem: Arc<Semaphore>,
    waiting_offset: usize,
}

impl std::fmt::Debug for Monitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Monitor")
            .field("name", &self.name)
            .finish()
    }
}

// SAFETY: Monitor holds Arc<Memory>, Arc<Mutex>, and Arc<Semaphore> which are
// Send + Sync. The waiting counter is atomic, making concurrent access safe.
unsafe impl Send for Monitor {}
unsafe impl Sync for Monitor {}

/// RAII guard that releases the monitor lock on drop.
pub struct MonitorGuard {
    monitor: Arc<Monitor>,
}

impl std::fmt::Debug for MonitorGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitorGuard")
            .field("name", &self.monitor.name)
            .finish()
    }
}

unsafe impl Send for MonitorGuard {}
unsafe impl Sync for MonitorGuard {}

impl Drop for MonitorGuard {
    fn drop(&mut self) {
        self.monitor.release_lock().ok();
    }
}

impl Monitor {
    /// Create a new monitor in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the monitor (stored in the table as a marker)
    pub fn new(memory: &Memory, name: &str) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Create mutex
        let mutex = Arc::new(Mutex::new(memory, &format!("{}_mtx", name))?);

        // Create semaphore for condition signaling (initially 0 permits)
        // max_count=0 = unbounded, allows multiple pending signals (notify_all releases per waiter)
        let cond_sem = Arc::new(Semaphore::new(
            memory,
            &format!("{}_cond", name),
            0,
            0,
        )?);

        // Create waiting counter
        let waiting_offset = memory.allocate(&format!("{}_count", name), MONITOR_COUNTER_SIZE)? as usize;
        unsafe {
            (*memory.at_mut(waiting_offset).cast::<AtomicU32>()).store(0, Ordering::Relaxed);
        }

        // Add marker entry
        memory.allocate(name, MONITOR_MARKER_SIZE)?;

        Ok(Monitor {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            mutex,
            cond_sem,
            waiting_offset,
        })
    }

    /// Open an existing monitor in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the monitor to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        // Check if marker exists
        if memory.find(name)?.is_none() {
            return Err(ZeroShmError::NotFoundError(format!(
                "monitor '{}' not found",
                name
            )));
        }

        // Open mutex
        let mutex = Arc::new(Mutex::open(memory, &format!("{}_mtx", name))?);

        // Open semaphore
        let cond_sem = Arc::new(Semaphore::open(memory, &format!("{}_cond", name))?);

        // Find waiting counter
        let (waiting_offset, size) = match memory.find(&format!("{}_count", name))? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "monitor counter for '{}' not found",
                    name
                )))
            }
        };

        if size != MONITOR_COUNTER_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid monitor counter size: {}, expected {}",
                size, MONITOR_COUNTER_SIZE
            )));
        }

        Ok(Monitor {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            mutex,
            cond_sem,
            waiting_offset: waiting_offset as usize,
        })
    }

    /// Lock the monitor's mutex.
    ///
    /// Returns a `MonitorGuard` that releases the lock on drop.
    pub fn lock(&self) -> Result<MonitorGuard> {
        self.mutex.acquire()?;
        Ok(MonitorGuard {
            monitor: Arc::new(Monitor {
                memory: Arc::clone(&self.memory),
                name: self.name.clone(),
                mutex: Arc::clone(&self.mutex),
                cond_sem: Arc::clone(&self.cond_sem),
                waiting_offset: self.waiting_offset,
            }),
        })
    }

    /// Try to lock the monitor's mutex without blocking.
    ///
    /// Returns `Ok(Some(guard))` if the lock was acquired, `Ok(None)` otherwise.
    pub fn try_lock(&self) -> Result<Option<MonitorGuard>> {
        match self.mutex.try_acquire()? {
            true => Ok(Some(MonitorGuard {
                monitor: Arc::new(Monitor {
                    memory: Arc::clone(&self.memory),
                    name: self.name.clone(),
                    mutex: Arc::clone(&self.mutex),
                    cond_sem: Arc::clone(&self.cond_sem),
                    waiting_offset: self.waiting_offset,
                }),
            })),
            false => Ok(None),
        }
    }

    /// Release the monitor's lock (for use by composite primitives and guards).
    pub fn release_lock(&self) -> Result<()> {
        self.mutex.release()
    }

    /// Wait for notification, atomically releasing the lock.
    ///
    /// Must hold the lock before calling wait(). When woken, reacquires the lock.
    ///
    /// # Warning
    /// Spurious wakeups can occur -- use `wait_predicate` for safety.
    pub fn wait(&self) -> Result<()> {
        let waiting = unsafe {
            self.memory
                .at(self.waiting_offset)
                .cast::<AtomicU32>()
                .as_ref()
                .unwrap()
        };

        // Increment waiting count
        waiting.fetch_add(1, Ordering::Release);

        // Release lock and wait on condition semaphore
        self.mutex.release()?;
        self.cond_sem.acquire()?;

        // Reacquire lock
        self.mutex.acquire()?;

        // Decrement waiting count AFTER reacquiring lock
        waiting.fetch_sub(1, Ordering::Release);

        Ok(())
    }

    /// Wait for notification with a predicate (handles spurious wakeups).
    ///
    /// Must hold the lock before calling. Repeatedly waits until the predicate
    /// returns true. This is the recommended way to use wait().
    ///
    /// # Arguments
    /// * `predicate` -- Callable that returns bool
    pub fn wait_predicate<F>(&self, mut predicate: F) -> Result<()>
    where
        F: FnMut() -> bool,
    {
        while !predicate() {
            self.wait()?;
        }
        Ok(())
    }

    /// Wait for notification with a timeout and predicate.
    ///
    /// # Arguments
    /// * `timeout` -- Maximum time to wait
    /// * `predicate` -- Callable that returns bool
    ///
    /// Returns `Ok(true)` if predicate became true, `Ok(false)` if timed out.
    pub fn wait_predicate_timeout<F>(&self, timeout: Duration, mut predicate: F) -> Result<bool>
    where
        F: FnMut() -> bool,
    {
        let deadline = std::time::Instant::now() + timeout;

        // Get waiting counter
        let waiting = unsafe {
            self.memory
                .at(self.waiting_offset)
                .cast::<AtomicU32>()
                .as_ref()
                .unwrap()
        };

        while !predicate() {
            let now = std::time::Instant::now();
            if now > deadline {
                return Ok(false); // Timeout
            }

            let remaining = deadline.saturating_duration_since(now);

            // Increment waiting count
            waiting.fetch_add(1, Ordering::Release);

            // Release lock and wait with timeout
            self.mutex.release()?;

            // Try to acquire with timeout
            let acquired = self.cond_sem.try_acquire_timeout(remaining)?;

            // Reacquire lock
            self.mutex.acquire()?;

            // Decrement waiting count
            waiting.fetch_sub(1, Ordering::Release);

            if !acquired {
                // Timeout, but check predicate one last time
                return Ok(predicate());
            }
        }

        Ok(true)
    }

    /// Wake one waiting thread/process.
    ///
    /// If multiple threads are waiting, wakes one arbitrarily.
    /// If no threads are waiting, the signal is ignored (not queued).
    pub fn notify_one(&self) -> Result<()> {
        // Only release if someone is actually waiting
        let waiting = unsafe {
            self.memory
                .at(self.waiting_offset)
                .cast::<AtomicU32>()
                .as_ref()
                .unwrap()
        };

        if self.cond_sem.waiting() > 0 || waiting.load(Ordering::Acquire) > 0 {
            self.cond_sem.release()?;
        }

        Ok(())
    }

    /// Wake all waiting threads/processes.
    ///
    /// Wakes all currently waiting threads. New waiters are not affected.
    pub fn notify_all(&self) -> Result<()> {
        let waiting = unsafe {
            self.memory
                .at(self.waiting_offset)
                .cast::<AtomicU32>()
                .as_ref()
                .unwrap()
        };

        let count = waiting.load(Ordering::Acquire);
        for _ in 0..count {
            self.cond_sem.release()?;
        }

        Ok(())
    }

    /// Get the number of waiting threads.
    pub fn waiting_count(&self) -> u32 {
        let waiting = unsafe {
            self.memory
                .at(self.waiting_offset)
                .cast::<AtomicU32>()
                .as_ref()
                .unwrap()
        };
        waiting.load(Ordering::Acquire)
    }

    /// Get the monitor name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::Array;
    use crate::memory::Memory;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn setup_memory() -> (Memory, String) {
        let name = format!("test_monitor_{}", std::process::id());
        let mem = Memory::create(&name, 64 * 1024, 64).expect("failed to create memory");
        (mem, name)
    }

    fn cleanup_memory(mem: &Memory, name: &str) {
        let _ = mem.unlink();
        let _ = Memory::open(name, 64).map(|m| m.unlink());
    }

    #[test]
    fn test_monitor_basic_notify() {
        let (mem, name) = setup_memory();
        let monitor = Arc::new(Monitor::new(&mem, "monitor").expect("failed to create monitor"));

        // Spawn a thread that waits for notification
        let monitor2 = Arc::clone(&monitor);
        let handle = thread::spawn(move || {
            let guard = monitor2.lock().expect("lock failed");
            // Wait for notification (releases lock, waits, reacquires lock)
            monitor2.wait().expect("wait failed");
            drop(guard);
        });

        // Give the thread time to start waiting (it releases the lock in wait())
        thread::sleep(Duration::from_millis(10));

        // Now acquire the lock (available because waiting thread released it)
        let guard = monitor.lock().expect("lock failed");

        // Notify the waiter
        monitor.notify_one().expect("notify failed");

        // Release the lock so the waiting thread can reacquire it
        drop(guard);

        handle.join().expect("thread panicked");

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_notify_all() {
        let (mem, name) = setup_memory();
        let monitor = Arc::new(Monitor::new(&mem, "monitor").expect("failed to create monitor"));

        // Spawn multiple threads that wait for notification
        let mut handles = Vec::new();
        for _ in 0..4 {
            let monitor2 = Arc::clone(&monitor);
            handles.push(thread::spawn(move || {
                let guard = monitor2.lock().expect("lock failed");
                monitor2.wait().expect("wait failed");
                drop(guard);
            }));
        }

        // Give threads time to start waiting (they release the lock in wait())
        thread::sleep(Duration::from_millis(20));

        // Acquire the lock (available because waiting threads released it)
        let guard = monitor.lock().expect("lock failed");

        // Notify all waiters
        monitor.notify_all().expect("notify_all failed");

        // Release the lock so waiting threads can reacquire it
        drop(guard);

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_predicate() {
        let (mem, name) = setup_memory();
        let monitor = Arc::new(Monitor::new(&mem, "monitor").expect("failed to create monitor"));

        // Create a shared counter
        let counter = Arc::new(Array::new(&mem, "counter", 1).expect("failed to create array"));
        counter.set(0, 0i32);

        // Spawn a thread that waits for the predicate
        let monitor2 = Arc::clone(&monitor);
        let counter2 = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            let guard = monitor2.lock().expect("lock failed");
            monitor2
                .wait_predicate(|| counter2.get(0) > 0)
                .expect("wait_predicate failed");
            drop(guard);
        });

        // Give the thread time to start waiting (it releases the lock in wait())
        thread::sleep(Duration::from_millis(10));

        // Acquire the lock, update the counter, and notify
        {
            let _guard = monitor.lock().expect("lock failed");
            counter.set(0, 1);
            monitor.notify_all().expect("notify failed");
        }

        handle.join().expect("thread panicked");
        assert_eq!(counter.get(0), 1);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_timeout() {
        let (mem, name) = setup_memory();
        let monitor = Monitor::new(&mem, "monitor").expect("failed to create monitor");

        // Lock and wait with timeout (no notification will come)
        let guard = monitor.lock().expect("lock failed");

        let result = monitor
            .wait_predicate_timeout(Duration::from_millis(50), || false)
            .expect("wait_predicate_timeout failed");
        assert!(!result, "expected timeout");

        drop(guard);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_open_persistence() {
        let (mem, name) = setup_memory();
        let monitor = Monitor::new(&mem, "monitor").expect("failed to create monitor");

        // Drop and reopen
        drop(monitor);
        let monitor2 = Monitor::open(&mem, "monitor").expect("failed to open monitor");
        assert_eq!(monitor2.name(), "monitor");

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_not_found() {
        let (mem, name) = setup_memory();

        let result = Monitor::open(&mem, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(_) => {}
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_duplicate_name() {
        let (mem, name) = setup_memory();

        Monitor::new(&mem, "monitor").expect("failed to create monitor");
        let result = Monitor::new(&mem, "monitor");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(_) => {}
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_monitor_guard_raii() {
        let (mem, name) = setup_memory();
        let monitor = Monitor::new(&mem, "monitor").expect("failed to create monitor");

        assert!(!monitor.mutex.is_locked());

        {
            let _guard = monitor.lock().expect("lock failed");
            assert!(monitor.mutex.is_locked());
        }

        // Guard dropped, mutex should be unlocked
        assert!(!monitor.mutex.is_locked());

        // Lock again to verify it works after release
        {
            let _guard = monitor.lock().expect("lock failed again");
            assert!(monitor.mutex.is_locked());
        }
        assert!(!monitor.mutex.is_locked());

        cleanup_memory(&mem, &name);
    }
}

