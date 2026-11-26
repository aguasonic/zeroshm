//! Linux shared memory backend using /dev/shm.

use super::SharedMemoryBackend;
use crate::error::{Result, ZeroShmError};
use std::ffi::CString;
use std::os::unix::io::RawFd;

/// Linux backend implementation using /dev/shm files.
pub struct LinuxBackend;

/// Convert a name to a /dev/shm path as a NUL-terminated C string.
fn to_devshm_path(name: &str) -> CString {
    let path = format!("/dev/shm{}", name);
    CString::new(path).expect("path must not contain NUL bytes")
}

impl SharedMemoryBackend for LinuxBackend {
    fn create(name: &str, size: usize) -> Result<(RawFd, *mut u8, usize)> {
        let c_path = to_devshm_path(name);
        let mode: libc::c_uint = (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint;

        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                mode,
            )
        };

        let fd = if fd < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EEXIST) {
                // Segment exists -- unlink and retry
                unsafe { libc::unlink(c_path.as_ptr()) };
                let fd = unsafe {
                    libc::open(
                        c_path.as_ptr(),
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
            unsafe { libc::close(fd) };
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
            unsafe { libc::close(fd) };
            return Err(ZeroShmError::MmapError(format!("mmap failed: {}", err)));
        }

        Ok((fd, data as *mut u8, size))
    }

    fn open(name: &str) -> Result<(RawFd, *mut u8, usize)> {
        let c_path = to_devshm_path(name);

        let fd = unsafe {
            libc::open(c_path.as_ptr(), libc::O_RDWR)
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
        let c_path = to_devshm_path(name);
        if unsafe { libc::unlink(c_path.as_ptr()) } < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ENOENT) {
                return Ok(()); // Already doesn't exist
            }
            return Err(ZeroShmError::IoError(err));
        }
        Ok(())
    }
}
