//! Memory segment management.
//!
//! Provides the `Memory` struct that wraps a shared memory segment with
//! automatic cleanup, safe slice access, and bounds-checked pointer arithmetic.
//! The struct tracks the *requested* size for allocation purposes (important
//! on macOS where fstat may report a larger actual size due to chunked
//! allocation), while using the actual mapped length for slice access.

use crate::error::{Result, ZeroShmError};
use crate::platform;
use crate::table::Table;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Manages a shared memory segment with automatic cleanup.
///
/// # Safety
///
/// `Memory` is `Send + Sync` because mmap'd memory with `MAP_SHARED` is
/// process-shared and can be safely accessed from multiple threads and
/// processes. All raw pointer access is confined to internal methods that
/// perform bounds checking.
pub struct Memory {
    /// Name of the shared memory segment.
    name: String,
    /// Requested size in bytes (used for allocation tracking).
    size: usize,
    /// Maximum number of table entries.
    max_entries: usize,
    /// File descriptor for the shared memory segment.
    fd: RawFd,
    /// Pointer to the mapped memory region.
    data: *mut u8,
    /// Actual length of the mapped region (may be larger than `size` on macOS).
    data_len: usize,
    /// True if this instance created (owns) the segment.
    owner: bool,
    /// Metadata table for named structures.
    table: Table,
    /// Shared reference count to ensure cleanup (close fd + munmap) happens
    /// only when the last Memory instance sharing the segment is dropped.
    shared_refs: Arc<AtomicUsize>,
}

// SAFETY: mmap'd memory with MAP_SHARED is process-shared and can be safely
// accessed from multiple threads and processes. All pointer access through
// this struct is bounds-checked.
unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

impl Memory {
    /// Create a new shared memory segment.
    ///
    /// The segment is created with the given name and size. On macOS, memory
    /// is zeroed after mapping because POSIX shm segments are not guaranteed
    /// to be zero-filled.
    ///
    /// # Arguments
    /// * `name` -- Segment name (must start with `/` on POSIX systems)
    /// * `size` -- Size in bytes
    /// * `max_entries` -- Maximum number of table entries
    pub fn create(name: &str, size: usize, max_entries: usize) -> Result<Self> {
        if size == 0 {
            return Err(ZeroShmError::SizeError(
                "size must be greater than zero".to_string(),
            ));
        }

        let (fd, data, data_len) = platform::create_shm(name, size)?;

        // Zero out memory on macOS (not guaranteed to be zero-filled)
        #[cfg(target_os = "macos")]
        unsafe {
            std::ptr::write_bytes(data, 0, data_len);
        }

        // Initialize the metadata table
        let table = Table::new(data, max_entries, size, true)?;

        Ok(Memory {
            name: name.to_string(),
            size,
            max_entries,
            fd,
            data,
            data_len,
            owner: true,
            table,
            shared_refs: Arc::new(AtomicUsize::new(1)),
        })
    }

    /// Open an existing shared memory segment.
    ///
    /// The actual size is determined via fstat on the file descriptor.
    ///
    /// # Arguments
    /// * `name` -- Segment name
    /// * `max_entries` -- Maximum number of table entries
    pub fn open(name: &str, max_entries: usize) -> Result<Self> {
        let (fd, data, data_len) = platform::open_shm(name)?;

        // Open existing table (passes 0 for memory_size; table will use stored value)
        let table = Table::new(data, max_entries, 0, false)?;

        Ok(Memory {
            name: name.to_string(),
            size: data_len,
            max_entries,
            fd,
            data,
            data_len,
            owner: false,
            table,
            shared_refs: Arc::new(AtomicUsize::new(1)),
        })
    }

    /// Create a shallow clone of this Memory instance.
    ///
    /// The clone shares the same file descriptor, mapped pointer, and
    /// reference count. This is used when wrapping a `&Memory` in an
    /// `Arc<Memory>` for use by structures like `Array`, `Queue`, etc.
    /// The shared reference count ensures that `close`/`munmap` is performed
    /// only when the last `Memory` instance (original or clone) is dropped.
    ///
    /// # Safety
    ///
    /// This is safe because mmap'd memory with `MAP_SHARED` is process-shared.
    /// Multiple `Memory` instances pointing to the same mapping can coexist,
    /// and the atomic reference count guarantees cleanup happens exactly once.
    pub fn clone_shallow(&self) -> Self {
        // Increment the shared reference count so cleanup is deferred
        // until the last Memory instance (original or clone) is dropped.
        self.shared_refs.fetch_add(1, Ordering::AcqRel);
        Memory {
            name: self.name.clone(),
            size: self.size,
            max_entries: self.max_entries,
            fd: self.fd,
            data: self.data,
            data_len: self.data_len,
            owner: self.owner,
            table: self.table.clone_shallow(),
            shared_refs: self.shared_refs.clone(),
        }
    }

    /// Unlink (delete) the shared memory segment.
    ///
    /// The segment is removed from the system but remains accessible to
    /// processes that have already opened it.
    pub fn unlink(&self) -> Result<()> {
        platform::unlink_shm(&self.name)
    }

    /// Returns a shared slice of the mapped memory region.
    pub fn data(&self) -> &[u8] {
        // SAFETY: `data` is a valid pointer to `data_len` bytes obtained
        // from mmap with PROT_READ | PROT_WRITE. The slice is immutable,
        // and all accesses are within bounds.
        unsafe { std::slice::from_raw_parts(self.data, self.data_len) }
    }

