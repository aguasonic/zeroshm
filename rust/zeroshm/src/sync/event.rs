//! Event synchronization primitive for cross-process coordination.
//!
//! Provides the `Event` type with AutoReset and ManualReset modes, similar to
//! Win32 Events. Supports `signal()`, `wait()`, `reset()`, and `pulse()` operations.
//!
//! # Binary Layout
//! ```text
//! [signaled: u32][mode: u32][waiting: u32]
//! ```
//! Plus additional table entry:
//! - `name_sem`: Semaphore for blocking (16 bytes)
//!
//! - signaled: atomic u32 at offset 0 -- 0 = not signaled, 1 = signaled
//! - mode: u32 at offset 4 -- 0 = AutoReset, 1 = ManualReset
//! - waiting: atomic u32 at offset 8 -- number of processes waiting
//!
//! # Modes
//! - **AutoReset**: `signal()` wakes one waiter, auto-clears signaled flag
//! - **ManualReset**: `signal()` wakes all waiters, stays signaled until `reset()`
//!
//! # Algorithm
//! - AutoReset: `signal()` releases semaphore (wakes one waiter); `wait()` acquires semaphore then clears signaled
//! - ManualReset: `signal()` sets signaled=1; `wait()` spin-waits for signaled==1; `pulse()` signals, waits for all waiters to unregister, then resets

use crate::error::{Result, ZeroShmError};
use crate::layout::{EVENT_MODE_OFFSET, EVENT_WAITING_OFFSET};
use crate::memory::Memory;
use crate::sync::semaphore::Semaphore;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Size of the event state in bytes.
const EVENT_STATE_SIZE: usize = 12;

/// Event synchronization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventMode {
    /// Signal wakes one waiter, auto-clears signaled flag.
    AutoReset = 0,
    /// Signal wakes all waiters, stays signaled until reset().
    ManualReset = 1,
}

/// An event synchronization primitive stored in shared memory.
///
/// Provides manual-reset and auto-reset event semantics.
/// - AutoReset: signal() wakes one waiter, then auto-resets
/// - ManualReset: signal() wakes all waiters, stays signaled until reset()
pub struct Event {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    sem: Arc<Semaphore>,
}

impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Event")
            .field("name", &self.name)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Event holds Arc<Memory> and Arc<Semaphore> which are Send + Sync.
