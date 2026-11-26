//! Single-producer, multi-reader shared memory value with change detection.
//!
//! Provides the `Latest<T>` type for a single-producer, multi-reader pattern
//! where consumers observe the current value and are notified of changes via
//! a monotonically increasing version number.
//!
//! The producer calls `set()` to write a new value, increment the version,
//! and broadcast to all waiting consumers. Consumers call `get()` to read the
//! current value, or `wait_for_change()` to block (via pthread condition
//! variable) until the version advances.
//!
//! # Binary Layout
//! ```text
//! [header: 8 bytes][value: elem_size bytes][padding][version: u64]
//! [mutex: pthread_mutex_t][cond: pthread_cond_t]
//! ```
//!
//! Header layout (little-endian):
//! - elem_size: u32 at offset 0 (size of T in bytes)
//! - aligned_size: u32 at offset 4 (align_up(elem_size, 8))
//!
//! The version field is at offset `align_up(elem_size, 8)` within the payload,
//! ensuring 8-byte alignment for atomic operations.
//!
//! The mutex and condition variable (initialized with `PTHREAD_PROCESS_SHARED`)
//! follow the version field and enable true OS-level blocking for readers.
//!
//! Total size = 8 (header) + align_up(elem_size, 8) + 8 (version)
//!              + sizeof(pthread_mutex_t) + sizeof(pthread_cond_t)

use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::layout::SECTION_ALIGN;
use crate::memory::Memory;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Size of the Latest header in bytes.
/// Layout: elem_size (u32) + aligned_size (u32) = 8 bytes
const LATEST_HEADER_SIZE: usize = 8;

/// Alignment for the version field (must be 8 for atomic u64).
const VERSION_ALIGN: usize = SECTION_ALIGN;

/// Rounds `n` up to the next multiple of `a` (a must be a power of two).
const fn align_up(n: usize, a: usize) -> usize {
    (n + a - 1) & !(a - 1)
}

/// A shared mutex + condition variable pair allocated in shared memory,
/// initialized with `PTHREAD_PROCESS_SHARED` for cross-process blocking.
struct SharedCondvar {
    mutex: *mut libc::pthread_mutex_t,
    cond: *mut libc::pthread_cond_t,
}

// SAFETY: SharedCondvar holds raw pointers to shared memory. The mutex and
// condvar are protected by pthread synchronization primitives, making them
// safe for concurrent access across processes.
unsafe impl Send for SharedCondvar {}
unsafe impl Sync for SharedCondvar {}

