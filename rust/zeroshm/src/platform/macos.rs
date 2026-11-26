//! macOS shared memory backend using POSIX shm_open.

use super::SharedMemoryBackend;
use crate::error::{Result, ZeroShmError};
use std::ffi::CString;
use std::os::unix::io::RawFd;

/// macOS POSIX shared memory name limit (including leading '/').
const MAX_SHM_NAME_LEN: usize = 31;

/// macOS backend implementation using POSIX shm_open.
pub struct MacosBackend;

/// Convert a Rust string to a NUL-terminated C string.
fn to_cstring(name: &str) -> CString {
    CString::new(name).expect("shared memory name must not contain NUL bytes")
}

impl SharedMemoryBackend for MacosBackend {
    fn create(name: &str, size: usize) -> Result<(RawFd, *mut u8, usize)> {
        if name.len() > MAX_SHM_NAME_LEN - 1 {
            return Err(ZeroShmError::NameTooLong(
                name.len(),
                MAX_SHM_NAME_LEN - 1,
            ));
        }

        let c_name = to_cstring(name);
        let mode: libc::c_uint = (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint;

        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                mode,
            )
        };

        let fd = if fd < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EEXIST) {
                // Segment exists -- unlink and retry
                unsafe { libc::shm_unlink(c_name.as_ptr()) };
                let fd = unsafe {
                    libc::shm_open(
                        c_name.as_ptr(),
                        libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                        mode,
                    )
                };
                if fd < 0 {
                    return Err(ZeroShmError::OpenError(format!(
                        "failed to create {} after unlink: {}",
                        name,
                        std::io::Error::last_os_error()
                    )));
                }
                fd
            } else {
                return Err(ZeroShmError::OpenError(format!(
                    "failed to create {}: {}",
                    name, err
                )));
            }
        } else {
            fd
        };

        // Set the file size
        if unsafe { libc::ftruncate(fd, size as libc::off_t) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
            };
            return Err(ZeroShmError::SizeError(format!(
                "failed to set size: {}",
                err
            )));
        }

        // Memory-map the file
        let data = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if data == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
            };
            return Err(ZeroShmError::MmapError(format!("mmap failed: {}", err)));
        }

        Ok((fd, data as *mut u8, size))
    }

    fn open(name: &str) -> Result<(RawFd, *mut u8, usize)> {
        if name.len() > MAX_SHM_NAME_LEN - 1 {
            return Err(ZeroShmError::NameTooLong(
                name.len(),
                MAX_SHM_NAME_LEN - 1,
            ));
        }

        let c_name = to_cstring(name);

        let fd = unsafe {
            libc::shm_open(c_name.as_ptr(), libc::O_RDWR)
        };

        if fd < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ENOENT) {
                return Err(ZeroShmError::NotFoundError(name.to_string()));
            }
            return Err(ZeroShmError::OpenError(format!(
                "failed to open {}: {}",
                name, err
            )));
        }

        // Get file size
        let mut stat;
        unsafe {
            stat = std::mem::zeroed();
            if libc::fstat(fd, &mut stat) < 0 {
                let err = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(ZeroShmError::IoError(err));
            }
        }
        let size = stat.st_size as usize;

        // Memory-map the file
        let data = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if data == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(ZeroShmError::MmapError(format!("mmap failed: {}", err)));
        }

        Ok((fd, data as *mut u8, size))
    }

    fn close(fd: RawFd, data: *mut u8, len: usize) -> Result<()> {
        if !data.is_null() {
            unsafe { libc::munmap(data as *mut libc::c_void, len) };
        }
        if fd >= 0 {
            unsafe { libc::close(fd) };
        }
        Ok(())
    }

    fn unlink(name: &str) -> Result<()> {
        let c_name = to_cstring(name);
        if unsafe { libc::shm_unlink(c_name.as_ptr()) } < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ENOENT) {
                return Ok(()); // Already doesn't exist
            }
            return Err(ZeroShmError::IoError(err));
        }
        Ok(())
    }
}

