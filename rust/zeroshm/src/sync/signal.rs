//! Reactive signal for fine-grained reactivity across processes.
//!
//! Provides the `Signal<T>` type that stores a value which can be observed
//! for changes. When the value changes, the version number increments,
//! allowing other processes to detect and react to changes.
//!
//! # Binary Layout
//! ```text
//! [version: u64 (8 bytes)][value: T (sizeof(T) bytes)]
//! ```
//! Plus additional table entries:
//! - `name_state`: Signal state (8 + sizeof(T) bytes)
//! - `name_mtx`: Mutex for thread-safe updates
//! - `name`: 1-byte marker
//!
//! - version: atomic u64 at offset 0 -- increments on each change
//! - value: T at offset 8 -- the current signal value
//!
//! # Features
//! - Fine-grained reactivity (SolidJS/Preact style)
//! - Version tracking for efficient polling
//! - Cross-process change detection
//! - Atomic updates via update()
//!
//! # Example
//! ```ignore
//! use zeroshm::{Memory, Signal};
//!
//! let mem = Memory::create("/data", 1024 * 1024, 64)?;
//! let counter = Signal::<i32>::new(&mem, "counter", 0)?;
//!
//! // Process A - producer
//! counter.set(counter.get()? + 1);
//!
//! // Process B - reactive consumer
//! let last_version = counter.version();
//! while counter.wait_for_change(last_version, Duration::from_secs(1))? {
//!     println!("Counter changed to: {}", counter.get()?);
//!     last_version = counter.version();
//! }
//! ```

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use crate::sync::mutex::Mutex;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Size of the version field in bytes.
const VERSION_SIZE: usize = 8;

/// A reactive signal stored in shared memory.
///
/// Stores a value of type `T` (must be `Pod`) with an atomic version counter.
/// The version increments on each `set()` or `update()`, enabling other
/// processes to detect changes efficiently.
pub struct Signal<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    mutex: Arc<Mutex>,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signal")
            .field("name", &self.name)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Signal holds Arc<Memory> and Arc<Mutex> which are Send + Sync.
// The version field is atomic, and the value field is protected by the mutex.
// mmap'd memory with MAP_SHARED is process-shared.
unsafe impl<T: Pod> Send for Signal<T> {}
unsafe impl<T: Pod> Sync for Signal<T> {}

impl<T: Pod> Signal<T> {
    /// Size of the signal state in bytes (version + value).
    pub fn state_size() -> usize {
        VERSION_SIZE + T::SIZE
    }

    /// Create a new signal in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the signal (stored in the table)
    /// * `initial_value` -- Initial value of the signal
    pub fn new(memory: &Memory, name: &str, initial_value: T) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let state_name = format!("{}_state", name);
        let mutex_name = format!("{}_mtx", name);

        // Allocate space for state: [version: u64][value: T]
        let state_size = Self::state_size();
        let state_offset = memory.allocate(&state_name, state_size)? as usize;

        // Initialize state: version=0, value=initial_value
        unsafe {
            let base = memory.at_mut(state_offset);
            (*base.cast::<AtomicU64>()).store(0, Ordering::Relaxed);
            std::ptr::write(base.add(VERSION_SIZE).cast::<T>(), initial_value);
        }

        // Create mutex for thread-safe updates
        let mutex = Arc::new(Mutex::new(memory, &mutex_name)?);

        // Add marker entry
        let marker_offset = memory.allocate(name, 1)? as usize;
        let _ = marker_offset;

