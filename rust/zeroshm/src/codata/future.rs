//! Shared memory Future for asynchronous computation results.
//!
//! Provides the `Future<T>` type representing a value that will be available
//! at some point in the future. Built on Once + Semaphore primitives.
//!
//! # Binary Layout
//! ```text
//! [once: 4 bytes][semaphore: 16 bytes][padding: 4 bytes][value: T]
//! ```
//!
//! # Usage
//! - Producer calls `set_value()` once to publish the result
//! - Consumers call `get()` to block until the result is available
//! - Multiple consumers can wait on the same future
use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use crate::sync::once::ONCE_HEADER_SIZE;
use crate::sync::semaphore::SEMAPHORE_HEADER_SIZE;
use crate::sync::{Once, Semaphore};
use std::marker::PhantomData;
use std::sync::Arc;

/// Total header size: Once (4) + Semaphore (16) + padding (4) = 24 bytes
const FUTURE_HEADER_SIZE: usize = ONCE_HEADER_SIZE + SEMAPHORE_HEADER_SIZE + 4;

/// A shared memory Future for a single result value.
///
/// Composed of:
/// - `Once`: ensures `set_value()` is executed exactly once
/// - `Semaphore`: signals consumers when the value is ready (starts at 0, released once)
/// - `value`: the result stored after setting
pub struct Future<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    once: Once,
    semaphore: Semaphore,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Future<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Future")
            .field("name", &self.name)
            .field("is_ready", &self.is_ready())
            .finish()
    }
}

// SAFETY: Future holds Arc<Memory> and Once/Semaphore which are Send + Sync.
// The value field is accessed only after synchronization via Once/Semaphore.
unsafe impl<T: Pod> Send for Future<T> {}
unsafe impl<T: Pod> Sync for Future<T> {}

impl<T: Pod> Future<T> {
    /// Create a new Future in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Base name for the Future (creates `{name}`, `{name}_once`, `{name}_sem`)
    pub fn new(memory: &Memory, name: &str) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let total_size = FUTURE_HEADER_SIZE + T::SIZE;
        let offset = memory.allocate(name, total_size)? as usize;

        // Create Once at offset (4 bytes)
        let once_name = format!("{}_once", name);
        let once = Once::new(memory, &once_name)?;

