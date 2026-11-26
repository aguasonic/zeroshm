//! Lazy evaluation container for deferred computations in shared memory.
//!
//! Provides the `Lazy<T>` type representing a value that is computed on first
//! access and cached for subsequent reads. Built on the Once primitive.
//!
//! # Binary Layout
//! ```text
//! [once: 4 bytes][padding: 4 bytes][value: T]
//! ```
//!
//! # Usage
//! - `get_or_init(f)`: computes the value on first call using `f`, caches it
//! - `get()`: blocks until the value is computed, then returns it
//! - Multiple threads/processes can call `get_or_init` safely; `f` executes exactly once
use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use crate::sync::once::ONCE_HEADER_SIZE;
use crate::sync::Once;
use std::marker::PhantomData;
use std::sync::Arc;

/// Total header size: Once (4) + padding (4) = 8 bytes
const LAZY_HEADER_SIZE: usize = ONCE_HEADER_SIZE + 4;

/// A lazy computation container stored in shared memory.
///
/// The value is computed at most once across all processes. Subsequent reads
/// return the cached value without recomputation.
pub struct Lazy<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    once: Once,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Lazy<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lazy")
            .field("name", &self.name)
            .field("is_computed", &self.is_computed())
            .finish()
    }
}

// SAFETY: Lazy holds Arc<Memory> and Once which are Send + Sync.
// The value field is accessed only after synchronization via Once.
unsafe impl<T: Pod> Send for Lazy<T> {}
unsafe impl<T: Pod> Sync for Lazy<T> {}

impl<T: Pod> Lazy<T> {
    /// Create a new Lazy container in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Base name for the Lazy (creates `{name}`, `{name}_once`)
    pub fn new(memory: &Memory, name: &str) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let total_size = LAZY_HEADER_SIZE + T::SIZE;
        let offset = memory.allocate(name, total_size)? as usize;

        // Create Once
        let once_name = format!("{}_once", name);
        let once = Once::new(memory, &once_name)?;

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            once,
            _marker: PhantomData,
        })
    }

    /// Open an existing Lazy container from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Base name of the Lazy
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "lazy '{}' not found",
                    name
                )))
            }
        };

        let once_name = format!("{}_once", name);
        let once = Once::open(memory, &once_name)?;

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset: offset as usize,
            once,
            _marker: PhantomData,
        })
    }

    /// Returns a pointer to the value field.
    fn value_ptr(&self) -> *const T {
        self.memory.at(self.offset + LAZY_HEADER_SIZE).cast::<T>()
    }

    /// Get or initialize the value, computing it if not yet done.
    ///
    /// The closure `f` is executed at most once across all processes.
    /// All other callers block until the computation completes.
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> T {
        // Use Once to ensure f executes exactly once
        // We need to capture the result for other threads to read
        let once_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let once_called_clone = Arc::clone(&once_called);

        if !self.once.is_called() {
            self.once.call(move || {
                once_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        }

        if once_called.load(std::sync::atomic::Ordering::SeqCst) {
            // We won the race -- compute and store the value
            let value = f();
            unsafe {
                std::ptr::write(self.value_ptr() as *mut T, value);
            }
            value
        } else {
            // We lost the race -- wait for the winner to finish, then read
            loop {
                if self.once.is_called() {
                    break;
                }
                std::thread::yield_now();
            }
            unsafe { std::ptr::read(self.value_ptr()) }
        }
    }

    /// Get the value, blocking until it is computed.
    ///
    /// Returns an error if the value has not been computed and never will be
    /// (i.e., no one has called `get_or_init`). In practice, this blocks
    /// until `get_or_init` is called by some process.
    pub fn get(&self) -> Result<T> {
        // Wait for Once to be called
        loop {
            if self.once.is_called() {
                break;
            }
            std::thread::yield_now();
        }
        unsafe { Ok(std::ptr::read(self.value_ptr())) }
    }

    /// Check if the value has been computed.
    pub fn is_computed(&self) -> bool {
        self.once.is_called()
    }

    /// Returns the Lazy name.
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
    use std::sync::atomic::{AtomicUsize, Ordering as SyncOrdering};
    use std::sync::Arc as StdArc;
    use std::thread;

    #[test]
    fn test_lazy_get_or_init() {
        let name = "/test_lazy_basic";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let lazy = Lazy::<i32>::new(&memory, "lazy_test").unwrap();

        assert!(!lazy.is_computed());

        let result = lazy.get_or_init(|| 42);
        assert_eq!(result, 42);
        assert!(lazy.is_computed());

        // Second call should return cached value, not recompute
        let result2 = lazy.get_or_init(|| 100);
        assert_eq!(result2, 42); // still 42, not 100

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_lazy_multithreaded() {
        let name = "/test_lazy_mt";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let lazy = StdArc::new(Lazy::<i32>::new(&memory, "lazy_mt").unwrap());

        let compute_count = StdArc::new(AtomicUsize::new(0));

        let num_threads = 4;
        let mut handles = Vec::new();

        for _ in 0..num_threads {
            let lazy = StdArc::clone(&lazy);
            let count = StdArc::clone(&compute_count);
            let handle = thread::spawn(move || {
                let result = lazy.get_or_init(|| {
                    count.fetch_add(1, SyncOrdering::SeqCst);
                    777
                });
                result
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.join().unwrap();
            assert_eq!(result, 777);
        }

        // Function should have been executed exactly once
        assert_eq!(compute_count.load(SyncOrdering::SeqCst), 1);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_lazy_get_blocks() {
        let name = "/test_lazy_get";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let lazy = StdArc::new(Lazy::<i32>::new(&memory, "lazy_get").unwrap());

        // Consumer thread waits on get()
        let consumer_lazy = StdArc::clone(&lazy);
        let consumer = thread::spawn(move || consumer_lazy.get().unwrap());

        // Give consumer time to start waiting
        thread::sleep(std::time::Duration::from_millis(10));

        // Producer initializes
        lazy.get_or_init(|| 321);

        let result = consumer.join().unwrap();
        assert_eq!(result, 321);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_lazy_open_persistence() {
        let name = "/test_lazy_persist";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let lazy = Lazy::<i32>::new(&memory, "lazy_persist").unwrap();
            let result = lazy.get_or_init(|| 888);
            assert_eq!(result, 888);
        }

        // Reopen and read
        let memory2 = Memory::open(name, 64).unwrap();
        let lazy = Lazy::<i32>::open(&memory2, "lazy_persist").unwrap();
        assert!(lazy.is_computed());
        assert_eq!(lazy.get().unwrap(), 888);

        memory2.unlink().unwrap();
        drop(memory2);
        drop(memory);
    }

    #[test]
    fn test_lazy_not_found() {
        let name = "/test_lazy_notfound";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        let result = Lazy::<i32>::open(&memory, "nonexistent_lazy");
        assert!(result.is_err());

        memory.unlink().unwrap();
        drop(memory);
    }
}