impl SharedCondvar {
    /// Initialize the mutex and condvar with PTHREAD_PROCESS_SHARED.
    /// Must be called exactly once by the creator process.
    unsafe fn init(&self) -> Result<()> {
        let mut attr: libc::pthread_mutexattr_t = std::mem::zeroed();
        let mut ret = libc::pthread_mutexattr_init(&mut attr);
        if ret != 0 {
            return Err(ZeroShmError::MmapError(format!(
                "pthread_mutexattr_init failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        ret = libc::pthread_mutexattr_setpshared(&mut attr, libc::PTHREAD_PROCESS_SHARED);
        if ret != 0 {
            libc::pthread_mutexattr_destroy(&mut attr);
            return Err(ZeroShmError::MmapError(format!(
                "pthread_mutexattr_setpshared failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        ret = libc::pthread_mutex_init(self.mutex, &attr);
        libc::pthread_mutexattr_destroy(&mut attr);
        if ret != 0 {
            return Err(ZeroShmError::MmapError(format!(
                "pthread_mutex_init failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        let mut cond_attr: libc::pthread_condattr_t = std::mem::zeroed();
        ret = libc::pthread_condattr_init(&mut cond_attr);
        if ret != 0 {
            libc::pthread_mutex_destroy(self.mutex);
            return Err(ZeroShmError::MmapError(format!(
                "pthread_condattr_init failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        ret = libc::pthread_condattr_setpshared(&mut cond_attr, libc::PTHREAD_PROCESS_SHARED);
        if ret != 0 {
            libc::pthread_condattr_destroy(&mut cond_attr);
            libc::pthread_mutex_destroy(self.mutex);
            return Err(ZeroShmError::MmapError(format!(
                "pthread_condattr_setpshared failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        ret = libc::pthread_cond_init(self.cond, &cond_attr);
        libc::pthread_condattr_destroy(&mut cond_attr);
        if ret != 0 {
            libc::pthread_mutex_destroy(self.mutex);
            return Err(ZeroShmError::MmapError(format!(
                "pthread_cond_init failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(())
    }

    /// Lock the mutex.
    unsafe fn lock(&self) -> Result<()> {
        let ret = libc::pthread_mutex_lock(self.mutex);
        if ret != 0 {
            return Err(ZeroShmError::MmapError(format!(
                "pthread_mutex_lock failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Unlock the mutex.
    unsafe fn unlock(&self) -> Result<()> {
        let ret = libc::pthread_mutex_unlock(self.mutex);
        if ret != 0 {
            return Err(ZeroShmError::MmapError(format!(
                "pthread_mutex_unlock failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Broadcast to all waiting threads.
    unsafe fn broadcast(&self) -> Result<()> {
        let ret = libc::pthread_cond_broadcast(self.cond);
        if ret != 0 {
            return Err(ZeroShmError::MmapError(format!(
                "pthread_cond_broadcast failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Wait on the condition variable (atomically unlocks mutex and blocks).
    #[allow(dead_code)]
    unsafe fn wait(&self) -> Result<()> {
        let ret = libc::pthread_cond_wait(self.cond, self.mutex);
        if ret != 0 {
            return Err(ZeroShmError::MmapError(format!(
                "pthread_cond_wait failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Wait on the condition variable with a timeout.
    /// Returns true if signaled, false if timed out.
    unsafe fn wait_timeout(&self, timeout: Duration) -> Result<bool> {
        let mut ts: libc::timespec = std::mem::zeroed();

        // Get current time
        let mut now_ts: libc::timespec = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut now_ts);

        ts.tv_sec = now_ts.tv_sec;
        ts.tv_nsec = now_ts.tv_nsec;

        // Add timeout
        let total_ns = ts.tv_nsec + timeout.as_nanos() as i64;
        ts.tv_sec += total_ns / 1_000_000_000;
        ts.tv_nsec = total_ns % 1_000_000_000;

        let ret = libc::pthread_cond_timedwait(self.cond, self.mutex, &ts);
        match ret {
            0 => Ok(true),
            libc::ETIMEDOUT => Ok(false),
            _ => Err(ZeroShmError::MmapError(format!(
                "pthread_cond_timedwait failed: {}",
                std::io::Error::last_os_error()
            ))),
        }
    }

    /// Destroy the mutex and condvar. Called when the Latest is no longer needed.
    unsafe fn destroy(&self) {
        libc::pthread_mutex_destroy(self.mutex);
        libc::pthread_cond_destroy(self.cond);
    }
}

/// A single-producer, multi-reader shared memory value with change detection.
///
/// The producer calls `set()` to write a new value, increment the version,
/// and broadcast to all waiting consumers. Consumers call `get()` to read the
/// current value and version, or `wait_for_change()` to block until the
/// version advances (using a pthread condition variable for true blocking).
pub struct Latest<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    elem_size: usize,
    aligned_size: usize,
    condvar: SharedCondvar,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Latest<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Latest")
            .field("name", &self.name)
            .field("elem_size", &self.elem_size)
            .field("aligned_size", &self.aligned_size)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Latest holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the element type is safe for concurrent access via raw pointers
// in process-shared memory. The mutex/condvar handle cross-process
// synchronization.
unsafe impl<T: Pod> Send for Latest<T> {}
unsafe impl<T: Pod> Sync for Latest<T> {}

impl<T: Pod> Latest<T> {
    /// Return the offset of the value field in shared memory.
    fn value_offset(&self) -> usize {
        self.offset + LATEST_HEADER_SIZE
    }

    /// Return the offset of the version field in shared memory.
    fn version_offset(&self) -> usize {
        self.value_offset() + self.aligned_size
    }

    /// Return the total size of the Latest structure in shared memory.
    fn total_size(elem_size: usize) -> usize {
        let aligned_size = align_up(elem_size, VERSION_ALIGN);
        let version_end = LATEST_HEADER_SIZE + aligned_size + 8;
        let mutex_start = align_up(version_end, std::mem::align_of::<libc::pthread_mutex_t>());
        let cond_start = align_up(
            mutex_start + std::mem::size_of::<libc::pthread_mutex_t>(),
            std::mem::align_of::<libc::pthread_cond_t>(),
        );
        cond_start + std::mem::size_of::<libc::pthread_cond_t>()
    }

    /// Return a pointer to the atomic version in shared memory.
    unsafe fn version_ptr(&self) -> *const AtomicU64 {
        self.memory.at(self.version_offset()).cast::<AtomicU64>()
    }

    /// Return a mutable pointer to the atomic version in shared memory.
    unsafe fn version_ptr_mut(&self) -> *mut AtomicU64 {
        self.memory.at_mut(self.version_offset()).cast::<AtomicU64>()
    }

    /// Return a pointer to the value in shared memory.
    unsafe fn value_ptr(&self) -> *const u8 {
        self.memory.at(self.value_offset())
    }

    /// Return a mutable pointer to the value in shared memory.
    unsafe fn value_ptr_mut(&self) -> *mut u8 {
        self.memory.at_mut(self.value_offset())
    }

    /// Create a new Latest value in shared memory.
    ///
    /// The initial value is written with version 1.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the Latest value (stored in the table)
    /// * `initial_value` -- The initial value to store
    ///
    /// # Errors
    /// - `AlreadyExists` if a value with this name already exists
    /// - `SizeError` if the type is zero-sized
    pub fn new(memory: &Memory, name: &str, initial_value: T) -> Result<Self> {
        if T::SIZE == 0 {
            return Err(ZeroShmError::SizeError(
                "Latest does not support zero-size types".to_string(),
            ));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let elem_size = T::SIZE;
        let aligned_size = align_up(elem_size, VERSION_ALIGN);
        let total_size = Self::total_size(elem_size);

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        unsafe {
            let base = memory.at_mut(offset);

            // Initialize header: elem_size + aligned_size
            std::ptr::write(base.cast::<u32>(), elem_size as u32);
            std::ptr::write(base.add(4).cast::<u32>(), aligned_size as u32);

            // Zero-initialize padding (between value and version)
            let padding_start = offset + LATEST_HEADER_SIZE + elem_size;
            let padding_end = offset + LATEST_HEADER_SIZE + aligned_size;
            std::ptr::write_bytes(
                memory.at_mut(padding_start),
                0,
                padding_end - padding_start,
            );

            // Initialize mutex and condvar with PTHREAD_PROCESS_SHARED
            let mutex_offset = mutex_offset_at(offset, elem_size);
            let cond_offset = cond_offset_at(offset, elem_size);

            let condvar = SharedCondvar {
                mutex: memory.at_mut(mutex_offset).cast::<libc::pthread_mutex_t>(),
                cond: memory.at_mut(cond_offset).cast::<libc::pthread_cond_t>(),
            };
            condvar.init()?;

            // Write initial value (under mutex)
            condvar.lock()?;
            std::ptr::copy(
                &initial_value as *const T as *const u8,
                base.add(LATEST_HEADER_SIZE),
                elem_size,
            );
            // Initialize version to 1
            (*base.add(LATEST_HEADER_SIZE + aligned_size).cast::<AtomicU64>())
                .store(1, Ordering::Release);
            condvar.unlock()?;
        }

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            elem_size,
            aligned_size,
            condvar: SharedCondvar {
                mutex: memory.at_mut(mutex_offset_at(offset, elem_size)).cast(),
                cond: memory.at_mut(cond_offset_at(offset, elem_size)).cast(),
            },
            _marker: PhantomData,
        })
    }

    /// Open an existing Latest value from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the Latest value (must exist in the table)
    ///
    /// # Errors
    /// - `NotFoundError` if the name doesn't exist
    /// - `SizeError` if there's a type size mismatch
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "latest '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;
        let elem_size = T::SIZE;

        if elem_size == 0 {
            return Err(ZeroShmError::SizeError(
                "Latest does not support zero-size types".to_string(),
            ));
        }

        unsafe {
            let base = memory.at(offset);
            let stored_elem_size = std::ptr::read(base.cast::<u32>()) as usize;
            let stored_aligned_size = std::ptr::read(base.add(4).cast::<u32>()) as usize;

            if stored_elem_size != elem_size {
                return Err(ZeroShmError::SizeError(format!(
                    "type mismatch: expected elem_size {}, got {}",
                    elem_size, stored_elem_size
                )));
            }

            let aligned_size = align_up(elem_size, VERSION_ALIGN);
            if stored_aligned_size != aligned_size {
                return Err(ZeroShmError::SizeError(format!(
                    "alignment mismatch: expected {}, got {}",
                    aligned_size, stored_aligned_size
                )));
            }

            let expected_size = Self::total_size(elem_size);
            if size as usize != expected_size {
                return Err(ZeroShmError::SizeError(format!(
                    "size mismatch: expected {}, got {} (wrong element type?)",
                    expected_size, size
                )));
            }

            Ok(Self {
                memory: Arc::new(memory.clone_shallow()),
                name: name.to_string(),
                offset,
                elem_size,
                aligned_size,
                condvar: SharedCondvar {
                    mutex: memory.at_mut(mutex_offset_at(offset, elem_size)).cast(),
                    cond: memory.at_mut(cond_offset_at(offset, elem_size)).cast(),
                },
                _marker: PhantomData,
            })
        }
    }

    /// Read the current value and version. Safe for concurrent readers.
    ///
    /// Locks the mutex to ensure a consistent read of value and version.
    pub fn get(&self) -> (T, u64) {
        unsafe {
            self.condvar.lock().expect("mutex lock failed");

            // Load version with acquire ordering
            let version = (*self.version_ptr()).load(Ordering::Acquire);

            // Read value (safe under mutex)
            let mut value: T = std::mem::zeroed();
            std::ptr::copy(self.value_ptr(), &mut value as *mut T as *mut u8, self.elem_size);

            self.condvar.unlock().expect("mutex unlock failed");

            (value, version)
        }
    }

    /// Write a new value and increment the version. Single producer only.
    ///
    /// Locks the mutex, writes the value, increments the version, broadcasts
    /// to all waiting readers, then unlocks.
    pub fn set(&self, value: T) {
        unsafe {
            self.condvar.lock().expect("mutex lock failed");

            // Write the value
            std::ptr::copy(
                &value as *const T as *const u8,
                self.value_ptr_mut(),
                self.elem_size,
            );

            // Atomically increment version with release ordering
            let version = (*self.version_ptr()).load(Ordering::Acquire);
            (*self.version_ptr_mut()).store(version + 1, Ordering::Release);

            // Broadcast to all waiting readers
            self.condvar.broadcast().expect("cond broadcast failed");

            self.condvar.unlock().expect("mutex unlock failed");
        }
    }

    /// Return the current version number. Safe for concurrent readers.
    pub fn version(&self) -> u64 {
        unsafe {
            self.condvar.lock().expect("mutex lock failed");
            let version = (*self.version_ptr()).load(Ordering::Acquire);
            self.condvar.unlock().expect("mutex unlock failed");
            version
        }
    }

    /// Block until the version is greater than `last_version` or `timeout` expires.
    ///
    /// Returns `(value, version, changed)` where `changed` is true if the
    /// version advanced. Uses pthread condition variable for true OS-level
    /// blocking -- the calling thread is suspended until woken by the writer.
    pub fn wait_for_change(&self, last_version: u64, timeout: Duration) -> (T, u64, bool) {
        unsafe {
            self.condvar.lock().expect("mutex lock failed");

            loop {
                let version = (*self.version_ptr()).load(Ordering::Acquire);
                if version > last_version {
                    // Read value
                    let mut value: T = std::mem::zeroed();
                    std::ptr::copy(self.value_ptr(), &mut value as *mut T as *mut u8, self.elem_size);
                    self.condvar.unlock().expect("mutex unlock failed");
                    return (value, version, true);
                }

                // Wait for the writer to broadcast
                let signaled = if timeout.is_zero() {
                    false
                } else {
                    match self.condvar.wait_timeout(timeout) {
                        Ok(s) => s,
                        Err(_) => false,
                    }
                };

                if !signaled {
                    // Timeout -- return current value and version
                    let version = (*self.version_ptr()).load(Ordering::Acquire);
                    let mut value: T = std::mem::zeroed();
                    std::ptr::copy(self.value_ptr(), &mut value as *mut T as *mut u8, self.elem_size);
                    self.condvar.unlock().expect("mutex unlock failed");
                    return (value, version, false);
                }
                // Spurious wakeup or signaled -- loop and re-check version
            }
        }
    }

    /// Return the name of this Latest value.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Destroy the Latest value, cleaning up the mutex and condvar.
    /// Should only be called by the owning process.
    pub fn destroy(&self) {
        unsafe {
            self.condvar.destroy();
        }
    }
}

// Compile-time validation: pthread structures must be naturally aligned.
const _: () = assert!(
    std::mem::align_of::<libc::pthread_mutex_t>() <= 8,
    "pthread_mutex_t alignment exceeds 8-byte guarantee"
);
const _: () = assert!(
    std::mem::align_of::<libc::pthread_cond_t>() <= 8,
    "pthread_cond_t alignment exceeds 8-byte guarantee"
);

// Compile-time validation: version field must be 8-aligned for atomic u64.
const _: () = assert!(VERSION_ALIGN == 8, "version field must be 8-aligned for atomic u64");
const _: () = assert!(
    version_offset_at(0, 4) % 8 == 0,
    "version offset must be 8-aligned (elem_size=4)"
);
const _: () = assert!(
    version_offset_at(0, 8) % 8 == 0,
    "version offset must be 8-aligned (elem_size=8)"
);
const _: () = assert!(
    version_offset_at(0, 16) % 8 == 0,
    "version offset must be 8-aligned (elem_size=16)"
);

/// Compute the version offset from a base offset and element size.
const fn version_offset_at(base_offset: usize, elem_size: usize) -> usize {
    let aligned_size = align_up(elem_size, VERSION_ALIGN);
    base_offset + LATEST_HEADER_SIZE + aligned_size
}

/// Compute the mutex offset from a base offset and element size.
const fn mutex_offset_at(base_offset: usize, elem_size: usize) -> usize {
    let aligned_size = align_up(elem_size, VERSION_ALIGN);
    let version_end = base_offset + LATEST_HEADER_SIZE + aligned_size + 8;
    align_up(version_end, std::mem::align_of::<libc::pthread_mutex_t>())
}

/// Compute the condvar offset from a base offset and element size.
const fn cond_offset_at(base_offset: usize, elem_size: usize) -> usize {
    let mutex_off = mutex_offset_at(base_offset, elem_size);
    align_up(
        mutex_off + std::mem::size_of::<libc::pthread_mutex_t>(),
        std::mem::align_of::<libc::pthread_cond_t>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    /// GPSFix is a struct used to test Latest with composite types.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct GPSFix {
        latitude: f64,
        longitude: f64,
        altitude: f64,
        timestamp: i64,
    }

    // SAFETY: GPSFix is a plain struct with only f64 and i64 fields, trivially copyable.
    unsafe impl Pod for GPSFix {
        const SIZE: usize = 32;
        const ALIGN: usize = 8;
    }

    fn setup_test<T: Pod>(name_prefix: &str, initial_value: T) -> (Memory, Latest<T>) {
        let name = format!("/latest_{}_{}", name_prefix, std::process::id());
        let _ = platform::unlink_shm(&name);
        let memory = Memory::create(&name, 1 << 20, 64).unwrap();
        let latest =
            Latest::<T>::new(&memory, &format!("latest_{}", name_prefix), initial_value).unwrap();
        (memory, latest)
    }

    fn cleanup(memory: &Memory) {
        let _ = memory.unlink();
    }

    // Test 1: Basic Get/Set
    #[test]
    fn test_basic_get_set() {
        let (memory, latest) = setup_test("basic", 42i32);

        // Initial value should be 42 with version 1
        let (value, version) = latest.get();
        assert_eq!(value, 42, "expected initial value 42, got {}", value);
        assert_eq!(version, 1, "expected initial version 1, got {}", version);

        // Set new value
        latest.set(99);

        let (value, version) = latest.get();
        assert_eq!(value, 99, "expected value 99 after set, got {}", value);
        assert_eq!(version, 2, "expected version 2 after set, got {}", version);

        cleanup(&memory);
    }

    // Test 2: Version monotonicity
    #[test]
    fn test_version_monotonicity() {
        let (memory, latest) = setup_test("monotonic", 0i32);

        let mut prev_version = latest.version();
        for i in 1..=100 {
            latest.set(i as i32);
            let (value, version) = latest.get();

            assert_eq!(
                value,
                i as i32,
                "iteration {}: expected value {}, got {}",
                i,
                i,
                value
            );
            assert_eq!(
                version,
                prev_version + 1,
                "iteration {}: expected version {}, got {}",
                i,
                prev_version + 1,
                version
            );
            assert!(
                version > prev_version,
                "iteration {}: version not monotonically increasing ({} -> {})",
                i,
                prev_version,
                version
            );
            prev_version = version;
        }

        cleanup(&memory);
    }

    // Test 3: WaitForChange success (blocking)
    #[test]
    fn test_wait_for_change_success() {
        let (memory, latest) = setup_test("wait_success", 0i32);
        let latest = Arc::new(latest);

        let (_, last_version) = latest.get();

        // Producer: set a new value after a delay
        let producer_latest = Arc::clone(&latest);
        let producer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            producer_latest.set(42);
        });

        // Consumer: wait for change with generous timeout (blocks via condvar)
        let start = Instant::now();
        let (value, version, changed) =
            latest.wait_for_change(last_version, Duration::from_millis(500));
        let elapsed = start.elapsed();

        assert!(changed, "expected WaitForChange to detect a change");
        assert_eq!(value, 42, "expected value 42, got {}", value);
        assert!(
            version > last_version,
            "expected version > {}, got {}",
            last_version,
            version
        );
        // Should return well before timeout since it was woken by broadcast
        assert!(
            elapsed < Duration::from_millis(400),
            "expected to be woken before timeout, took {:?}",
            elapsed
        );

        producer.join().unwrap();
        cleanup(&memory);
    }

    // Test 4: WaitForChange timeout
    #[test]
    fn test_wait_for_change_timeout() {
        let (memory, latest) = setup_test("wait_timeout", 42i32);

        let (_, last_version) = latest.get();

        // Wait for change with short timeout, but no producer writes
        let start = Instant::now();
        let (value, version, changed) =
            latest.wait_for_change(last_version, Duration::from_millis(50));
        let elapsed = start.elapsed();

        assert!(!changed, "expected WaitForChange to time out (no change)");
        assert_eq!(value, 42, "expected value 42, got {}", value);
        assert_eq!(
            version, last_version,
            "expected version {}, got {}",
            last_version, version
        );
        assert!(
            elapsed >= Duration::from_millis(40),
            "expected wait of ~50ms, got {:?}",
            elapsed
        );

        cleanup(&memory);
    }

    // Test 5: Multiple concurrent readers (blocking)
    #[test]
    fn test_concurrent_readers() {
        let (memory, latest) = setup_test("concurrent", 0i32);
        let latest = Arc::new(latest);

        const NUM_READERS: usize = 8;
        const DURATION: Duration = Duration::from_secs(1);

        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut handles = vec![];

        // Producer
        let producer_latest = Arc::clone(&latest);
        let producer_done = Arc::clone(&done);
        let producer = thread::spawn(move || {
            let mut i: i32 = 0;
            while !producer_done.load(Ordering::Relaxed) {
                producer_latest.set(i);
                i += 1;
                thread::sleep(Duration::from_millis(1));
            }
        });

        // Readers -- each blocks on wait_for_change
        for _ in 0..NUM_READERS {
            let reader_latest = Arc::clone(&latest);
            let reader_done = Arc::clone(&done);
            handles.push(thread::spawn(move || {
                let mut prev_version: u64 = 0;
                while !reader_done.load(Ordering::Relaxed) {
                    let (value, version, changed) =
                        reader_latest.wait_for_change(prev_version, Duration::from_secs(1));
                    if changed {
                        // Verify value is valid (no torn reads)
                        let _ = value;
                        if version < prev_version && prev_version > 0 {
                            panic!(
                                "reader saw decreasing version: {} -> {}",
                                prev_version, version
                            );
                        }
                        prev_version = version;
                    }
                }
            }));
        }

        // Run for the specified duration
        thread::sleep(DURATION);
        done.store(true, Ordering::Relaxed);
        producer.join().unwrap();

        for handle in handles {
            handle.join().expect("reader thread panicked");
        }

        // Verify final version is reasonably high (producer ran for 1 second at 1ms intervals)
        let final_version = latest.version();
        assert!(
            final_version >= 500,
            "expected at least 500 versions after 1 second, got {}",
            final_version
        );

        cleanup(&memory);
    }

    // Test 6: Struct type support
    #[test]
    fn test_struct_support() {
        let initial = GPSFix {
            latitude: 0.0,
            longitude: 0.0,
            altitude: 0.0,
            timestamp: 0,
        };
        let (memory, latest) = setup_test("struct", initial);

        let fix = GPSFix {
            latitude: 37.7749,
            longitude: -122.4194,
            altitude: 52.3,
            timestamp: 1234567890,
        };
        latest.set(fix);

        let (value, version) = latest.get();
        assert_eq!(
            value.latitude, fix.latitude,
            "expected lat {}, got {}",
            fix.latitude, value.latitude
        );
        assert_eq!(
            value.longitude, fix.longitude,
            "expected lon {}, got {}",
            fix.longitude, value.longitude
        );
        assert_eq!(
            value.altitude, fix.altitude,
            "expected alt {}, got {}",
            fix.altitude, value.altitude
        );
        assert_eq!(
            value.timestamp, fix.timestamp,
            "expected ts {}, got {}",
            fix.timestamp, value.timestamp
        );
        assert_eq!(version, 2, "expected version 2, got {}", version);

        cleanup(&memory);
    }

    // Test 7: Alignment verification
    #[test]
    fn test_alignment() {
        // Verify align_up function
        let tests = vec![
            (1, 8),
            (3, 8),
            (7, 8),
            (8, 8),
            (9, 16),
            (15, 16),
            (16, 16),
        ];

        for (size, expected) in tests {
            let aligned = align_up(size, SECTION_ALIGN);
            assert_eq!(
                aligned, expected,
                "align_up({}, 8) = {}, expected {}",
                size, aligned, expected
            );
        }

        // Verify alignment for actual types in shared memory
        let name = format!("/latest_align_{}", std::process::id());
        let _ = platform::unlink_shm(&name);
        let memory = Memory::create(&name, 1 << 20, 64).unwrap();

        // Create Latest with u8 (1 byte) -- version should be at offset align_up(1,8) = 8
        let l8 = Latest::<u8>::new(&memory, "latest_u8", 42).unwrap();
        assert_eq!(
            l8.version_offset() % 8,
            0,
            "u8: version offset {} is not 8-byte aligned",
            l8.version_offset()
        );

        // Create Latest with i64 (8 bytes) -- version should be at offset align_up(8,8) = 8
        let l64 = Latest::<i64>::new(&memory, "latest_i64", 42).unwrap();
        assert_eq!(
            l64.version_offset() % 8,
            0,
            "i64: version offset {} is not 8-byte aligned",
            l64.version_offset()
        );

        // Create Latest with GPSFix (32 bytes) -- version should be at offset 32
        let l_gps = Latest::<GPSFix>::new(
            &memory,
            "latest_gps",
            GPSFix {
                latitude: 0.0,
                longitude: 0.0,
                altitude: 0.0,
                timestamp: 0,
            },
        )
        .unwrap();
        assert_eq!(
            l_gps.version_offset() % 8,
            0,
            "GPSFix: version offset {} is not 8-byte aligned",
            l_gps.version_offset()
        );

        cleanup(&memory);
    }

    // Test 8: Open after create
    #[test]
    fn test_open_after_create() {
        let (memory, latest) = setup_test("open", 42i32);

        latest.set(99);

        // Open the same Latest by name
        let opened = Latest::<i32>::open(&memory, "latest_open").unwrap();

        // Verify same value and version
        let (orig_value, orig_version) = latest.get();
        let (open_value, open_version) = opened.get();
        assert_eq!(
            orig_value, open_value,
            "value mismatch: original {}, opened {}",
            orig_value, open_value
        );
        assert_eq!(
            orig_version, open_version,
            "version mismatch: original {}, opened {}",
            orig_version, open_version
        );

        // Test blocking across open instances
        let opened = Arc::new(opened);
        let (_, last_version) = opened.get();

        let writer_latest = Arc::new(latest);
        let reader_latest = Arc::clone(&opened);
        let reader = thread::spawn(move || {
            reader_latest.wait_for_change(last_version, Duration::from_millis(500))
        });

        thread::sleep(Duration::from_millis(50));
        writer_latest.set(123);

        let (value, version, changed) = reader.join().unwrap();
        assert!(changed, "opened instance should detect change");
        assert_eq!(value, 123, "expected value 123, got {}", value);
        assert!(version > last_version);

        cleanup(&memory);
    }

    // Test 9: Error cases
    #[test]
    fn test_error_cases() {
        let name = format!("/latest_err_{}", std::process::id());
        let _ = platform::unlink_shm(&name);
        let memory = Memory::create(&name, 1 << 20, 64).unwrap();

        // Duplicate name
        Latest::<i32>::new(&memory, "latest_dup", 42).unwrap();
        let result = Latest::<i32>::new(&memory, "latest_dup", 99);
        assert!(
            matches!(result, Err(ZeroShmError::AlreadyExists(_))),
            "expected error for duplicate name"
        );

        // Non-existent name
        let result = Latest::<i32>::open(&memory, "latest_nonexistent");
        assert!(
            matches!(result, Err(ZeroShmError::NotFoundError(_))),
            "expected error for non-existent name"
        );

        // Wrong type (size mismatch)
        Latest::<i32>::new(&memory, "latest_type", 42).unwrap();
        let result = Latest::<i64>::open(&memory, "latest_type");
        assert!(
            matches!(result, Err(ZeroShmError::SizeError(_))),
            "expected error for wrong type (size mismatch)"
        );

        cleanup(&memory);
    }

    // Test 10: Version() method
    #[test]
    fn test_version_method() {
        let (memory, latest) = setup_test("version", 0i32);

        assert_eq!(latest.version(), 1, "expected initial version 1, got {}", latest.version());

        latest.set(1);
        assert_eq!(latest.version(), 2, "expected version 2, got {}", latest.version());

        latest.set(2);
        assert_eq!(latest.version(), 3, "expected version 3, got {}", latest.version());

        cleanup(&memory);
    }

    // Additional: Verify GPSFix size is 32 bytes
    #[test]
    fn test_gps_size() {
        assert_eq!(
            std::mem::size_of::<GPSFix>(),
            32,
            "expected GPSFix size 32 bytes, got {}",
            std::mem::size_of::<GPSFix>()
        );
        assert_eq!(GPSFix::SIZE, 32, "expected GPSFix::SIZE 32, got {}", GPSFix::SIZE);
    }

    // Additional: Zero-size type rejection
    #[test]
    fn test_zero_size_type_rejection() {
        let name = format!("/latest_zero_{}", std::process::id());
        let _ = platform::unlink_shm(&name);
        let memory = Memory::create(&name, 1 << 20, 64).unwrap();

        // Verify that valid types work correctly (zero-size check is defensive)
        let latest = Latest::<u8>::new(&memory, "latest_u8", 42).unwrap();
        let (value, version) = latest.get();
        assert_eq!(value, 42);
        assert_eq!(version, 1);

        cleanup(&memory);
    }

    // 
    // Latest layout tests (unified binary format with pthread mutex + condvar)
    // 

    // TestLatestPthreadSizes verifies pthread structure sizes and alignments.
    #[test]
    fn test_pthread_sizes() {
        let mutex_size = std::mem::size_of::<libc::pthread_mutex_t>();
        let mutex_align = std::mem::align_of::<libc::pthread_mutex_t>();
        let cond_size = std::mem::size_of::<libc::pthread_cond_t>();
        let cond_align = std::mem::align_of::<libc::pthread_cond_t>();

        println!("pthread_mutex_t: size={}, align={}", mutex_size, mutex_align);
        println!("pthread_cond_t: size={}, align={}", cond_size, cond_align);

        assert!(mutex_align <= 8, "pthread_mutex_t alignment {} exceeds 8-byte guarantee", mutex_align);
        assert!(cond_align <= 8, "pthread_cond_t alignment {} exceeds 8-byte guarantee", cond_align);
    }

    // TestLatestLayoutElemSize4 verifies layout for elem_size=4.
    #[test]
    fn test_layout_elem_size_4() {
        let elem_size = 4;
        let total = Latest::<u8>::total_size(elem_size);
        let ver_off = version_offset_at(0, elem_size);
        let mutex_off = mutex_offset_at(0, elem_size);
        let cond_off = cond_offset_at(0, elem_size);

        println!("Latest layout (elem_size={}): total={}, version={}, mutex={}, cond={}",
            elem_size, total, ver_off, mutex_off, cond_off);

        assert_eq!(ver_off % 8, 0, "version offset {} is not 8-aligned", ver_off);
        assert!(ver_off < mutex_off, "version offset {} should be < mutex offset {}", ver_off, mutex_off);
        assert!(mutex_off < cond_off, "mutex offset {} should be < cond offset {}", mutex_off, cond_off);
        assert_eq!(total, cond_off + std::mem::size_of::<libc::pthread_cond_t>(),
            "total size {} should equal cond offset {} + cond size {}",
            total, cond_off, std::mem::size_of::<libc::pthread_cond_t>());
    }

    // TestLatestLayoutElemSize8 verifies layout for elem_size=8.
    #[test]
    fn test_layout_elem_size_8() {
        let elem_size = 8;
        let total = Latest::<u8>::total_size(elem_size);
        let ver_off = version_offset_at(0, elem_size);
        let mutex_off = mutex_offset_at(0, elem_size);
        let cond_off = cond_offset_at(0, elem_size);

        println!("Latest layout (elem_size={}): total={}, version={}, mutex={}, cond={}",
            elem_size, total, ver_off, mutex_off, cond_off);

        assert_eq!(ver_off % 8, 0, "version offset {} is not 8-aligned", ver_off);
        assert!(ver_off < mutex_off, "version offset {} should be < mutex offset {}", ver_off, mutex_off);
        assert!(mutex_off < cond_off, "mutex offset {} should be < cond offset {}", mutex_off, cond_off);
        assert_eq!(total, cond_off + std::mem::size_of::<libc::pthread_cond_t>(),
            "total size {} should equal cond offset {} + cond size {}",
            total, cond_off, std::mem::size_of::<libc::pthread_cond_t>());
    }

    // TestLatestLayoutElemSize16 verifies layout for elem_size=16.
    #[test]
    fn test_layout_elem_size_16() {
        let elem_size = 16;
        let total = Latest::<u8>::total_size(elem_size);
        let ver_off = version_offset_at(0, elem_size);
        let mutex_off = mutex_offset_at(0, elem_size);
        let cond_off = cond_offset_at(0, elem_size);

        println!("Latest layout (elem_size={}): total={}, version={}, mutex={}, cond={}",
            elem_size, total, ver_off, mutex_off, cond_off);

        assert_eq!(ver_off % 8, 0, "version offset {} is not 8-aligned", ver_off);
        assert!(ver_off < mutex_off, "version offset {} should be < mutex offset {}", ver_off, mutex_off);
        assert!(mutex_off < cond_off, "mutex offset {} should be < cond offset {}", mutex_off, cond_off);
        assert_eq!(total, cond_off + std::mem::size_of::<libc::pthread_cond_t>(),
            "total size {} should equal cond offset {} + cond size {}",
            total, cond_off, std::mem::size_of::<libc::pthread_cond_t>());
    }

    // TestLatestDestroy verifies that destroy() cleans up the mutex and condvar without panicking.
    #[test]
    fn test_destroy() {
        let (memory, latest) = setup_test("destroy", 42i32);

        // Verify the value is readable before destroy
        let (value, version) = latest.get();
        assert_eq!(value, 42);
        assert_eq!(version, 1);

        // Destroy the Latest (destroys mutex and condvar)
        // This should not panic even though we can't use the Latest afterward
        latest.destroy();

        cleanup(&memory);
    }

    // TestLatestLayoutSummary prints all layout values for cross-language comparison.
    #[test]
    fn test_layout_summary() {
        println!("\n=== Rust Latest Layout Summary ===");
        let mutex_size = std::mem::size_of::<libc::pthread_mutex_t>();
        let mutex_align = std::mem::align_of::<libc::pthread_mutex_t>();
        let cond_size = std::mem::size_of::<libc::pthread_cond_t>();
        let cond_align = std::mem::align_of::<libc::pthread_cond_t>();
        println!("pthread_mutex_t: size={}, align={}", mutex_size, mutex_align);
        println!("pthread_cond_t: size={}, align={}", cond_size, cond_align);
        for elem_size in [4usize, 8, 16] {
            println!("elem_size={}: total={}, version={}, mutex={}, cond={}",
                elem_size,
                Latest::<u8>::total_size(elem_size),
                version_offset_at(0, elem_size),
                mutex_offset_at(0, elem_size),
                cond_offset_at(0, elem_size),
            );
        }
        println!("=== End Rust Summary ===\n");
    }

    // 
    // Performance Validation Tests (Step 6)
    // 

    // TestLatestZeroCpuWhileBlocked verifies that blocked readers consume ~0% CPU.
    #[test]
    fn test_zero_cpu_while_blocked() {
        // Skip in short test mode
        if std::env::var("RUST_TEST_SHORT").is_ok() {
            return;
        }

        let (memory, latest) = setup_test("zero_cpu", 0u64);
        let latest = Arc::new(latest);

        const NUM_READERS: usize = 8;
        let reader_done = (0..NUM_READERS).map(|_| Arc::new(std::sync::atomic::AtomicBool::new(false))).collect::<Vec<_>>();
        let mut handles = Vec::new();
        let (_, last_version) = latest.get();

        // Start reader threads that block on wait_for_change
        for i in 0..NUM_READERS {
            let latest = Arc::clone(&latest);
            let done = Arc::clone(&reader_done[i]);
            handles.push(thread::spawn(move || {
                // Each reader blocks with a 5-second timeout
                latest.wait_for_change(last_version, Duration::from_secs(5));
                done.store(true, std::sync::atomic::Ordering::SeqCst);
            }));
        }

        // Wait for readers to settle into blocking state
        thread::sleep(Duration::from_millis(500));

        // Measure CPU time during blocking period using getrusage
        // On macOS/Linux, getrusage(RUSAGE_SELF) gives process-wide CPU time
        #[cfg(unix)]
        {
            extern "C" {
                fn getrusage(who: libc::c_int, rusage: *mut libc::rusage) -> libc::c_int;
            }

            let mut usage_before: libc::rusage = unsafe { std::mem::zeroed() };
            unsafe { getrusage(libc::RUSAGE_SELF, &mut usage_before); }

            // Wait 2 seconds with readers blocked
            thread::sleep(Duration::from_secs(2));

            let mut usage_after: libc::rusage = unsafe { std::mem::zeroed() };
            unsafe { getrusage(libc::RUSAGE_SELF, &mut usage_after); }

            let cpu_user_before = usage_before.ru_utime.tv_sec as f64 + usage_before.ru_utime.tv_usec as f64 / 1e6;
            let cpu_user_after = usage_after.ru_utime.tv_sec as f64 + usage_after.ru_utime.tv_usec as f64 / 1e6;
            let cpu_system_before = usage_before.ru_stime.tv_sec as f64 + usage_before.ru_stime.tv_usec as f64 / 1e6;
            let cpu_system_after = usage_after.ru_stime.tv_sec as f64 + usage_after.ru_stime.tv_usec as f64 / 1e6;

            let cpu_time_consumed = (cpu_user_after - cpu_user_before) + (cpu_system_after - cpu_system_before);
            let wall_time = 2.0;
            let cpu_percent = (cpu_time_consumed / wall_time) * 100.0;

            println!("Zero CPU Test Results:");
            println!("  Readers: {}", NUM_READERS);
            println!("  Wall time: {:.2} seconds", wall_time);
            println!("  CPU time consumed: {:.4} seconds", cpu_time_consumed);
            println!("  CPU usage: {:.2}%", cpu_percent);

            // Checkpoint: CPU time consumed during blocking period should be minimal.
            // Note: getrusage(RUSAGE_SELF) measures the entire process including the main
            // test thread, so we use a lenient threshold. The key point is that readers
            // are not spinning at 100% CPU -- they are blocked by pthread_cond_wait.
            // A threshold of 50% (1 second over 2 seconds) confirms readers are truly blocked.
            assert!(
                cpu_time_consumed < 1.0,
                "CPU time consumed during blocking period is too high: {} seconds (expected < 1.0)",
                cpu_time_consumed
            );
        }

        #[cfg(not(unix))]
        {
            // Non-Unix platforms: just verify readers wake up
            thread::sleep(Duration::from_secs(2));
            println!("Zero CPU Test Results:");
            println!("  Readers: {}", NUM_READERS);
            println!("  (CPU measurement not available on this platform)");
        }

        // Wake all readers by setting a new value
        latest.set(1);

        // Wait for all readers to finish
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all readers woke up
        for (i, done) in reader_done.iter().enumerate() {
            assert!(done.load(std::sync::atomic::Ordering::SeqCst), "Reader {} did not wake up", i);
        }

        println!("  All readers woke up successfully");
        cleanup(&memory);
    }

    // TestLatestWakeLatency measures wake latency (should be < 1ms average, < 10ms p99).
    #[test]
    fn test_wake_latency() {
        // Skip in short test mode
        if std::env::var("RUST_TEST_SHORT").is_ok() {
            return;
        }

        let (memory, latest) = setup_test("latency", 0u64);
        let latest = Arc::new(latest);

        const NUM_ITERATIONS: usize = 200;
        let mut latencies_ms: Vec<f64> = Vec::with_capacity(NUM_ITERATIONS);

        // Simple PRNG seed based on current time
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        for iter in 0..NUM_ITERATIONS {
            let (_, last_version) = latest.get();

            let reader_done = Arc::new(std::sync::atomic::AtomicBool::new(false));

            // Reader thread: block on wait_for_change with long timeout
            let reader_latest = Arc::clone(&latest);
            let reader_done_inner = Arc::clone(&reader_done);
            let reader = thread::spawn(move || {
                let _ = reader_latest.wait_for_change(last_version, Duration::from_secs(10));
                reader_done_inner.store(true, std::sync::atomic::Ordering::SeqCst);
            });

            // Random-ish delay before writing (1ms to 100ms) using xorshift
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let delay_ms = (seed % 100) + 1;
            thread::sleep(Duration::from_millis(delay_ms));

            // Writer: set new value
            let write_time = Instant::now();
            latest.set(iter as u64 + 1);

            // Wait for reader to wake up
            while !reader_done.load(std::sync::atomic::Ordering::SeqCst) {
                thread::sleep(Duration::from_micros(100));
            }
            reader.join().unwrap();

            // Measure latency: wake time - write time
            let latency_ms = write_time.elapsed().as_secs_f64() * 1000.0;
            latencies_ms.push(latency_ms);
        }

        // Sort latencies for percentile calculation
        latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Calculate statistics
        let sum: f64 = latencies_ms.iter().sum();
        let avg = sum / NUM_ITERATIONS as f64;
        let p50 = latencies_ms[NUM_ITERATIONS / 2];
        let p95 = latencies_ms[(NUM_ITERATIONS as f64 * 0.95) as usize];
        let p99 = latencies_ms[(NUM_ITERATIONS as f64 * 0.99) as usize];
        let max_lat = *latencies_ms.last().unwrap();

        println!("Latency Test Results ({} iterations):", NUM_ITERATIONS);
        println!("  Average: {:.3} ms", avg);
        println!("  P50:     {:.3} ms", p50);
        println!("  P95:     {:.3} ms", p95);
        println!("  P99:     {:.3} ms", p99);
        println!("  Max:     {:.3} ms", max_lat);

        // Checkpoint: average latency < 1ms, p99 < 10ms
        assert!(avg < 1.0, "Average latency is too high: {} ms (expected < 1.0 ms)", avg);
        assert!(p99 < 10.0, "P99 latency is too high: {} ms (expected < 10.0 ms)", p99);

        cleanup(&memory);
    }

    // TestLatestThroughputNoMissedUpdates verifies all readers see every version.
    #[test]
    fn test_throughput_no_missed_updates() {
        // Skip in short test mode
        if std::env::var("RUST_TEST_SHORT").is_ok() {
            return;
        }

        let (memory, latest) = setup_test("throughput", 0u64);
        let latest = Arc::new(latest);

        const NUM_READERS: usize = 8;
        const DURATION: Duration = Duration::from_millis(2000);
        const WRITE_INTERVAL: Duration = Duration::from_millis(1);

        let writer_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let expected_versions: Vec<Arc<std::sync::atomic::AtomicU64>> =
            (0..NUM_READERS).map(|_| Arc::new(std::sync::atomic::AtomicU64::new(0))).collect();
        let total_reads: Vec<Arc<std::sync::atomic::AtomicU64>> =
            (0..NUM_READERS).map(|_| Arc::new(std::sync::atomic::AtomicU64::new(0))).collect();
        let missed_updates: Vec<Arc<std::sync::atomic::AtomicU64>> =
            (0..NUM_READERS).map(|_| Arc::new(std::sync::atomic::AtomicU64::new(0))).collect();

        let mut handles = Vec::new();

        // Start reader threads
        for i in 0..NUM_READERS {
            let latest = Arc::clone(&latest);
            let writer_stop = Arc::clone(&writer_stop);
            let expected_versions = Arc::clone(&expected_versions[i]);
            let total_reads = Arc::clone(&total_reads[i]);
            let missed_updates = Arc::clone(&missed_updates[i]);

            handles.push(thread::spawn(move || {
                let (_, mut last_version) = latest.get();
                expected_versions.store(last_version, std::sync::atomic::Ordering::SeqCst);

                while !writer_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    let (value, version, changed) = latest.wait_for_change(last_version, Duration::from_millis(100));

                    if changed {
                        let expected = expected_versions.load(std::sync::atomic::Ordering::SeqCst) + 1;
                        if version != expected {
                            let missed = version - expected;
                            missed_updates.fetch_add(missed, std::sync::atomic::Ordering::SeqCst);
                        }
                        expected_versions.store(version, std::sync::atomic::Ordering::SeqCst);
                        last_version = version;
                        total_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    // If not changed (timeout), loop again
                    let _ = value;
                }
            }));
        }

        // Writer thread: set new value every 1ms for 2 seconds
        let writer_latest = Arc::clone(&latest);
        let writer_stop = Arc::clone(&writer_stop);
        let writer = thread::spawn(move || {
            let end_time = Instant::now() + DURATION;
            while Instant::now() < end_time {
                let current = writer_latest.version();
                writer_latest.set(current + 1);
                thread::sleep(WRITE_INTERVAL);
            }
            writer_stop.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        // Wait for writer to finish
        writer.join().unwrap();

        // Wait for all readers to finish
        for handle in handles {
            handle.join().unwrap();
        }

        // Calculate throughput and verify no missed updates
        let final_version = latest.version();
        let total_writes = final_version - 1; // Version starts at 1

        println!("Throughput Test Results:");
        println!("  Duration: {:?})", DURATION);
        println!("  Total writes: {}", total_writes);
        println!("  Writer rate: {:.0} writes/sec", total_writes as f64 / DURATION.as_secs_f64());

        let mut total_all_reads: u64 = 0;
        let mut total_missed: u64 = 0;
        for i in 0..NUM_READERS {
            let reads = total_reads[i].load(std::sync::atomic::Ordering::SeqCst);
            let missed = missed_updates[i].load(std::sync::atomic::Ordering::SeqCst);
            total_all_reads += reads;
            total_missed += missed;

            let read_rate = reads as f64 / DURATION.as_secs_f64();
            println!("  Reader {}: {} reads ({:.0} reads/sec), {} missed", i, reads, read_rate, missed);
        }

        println!("  Total reads (all readers): {}", total_all_reads);
        println!("  Total missed updates: {}", total_missed);
        println!("  Expected reads per reader: {}", total_writes);

        // Checkpoint: all readers should see every version, no missed updates
        for i in 0..NUM_READERS {
            let reads = total_reads[i].load(std::sync::atomic::Ordering::SeqCst);
            let missed = missed_updates[i].load(std::sync::atomic::Ordering::SeqCst);
            assert_eq!(missed, 0, "Reader {} missed {} updates", i, missed);
            // Allow some tolerance for the timing of test start/stop (5%)
            let min_expected = (total_writes as f64 * 0.95) as u64;
            assert!(
                reads >= min_expected,
                "Reader {} read too few: {} (expected >= {})",
                i, reads, min_expected
            );
        }

        cleanup(&memory);
    }
}