        // Create Semaphore at offset + 4 (16 bytes), initial count=0, unbounded
        let sem_name = format!("{}_sem", name);
        let semaphore = Semaphore::new(memory, &sem_name, 0, 0)?;

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            once,
            semaphore,
            _marker: PhantomData,
        })
    }

    /// Open an existing Future from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Base name of the Future
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "future '{}' not found",
                    name
                )))
            }
        };

        let once_name = format!("{}_once", name);
        let once = Once::open(memory, &once_name)?;

        let sem_name = format!("{}_sem", name);
        let semaphore = Semaphore::open(memory, &sem_name)?;

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: offset as usize,
            once,
            semaphore,
            _marker: PhantomData,
        })
    }

    /// Returns a pointer to the value field.
    fn value_ptr(&self) -> *const T {
        self.memory
            .at(self.offset + FUTURE_HEADER_SIZE)
            .cast::<T>()
    }

    /// Set the future's value (can only succeed once).
    ///
    /// Uses Once to ensure only one caller executes the set. The Once guard
    /// writes the value and releases the semaphore to wake all waiters.
    ///
    /// IMPORTANT: The value write and semaphore release happen INSIDE the Once
    /// closure so that the Once DONE state (checked by consumers) is only set
    /// after the value is safely written. This prevents a race where a consumer
    /// sees `is_called() == true` but reads uninitialized value data.
    ///
    /// Returns `true` if this call set the value, `false` if it was already set.
    pub fn set_value(&self, value: T) -> bool {
        if self.once.is_called() {
            return false;
        }

        // Use Once to ensure only one thread writes and signals
        // We need a flag to know if we won the race
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);
        let semaphore = &self.semaphore;
        let value_ptr = self.value_ptr() as *mut T;

        self.once.call(move || {
            // Only the winner executes this.
            // Order matters: write value, release semaphore, THEN Once marks DONE.
            // This ensures consumers waiting on is_called() see the written value.
            unsafe {
                std::ptr::write(value_ptr, value);
            }
            // Release semaphore to wake all waiters
            let _ = semaphore.release();
            flag_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        flag.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the future's value, blocking until it is set.
    ///
    /// If already set, returns immediately. Otherwise spin-waits until the
    /// producer calls `set_value()`.
    pub fn get(&self) -> Result<T> {
        // Spin-wait until ready
        while !self.once.is_called() {
            std::thread::yield_now();
        }
        unsafe { Ok(std::ptr::read(self.value_ptr())) }
    }

    /// Try to get the value without blocking.
    ///
    /// Returns `Some(value)` if the future is ready, `None` otherwise.
    pub fn try_get(&self) -> Result<Option<T>> {
        if self.once.is_called() {
            unsafe { return Ok(Some(std::ptr::read(self.value_ptr()))); }
        }
        Ok(None)
    }

    /// Try to get the value with a timeout.
    ///
    /// Returns `Some(value)` if ready within timeout, `None` on timeout.
    pub fn get_timeout(&self, timeout: std::time::Duration) -> Result<Option<T>> {
        let deadline = std::time::Instant::now() + timeout;
        // Spin-wait until ready or timeout
        while !self.once.is_called() {
            if std::time::Instant::now() > deadline {
                return Ok(None);
            }
            std::thread::yield_now();
        }
        unsafe { Ok(Some(std::ptr::read(self.value_ptr()))) }
    }

    /// Check if the future has been set (is ready).
    pub fn is_ready(&self) -> bool {
        self.once.is_called()
    }

    /// Returns the Future name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset in the memory segment.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_future_set_and_get() {
        let name = "/test_future_basic";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let future = Future::<i32>::new(&memory, "future_test").unwrap();

        assert!(!future.is_ready());
        assert_eq!(future.try_get().unwrap(), None);

        assert!(future.set_value(42));
        assert!(future.is_ready());
        assert_eq!(future.get().unwrap(), 42);

        // Setting again should fail
        assert!(!future.set_value(100));

        // Get still returns the original value
        assert_eq!(future.get().unwrap(), 42);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_future_timeout() {
        let name = "/test_future_timeout";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let future = Future::<i32>::new(&memory, "future_timeout").unwrap();

        // Should timeout
        assert_eq!(
            future
                .get_timeout(Duration::from_millis(50))
                .unwrap(),
            None
        );

        // Now set and get
        assert!(future.set_value(99));
        assert_eq!(future.get_timeout(Duration::from_millis(100)).unwrap(), Some(99));

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_future_producer_consumer_threads() {
        let name = "/test_future_threads";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let future = Arc::new(Future::<i32>::new(&memory, "future_threads").unwrap());

        // Producer thread
        let producer_future = Arc::clone(&future);
        let producer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            producer_future.set_value(123);
        });

        // Consumer thread (should block until producer sets value)
        let consumer_future = Arc::clone(&future);
        let consumer = thread::spawn(move || {
            consumer_future.get().unwrap()
        });

        producer.join().unwrap();
        let result = consumer.join().unwrap();
        assert_eq!(result, 123);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_future_multiple_consumers() {
        let name = "/test_future_multi";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let future = Arc::new(Future::<i32>::new(&memory, "future_multi").unwrap());

        // Spawn 4 consumer threads
        let mut handles = Vec::new();
        for _ in 0..4 {
            let f = Arc::clone(&future);
            let handle = thread::spawn(move || f.get_timeout(Duration::from_secs(2)));
            handles.push(handle);
        }

        // Set value after consumers start waiting
        thread::sleep(Duration::from_millis(10));
        future.set_value(555);

        for handle in handles {
            let result = handle.join().unwrap().unwrap();
            assert_eq!(result, Some(555));
        }

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_future_open_persistence() {
        let name = "/test_future_persist";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let future = Future::<i32>::new(&memory, "future_persist").unwrap();
            assert!(future.set_value(777));
        }

        // Reopen and read
        let memory2 = Memory::open(name, 64).unwrap();
        let future = Future::<i32>::open(&memory2, "future_persist").unwrap();
        assert!(future.is_ready());
        assert_eq!(future.get().unwrap(), 777);

        memory2.unlink().unwrap();
        drop(memory2);
        drop(memory);
    }

    #[test]
    fn test_future_not_found() {
        let name = "/test_future_notfound";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Future::<i32>::open(&memory, "nonexistent_future");
        assert!(result.is_err());

        memory.unlink().unwrap();
        drop(memory);
    }
}