        Ok(Signal {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: state_offset,
            mutex,
            _marker: PhantomData,
        })
    }

    /// Open an existing signal in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the signal to open
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        // Check if exists
        if memory.find(name)?.is_none() {
            return Err(ZeroShmError::NotFoundError(name.to_string()));
        }

        let state_name = format!("{}_state", name);
        let mutex_name = format!("{}_mtx", name);

        // Find state entry
        let state_entry = memory.find(&state_name)?;
        let (state_offset, state_size) = state_entry.ok_or_else(|| {
            ZeroShmError::NotFoundError(format!("{}_state not found", name))
        })?;

        // Validate state size
        let expected_size = Self::state_size();
        if state_size as usize != expected_size {
            return Err(ZeroShmError::SizeError(format!(
                "signal state size mismatch: got {}, expected {}",
                state_size, expected_size
            )));
        }

        // Open mutex
        let mutex = Arc::new(Mutex::open(memory, &mutex_name)?);

        Ok(Signal {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: state_offset as usize,
            mutex,
            _marker: PhantomData,
        })
    }

    /// Get the current value.
    ///
    /// Acquires the mutex to ensure a consistent read.
    pub fn get(&self) -> Result<T> {
        let _guard = self.mutex.lock()?;
        let value = unsafe {
            let base = self.memory.at_mut(self.offset);
            *base.add(VERSION_SIZE).cast::<T>()
        };
        Ok(value)
    }

    /// Set a new value.
    ///
    /// Atomically sets the value and increments the version under the mutex.
    pub fn set(&self, new_value: T) -> Result<()> {
        let _guard = self.mutex.lock()?;
        unsafe {
            let base = self.memory.at_mut(self.offset);
            std::ptr::write(base.add(VERSION_SIZE).cast::<T>(), new_value);
            (*base.cast::<AtomicU64>()).fetch_add(1, Ordering::Release);
        }
        Ok(())
    }

    /// Atomically update the value using a function.
    ///
    /// Applies the function to the current value under the mutex,
    /// ensuring the read-modify-write is atomic.
    pub fn update<F>(&self, func: F) -> Result<T>
    where
        F: FnOnce(T) -> T,
    {
        let _guard = self.mutex.lock()?;
        unsafe {
            let base = self.memory.at_mut(self.offset);
            let value_ptr = base.add(VERSION_SIZE).cast::<T>();
            let current = *value_ptr;
            let new_value = func(current);
            std::ptr::write(value_ptr, new_value);
            (*base.cast::<AtomicU64>()).fetch_add(1, Ordering::Release);
            Ok(new_value)
        }
    }

    /// Get the current version number.
    ///
    /// Version increments on each set/update. Use for change detection.
    pub fn version(&self) -> u64 {
        unsafe {
            let base = self.memory.at(self.offset);
            (*base.cast::<AtomicU64>()).load(Ordering::Acquire)
        }
    }

    /// Check if the signal has changed since a given version.
    ///
    /// # Arguments
    /// * `old_version` -- Previous version to compare against
    pub fn has_changed(&self, old_version: u64) -> bool {
        self.version() != old_version
    }

    /// Wait for the signal to change from a given version.
    ///
    /// Spin-waits with exponential backoff until the version changes or
    /// the timeout expires.
    ///
    /// # Arguments
    /// * `old_version` -- Version to wait for change from
    /// * `timeout` -- Maximum time to wait
    ///
    /// # Returns
    /// `Ok(true)` if the signal changed, `Ok(false)` if timeout expired.
    pub fn wait_for_change(&self, old_version: u64, timeout: Duration) -> Result<bool> {
        let deadline = std::time::Instant::now() + timeout;
        let mut backoff = Duration::from_micros(100);

        while std::time::Instant::now() < deadline {
            if self.has_changed(old_version) {
                return Ok(true);
            }
            thread::sleep(backoff);
            // Exponential backoff up to 1ms
            backoff = std::cmp::min(backoff * 2, Duration::from_millis(1));
        }

        Ok(false)
    }

    /// Returns the name of the signal.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the shared memory.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the offset of the signal state in shared memory.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;

    fn setup_memory(suffix: &str) -> (Memory, String) {
        let name = format!("/test_signal_{}_{}", suffix, std::process::id());
        let size = 1024 * 1024;
        let _ = platform::unlink_shm(&name);
        let mem = Memory::create(&name, size, 64).expect("create should succeed");
        (mem, name)
    }

    fn cleanup_memory(mem: &Memory, name: &str) {
        let _ = mem.unlink();
        let _ = Memory::open(name, 64).map(|m| m.unlink());
    }

    #[test]
    fn test_signal_basic() {
        let (mem, name) = setup_memory("basic");

        // Create signal with initial value
        let sig = Signal::<i32>::new(&mem, "counter", 42).expect("create should succeed");
        assert_eq!(sig.get().expect("get"), 42);
        assert_eq!(sig.version(), 0);

        // Set new value
        sig.set(100).expect("set should succeed");
        assert_eq!(sig.get().expect("get"), 100);
        assert_eq!(sig.version(), 1);

        // Set again
        sig.set(200).expect("set should succeed");
        assert_eq!(sig.get().expect("get"), 200);
        assert_eq!(sig.version(), 2);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_update() {
        let (mem, name) = setup_memory("update");

        let sig = Signal::<i32>::new(&mem, "counter", 0).expect("create should succeed");

        // Use update to increment
        let val = sig
            .update(|v| v + 1)
            .expect("update should succeed");
        assert_eq!(val, 1);
        assert_eq!(sig.get().expect("get"), 1);
        assert_eq!(sig.version(), 1);

        let val = sig
            .update(|v| v * 2)
            .expect("update should succeed");
        assert_eq!(val, 2);
        assert_eq!(sig.get().expect("get"), 2);
        assert_eq!(sig.version(), 2);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_version_tracking() {
        let (mem, name) = setup_memory("version");

        let sig = Signal::<i32>::new(&mem, "counter", 0).expect("create should succeed");
        assert_eq!(sig.version(), 0);

        let v0 = sig.version();
        assert!(!sig.has_changed(v0), "should not have changed");

        sig.set(1).expect("set");
        assert!(sig.has_changed(v0), "should have changed");
        assert_eq!(sig.version(), 1);

        let v1 = sig.version();
        assert!(!sig.has_changed(v1), "should not have changed yet");

        sig.update(|v| v + 10).expect("update");
        assert!(sig.has_changed(v1), "should have changed");
        assert_eq!(sig.version(), 2);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_wait_for_change() {
        let (mem, name) = setup_memory("wait");

        let sig = Signal::<i32>::new(&mem, "counter", 0).expect("create should succeed");
        let last_version = sig.version();

        // Start a thread that will change the signal after a delay
        let sig_clone = Signal::<i32>::open(&mem, "counter").expect("open should succeed");
        let handle = std::thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            sig_clone.set(42).expect("set in thread");
        });

        // Wait for change with 2 second timeout
        let changed = sig
            .wait_for_change(last_version, Duration::from_secs(2))
            .expect("wait should succeed");
        assert!(changed, "should detect change");
        assert_eq!(sig.get().expect("get"), 42);

        handle.join().unwrap();

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_wait_timeout() {
        let (mem, name) = setup_memory("timeout");

        let sig = Signal::<i32>::new(&mem, "counter", 0).expect("create should succeed");
        let last_version = sig.version();

        // Wait for change with very short timeout -- should timeout
        let start = std::time::Instant::now();
        let changed = sig
            .wait_for_change(last_version, Duration::from_millis(50))
            .expect("wait should succeed");
        let elapsed = start.elapsed();

        assert!(!changed, "should timeout without change");
        assert!(elapsed >= Duration::from_millis(40), "should wait at least 40ms");

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_persistence() {
        let name = format!("/test_signal_persist_{}", std::process::id());
        let size = 1024 * 1024;
        let _ = platform::unlink_shm(&name);

        // Create and set value (don't unlink -- segment persists for reopen)
        {
            let mem = Memory::create(&name, size, 64).expect("create");
            let sig = Signal::<i32>::new(&mem, "counter", 99).expect("create signal");
            sig.set(123).expect("set");
            assert_eq!(sig.version(), 1);
            drop(sig);
            drop(mem);
        }

        // Reopen and verify
        {
            let mem = Memory::open(&name, 64).expect("open");
            let sig = Signal::<i32>::open(&mem, "counter").expect("open signal");
            assert_eq!(sig.get().expect("get"), 123);
            assert_eq!(sig.version(), 1);
            drop(sig);
            drop(mem);
        }

        let _ = platform::unlink_shm(&name);
    }

    #[test]
    fn test_signal_not_found() {
        let (mem, name) = setup_memory("notfound");

        let result = Signal::<i32>::open(&mem, "nonexistent");
        assert!(
            matches!(result, Err(ZeroShmError::NotFoundError(_))),
            "opening nonexistent signal should return NotFoundError"
        );

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_duplicate_name() {
        let (mem, name) = setup_memory("duplicate");

        Signal::<i32>::new(&mem, "counter", 0).expect("first create");

        let result = Signal::<i32>::new(&mem, "counter", 42);
        assert!(
            matches!(result, Err(ZeroShmError::AlreadyExists(_))),
            "duplicate create should return AlreadyExists"
        );

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_f64() {
        let (mem, name) = setup_memory("f64");

        let sig = Signal::<f64>::new(&mem, "value", 3.14).expect("create");
        assert!((sig.get().expect("get") - 3.14).abs() < f64::EPSILON);

        sig.set(2.71).expect("set");
        assert!((sig.get().expect("get") - 2.71).abs() < f64::EPSILON);

        cleanup_memory(&mem, &name);
    }

    #[test]
    fn test_signal_u64() {
        let (mem, name) = setup_memory("u64");

        let sig = Signal::<u64>::new(&mem, "counter", 0).expect("create");
        sig.set(123456789).expect("set");
        assert_eq!(sig.get().expect("get"), 123456789);

        cleanup_memory(&mem, &name);
    }
}