    /// Returns a mutable slice of the mapped memory region.
    pub fn data_mut(&self) -> &mut [u8] {
        // SAFETY: `data` is a valid pointer to `data_len` bytes obtained
        // from mmap with PROT_READ | PROT_WRITE. We provide exclusive
        // mutable access through &self because the memory is process-shared;
        // callers are responsible for synchronization across processes.
        unsafe { std::slice::from_raw_parts_mut(self.data, self.data_len) }
    }

    /// Returns a bounds-checked pointer to the memory at the given offset.
    ///
    /// # Panics
    /// Panics if the offset is out of bounds.
    pub fn at(&self, offset: usize) -> *const u8 {
        if offset >= self.data_len {
            panic!(
                "offset {} out of bounds (size={})",
                offset, self.data_len
            );
        }
        // SAFETY: offset has been bounds-checked against data_len.
        unsafe { self.data.add(offset) }
    }

    /// Returns a bounds-checked mutable pointer to the memory at the given offset.
    ///
    /// # Panics
    /// Panics if the offset is out of bounds.
    pub fn at_mut(&self, offset: usize) -> *mut u8 {
        if offset >= self.data_len {
            panic!(
                "offset {} out of bounds (size={})",
                offset, self.data_len
            );
        }
        // SAFETY: offset has been bounds-checked against data_len.
        unsafe { self.data.add(offset) }
    }

    /// Returns the name of the shared memory segment.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the requested size of the shared memory segment.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the actual length of the mapped region.
    pub fn data_len(&self) -> usize {
        self.data_len
    }

    /// Returns the maximum number of table entries.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Returns true if this instance created (owns) the segment.
    pub fn is_owner(&self) -> bool {
        self.owner
    }

    /// Returns a reference to the metadata table.
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// Reserve space in shared memory and add an entry to the table.
    ///
    /// # Arguments
    /// * `name` -- Name for the structure
    /// * `size` -- Size in bytes to allocate
    ///
    /// # Returns
    /// The offset of the allocated space.
    pub fn allocate(&self, name: &str, size: usize) -> Result<u64> {
        let align = 8;
        let offset = self.table.allocate(size, align)?;
        self.table.add(name, offset, size as u64)?;
        Ok(offset)
    }

    /// Look up a named structure in the table.
    ///
    /// Returns the entry's offset and size if found.
    pub fn find(&self, name: &str) -> Result<Option<(u64, u64)>> {
        match self.table.find(name)? {
            Some(entry) => Ok(Some((entry.offset, entry.size))),
            None => Ok(None),
        }
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        // Decrement the shared reference count. Cleanup (close fd + munmap)
        // only happens when this was the last Memory instance sharing the
        // segment, preventing premature cleanup that would invalidate other
        // live references.
        if self.shared_refs.fetch_sub(1, Ordering::AcqRel) == 1 {
            if !self.data.is_null() {
                let _ = platform::close_shm(self.fd, self.data, self.data_len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_write_drop_reopen() {
        let name = "/test_step5_rust";
        let size = 1024 * 1024; // 1MB
        let pattern: u8 = 0xAB;

        // Clean up any existing segment
        let _ = platform::unlink_shm(name);

        // Create a 1MB segment
        let mem = Memory::create(name, size, 64).expect("create should succeed");
        assert_eq!(mem.size(), size);
        assert!(mem.data_len() >= size);
        assert!(mem.is_owner());

        // Write a known pattern to offset 100
        let data = mem.data_mut();
        data[100] = pattern;
        data[101] = 0xCD;
        data[102] = 0xEF;

        // Drop the Memory (closes fd and unmaps, but segment persists)
        drop(mem);

        // Reopen the segment
        let mem2 = Memory::open(name, 64).expect("reopen should succeed");
        assert!(!mem2.is_owner());

        // Verify data persists across drop/reopen
        let data2 = mem2.data();
        assert_eq!(data2[100], pattern, "pattern should persist after drop/reopen");
        assert_eq!(data2[101], 0xCD);
        assert_eq!(data2[102], 0xEF);

        // Clean up
        mem2.unlink().expect("unlink should succeed");
        drop(mem2);

        // Verify segment is gone
        let result = Memory::open(name, 64);
        assert!(
            matches!(result, Err(ZeroShmError::NotFoundError(_))),
            "opening unlinked segment should return NotFoundError"
        );
    }

    #[test]
    fn test_bounds_check() {
        let name = "/test_step5_bounds";
        let size = 64 * 1024; // 64KB (needs to fit table: 32 + 64*384 = 24608)

        let _ = platform::unlink_shm(name);
        let mem = Memory::create(name, size, 64).expect("create should succeed");

        // Valid offset
        let ptr = mem.at(0);
        assert!(!ptr.is_null());

        // Out of bounds should panic
        let result = std::panic::catch_unwind(|| {
            mem.at(size + 1);
        });
        assert!(result.is_err(), "out-of-bounds access should panic");

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_at_mut() {
        let name = "/test_step5_at_mut";
        let size = 64 * 1024; // 64KB (needs to fit table: 32 + 64*384 = 24608)

        let _ = platform::unlink_shm(name);
        let mem = Memory::create(name, size, 64).expect("create should succeed");

        // Write via at_mut
        unsafe {
            *mem.at_mut(50) = 0x42;
            *mem.at_mut(51) = 0x43;
        }

        // Read via data()
        let data = mem.data();
        assert_eq!(data[50], 0x42);
        assert_eq!(data[51], 0x43);

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }
}
