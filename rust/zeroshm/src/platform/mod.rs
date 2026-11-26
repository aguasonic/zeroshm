//! Platform-specific shared memory backend.
//!
//! Provides a unified interface for creating, opening, closing, and
//! unlinking shared memory segments on both Linux and macOS.

use crate::error::Result;
use crate::hash;
use std::os::unix::io::RawFd;

/// Trait defining the shared memory backend operations.
pub trait SharedMemoryBackend {
    /// Create a new shared memory segment with the given name and size.
    /// Returns the file descriptor, mapped pointer, and actual size.
    fn create(name: &str, size: usize) -> Result<(RawFd, *mut u8, usize)>;

    /// Open an existing shared memory segment.
    /// Returns the file descriptor, mapped pointer, and size.
    fn open(name: &str) -> Result<(RawFd, *mut u8, usize)>;

    /// Close a shared memory segment and unmap the memory.
    fn close(fd: RawFd, data: *mut u8, len: usize) -> Result<()>;

    /// Unlink (delete) a shared memory segment by name.
    fn unlink(name: &str) -> Result<()>;
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::LinuxBackend;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::MacosBackend;

/// Convert a logical segment name to a short OS-level name suitable for shm_open.
/// Uses xxhash3-64 with the ZeroSHM seed to produce a deterministic short name.
/// Matches Go's `osSegmentName`: "/z" + 16 hex digits = 18 chars.
fn os_segment_name(name: &str) -> String {
    let normalized = if name.is_empty() || !name.starts_with('/') {
        format!("/{}", name)
    } else {
        name.to_string()
    };
    let h = hash::hash_name(&normalized);
    format!("/z{:016x}", h)
}

/// Create a new shared memory segment.
pub fn create_shm(name: &str, size: usize) -> Result<(RawFd, *mut u8, usize)> {
    let os_name = os_segment_name(name);
    #[cfg(target_os = "linux")]
    {
        LinuxBackend::create(&os_name, size)
    }
    #[cfg(target_os = "macos")]
    {
        MacosBackend::create(&os_name, size)
    }
}

/// Open an existing shared memory segment.
pub fn open_shm(name: &str) -> Result<(RawFd, *mut u8, usize)> {
    let os_name = os_segment_name(name);
    #[cfg(target_os = "linux")]
    {
        LinuxBackend::open(&os_name)
    }
    #[cfg(target_os = "macos")]
    {
        MacosBackend::open(&os_name)
    }
}

/// Close a shared memory segment.
pub fn close_shm(fd: RawFd, data: *mut u8, len: usize) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        LinuxBackend::close(fd, data, len)
    }
    #[cfg(target_os = "macos")]
    {
        MacosBackend::close(fd, data, len)
    }
}

/// Unlink a shared memory segment.
pub fn unlink_shm(name: &str) -> Result<()> {
    let os_name = os_segment_name(name);
    #[cfg(target_os = "linux")]
    {
        LinuxBackend::unlink(&os_name)
    }
    #[cfg(target_os = "macos")]
    {
        MacosBackend::unlink(&os_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZeroShmError;

    #[test]
    fn test_create_open_close_unlink() {
        let name = "/test_step4_rust";
        let size = 4096;

        // Clean up any existing segment
        let _ = unlink_shm(name);

        // Create
        let (fd, data, len) = create_shm(name, size).expect("create should succeed");
        assert!(fd >= 0, "fd should be valid");
        assert!(!data.is_null(), "data pointer should not be null");
        assert_eq!(len, size, "size should match");

        // Write to mapped memory
        unsafe {
            *data = 0xAB;
            *data.add(100) = 0xCD;
        }

        // Close
        close_shm(fd, data, len).expect("close should succeed");

        // Reopen and verify data persists
        // Note: on macOS, fstat may report a larger size than requested
        // (shared memory is allocated in larger chunks), so we check >=
        let (fd2, data2, len2) = open_shm(name).expect("reopen should succeed");
        assert!(
            len2 >= size,
            "reopened size {} should be >= requested {}",
            len2,
            size
        );
        unsafe {
            assert_eq!(*data2, 0xAB, "data should persist after close/reopen");
            assert_eq!(*data2.add(100), 0xCD);
        }
        close_shm(fd2, data2, len2).expect("close should succeed");

        // Unlink
        unlink_shm(name).expect("unlink should succeed");

        // Opening a non-existent segment should return NotFoundError
        let result = open_shm(name);
        assert!(
            matches!(result, Err(ZeroShmError::NotFoundError(_))),
            "opening non-existent segment should return NotFoundError, got {:?}",
            result
        );
    }

    #[test]
    fn test_create_existing_segment() {
        let name = "/test_step4_existing";
        let size = 4096;

        // Clean up any existing segment
        let _ = unlink_shm(name);

        // Create first time
        let (fd, data, len) = create_shm(name, size).expect("first create should succeed");
        close_shm(fd, data, len).expect("close should succeed");

        // Create again (should unlink and recreate)
        let (fd2, data2, len2) = create_shm(name, size).expect("second create should succeed");
        assert_eq!(len2, size);
        close_shm(fd2, data2, len2).expect("close should succeed");

        // Clean up
        unlink_shm(name).expect("unlink should succeed");
    }

    #[test]
    fn test_unlink_nonexistent() {
        // Unlinking a non-existent segment should succeed (no error)
        let result = unlink_shm("/test_step4_nonexistent_xyz");
        assert!(result.is_ok(), "unlinking non-existent segment should not error");
    }
}