// The signaled and waiting fields are atomic, making concurrent access safe.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    /// Create a new event in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the event (stored in the table)
    /// * `mode` -- Event mode (AutoReset or ManualReset)
    pub fn new(memory: &Memory, name: &str, mode: EventMode) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Allocate space for state
        let offset = memory.allocate(name, EVENT_STATE_SIZE)? as usize;

        // Initialize state: signaled=0, mode, waiting=0
        unsafe {
            let base = memory.at_mut(offset);
            (*base.cast::<AtomicU32>()).store(0, Ordering::Relaxed);
            std::ptr::write(
                base.add(EVENT_MODE_OFFSET).cast::<u32>(),
                mode as u32,
            );
            base.add(EVENT_WAITING_OFFSET)
                .cast::<AtomicU32>()
                .as_ref()
                .unwrap()
                .store(0, Ordering::Relaxed);
        }

        // Create semaphore for blocking (initially 0 permits)
        let sem = Arc::new(Semaphore::new(memory, &format!("{}_sem", name), 0, 0)?);

        Ok(Event {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            sem,
        })
    }

    /// Open an existing event in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the event to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "event '{}' not found",
                    name
                )))
            }
        };

        if size != EVENT_STATE_SIZE as u64 {
            return Err(ZeroShmError::SizeError(format!(
                "invalid event size: {}, expected {}",
                size, EVENT_STATE_SIZE
            )));
        }

        // Open semaphore
        let sem = Arc::new(Semaphore::open(memory, &format!("{}_sem", name))?);

        Ok(Event {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: offset as usize,
            sem,
        })
    }

    /// Signal the event.
    ///
    /// - AutoReset: Wakes one waiting thread (semaphore acts as the event)
    /// - ManualReset: Sets signaled flag, stays signaled until reset()
    pub fn signal(&self) -> Result<()> {
        let signaled =
            unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };
        signaled.store(1, Ordering::Release);

        if !self.is_manual_reset() {
            // AutoReset: release semaphore to wake one waiter
            self.sem.release()?;
        }

        Ok(())
    }

    /// Wait for the event to be signaled.
    ///
    /// Blocks until signal() is called. For AutoReset events, consuming
    /// the signal is automatic (semaphore). For ManualReset events, the
    /// event stays signaled until reset() is called.
    pub fn wait(&self) -> Result<()> {
        if self.is_manual_reset() {
            let waiting =
                unsafe { self.memory.at(self.offset + 8).cast::<AtomicU32>().as_ref().unwrap() };
            waiting.fetch_add(1, Ordering::Relaxed);

            let signaled =
                unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };

            let mut backoff = Duration::from_micros(1);
            let max_backoff = Duration::from_millis(1);

            while signaled.load(Ordering::Acquire) == 0 {
                thread::sleep(backoff);
                if backoff < max_backoff {
                    backoff *= 2;
                }
            }

            waiting.fetch_sub(1, Ordering::Release);
        } else {
            // AutoReset: acquire semaphore, then clear signaled flag
            self.sem.acquire()?;
            let signaled =
                unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };
            signaled.store(0, Ordering::Release);
        }

        Ok(())
    }

    /// Wait for the event with a timeout.
    ///
    /// Returns `Ok(true)` if signaled, `Ok(false)` if timed out.
    pub fn wait_timeout(&self, timeout: Duration) -> Result<bool> {
        let deadline = std::time::Instant::now() + timeout;

        if self.is_manual_reset() {
            let waiting =
                unsafe { self.memory.at(self.offset + 8).cast::<AtomicU32>().as_ref().unwrap() };
            waiting.fetch_add(1, Ordering::Relaxed);

            let signaled =
                unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };

            let mut backoff = Duration::from_micros(1);
            let max_backoff = Duration::from_millis(1);

            while signaled.load(Ordering::Acquire) == 0 {
                if std::time::Instant::now() > deadline {
                    waiting.fetch_sub(1, Ordering::Release);
                    return Ok(false);
                }

                thread::sleep(backoff);
                if backoff < max_backoff {
                    backoff *= 2;
                }
            }

            waiting.fetch_sub(1, Ordering::Release);
            Ok(true)
        } else {
            // AutoReset: use semaphore with timeout
            if self.sem.try_acquire_timeout(timeout)? {
                let signaled =
                    unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };
                signaled.store(0, Ordering::Release);
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    /// Reset the event to non-signaled state.
    ///
    /// Only meaningful for ManualReset events. AutoReset events
    /// reset automatically.
    pub fn reset(&self) -> Result<()> {
        let signaled =
            unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };
        signaled.store(0, Ordering::Release);
        Ok(())
    }

    /// Pulse the event (signal then reset after all waiters wake).
    ///
    /// Wakes all waiting threads then resets once they've all seen the signal.
    /// Only meaningful for ManualReset events.
    pub fn pulse(&self) -> Result<()> {
        self.signal()?;

        // Wait for all registered waiters to see the signal and unregister
        let waiting =
            unsafe { self.memory.at(self.offset + 8).cast::<AtomicU32>().as_ref().unwrap() };

        let mut backoff = Duration::from_micros(1);
        let max_backoff = Duration::from_millis(1);

        while waiting.load(Ordering::Acquire) > 0 {
            thread::sleep(backoff);
            if backoff < max_backoff {
                backoff *= 2;
            }
        }

        self.reset()
    }

    /// Check if the event is currently signaled.
    pub fn is_signaled(&self) -> bool {
        let signaled =
            unsafe { self.memory.at(self.offset).cast::<AtomicU32>().as_ref().unwrap() };
        signaled.load(Ordering::Acquire) != 0
    }

    /// Check if the event is in ManualReset mode.
    pub fn is_manual_reset(&self) -> bool {
        let mode = unsafe { *self.memory.at(self.offset + 4).cast::<u32>() };
        mode == EventMode::ManualReset as u32
    }

    /// Get the event name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Get the offset of the event state in shared memory.
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
        let name = format!("test_event_{}_{}", suffix, std::process::id());
        let _ = platform::unlink_shm(&name);
        let mem = Memory::create(&name, 64 * 1024, 64).expect("failed to create memory");
        (mem, name)
    }

    fn cleanup_memory(mem: &Memory, name: &str) {
        let _ = mem.unlink();
        let _ = Memory::open(name, 64).map(|m| m.unlink());
    }

    #[test]
    fn test_event_autoreset() {
        let (mem, name) = setup_memory("autoreset");
        let event = Arc::new(Event::new(&mem, "event", EventMode::AutoReset).expect("failed to create event"));

        assert!(!event.is_signaled());

        // Spawn a thread that waits for the event
        let event2 = Arc::clone(&event);
        let handle = thread::spawn(move || {
            event2.wait().expect("wait failed");
        });

        // Give the thread time to start waiting
        thread::sleep(Duration::from_millis(10));

        // Signal the event
        event.signal().expect("signal failed");

        // The waiter should now be released
        handle.join().expect("thread panicked");

        // AutoReset should have cleared the signaled flag
        assert!(!event.is_signaled());

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_event_manualreset() {
        let (mem, name) = setup_memory("manualreset");
        let event = Arc::new(Event::new(&mem, "event", EventMode::ManualReset).expect("failed to create event"));

        assert!(!event.is_signaled());

        // Spawn multiple threads that wait for the event
        let mut handles = Vec::new();
        for _ in 0..3 {
            let event2 = Arc::clone(&event);
            handles.push(thread::spawn(move || {
                event2.wait().expect("wait failed");
            }));
        }

        // Give threads time to start waiting
        thread::sleep(Duration::from_millis(10));

        // Signal the event -- all waiters should be released
        event.signal().expect("signal failed");
        assert!(event.is_signaled());

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        // ManualReset should stay signaled
        assert!(event.is_signaled());

        // Reset it
        event.reset().expect("reset failed");
        assert!(!event.is_signaled());

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_event_pulse() {
        let (mem, name) = setup_memory("pulse");
        let event = Arc::new(Event::new(&mem, "event", EventMode::ManualReset).expect("failed to create event"));

        // Spawn a thread that waits
        let event2 = Arc::clone(&event);
        let handle = thread::spawn(move || {
            event2.wait().expect("wait failed");
        });

        // Give the thread time to start waiting
        thread::sleep(Duration::from_millis(10));

        // Pulse the event
        event.pulse().expect("pulse failed");

        // The waiter should be released
        handle.join().expect("thread panicked");

        // Pulse should have reset the event
        assert!(!event.is_signaled());

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_event_timeout() {
        let (mem, name) = setup_memory("timeout");
        let event = Event::new(&mem, "event", EventMode::AutoReset).expect("failed to create event");

        // Wait with timeout (event is not signaled, so it will time out)
        let result = event
            .wait_timeout(Duration::from_millis(50))
            .expect("wait_timeout failed");
        assert!(!result, "expected timeout");

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_event_open_persistence() {
        let (mem, name) = setup_memory("persistence");
        let event = Event::new(&mem, "event", EventMode::ManualReset).expect("failed to create event");
        event.signal().expect("signal failed");
        assert!(event.is_signaled());

        // Drop and reopen
        drop(event);
        let event2 = Event::open(&mem, "event").expect("failed to open event");
        assert!(event2.is_signaled());

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_event_not_found() {
        let (mem, name) = setup_memory("notfound");

        let result = Event::open(&mem, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::NotFoundError(_) => {}
            other => panic!("expected NotFoundError, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_event_duplicate_name() {
        let (mem, name) = setup_memory("duplicate");

        Event::new(&mem, "event", EventMode::AutoReset).expect("failed to create event");
        let result = Event::new(&mem, "event", EventMode::AutoReset);
        assert!(result.is_err());
        match result.unwrap_err() {
            ZeroShmError::AlreadyExists(_) => {}
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        cleanup_memory(&mem, &name);
    }
}

