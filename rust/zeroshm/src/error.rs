//! Error types for ZeroSHM operations.

use thiserror::Error;

/// Error type for all ZeroSHM operations.
#[derive(Error, Debug)]
pub enum ZeroShmError {
    /// Failed to open a shared memory segment.
    #[error("failed to open shared memory segment: {0}")]
    OpenError(String),

    /// Failed to memory-map a shared memory segment.
    #[error("failed to mmap shared memory: {0}")]
    MmapError(String),

    /// Invalid size specified for a shared memory segment or structure.
    #[error("invalid size: {0}")]
    SizeError(String),

    /// Structure or segment not found.
    #[error("not found: {0}")]
    NotFoundError(String),

    /// Table is full; no more entries can be added.
    #[error("table is full")]
    TableFull,

    /// Name is shorter than the minimum length of 3 characters.
    #[error("name too short: {0} characters (min {1})")]
    NameTooShort(usize, usize),

    /// Name exceeds the maximum length of 254 characters.
    #[error("name too long: {0} characters (max {1})")]
    NameTooLong(usize, usize),

    /// Invalid magic number in table header.
    #[error("invalid magic number: expected 0x{:X}, got 0x{:X}", 0x5A49504D, 0)]
    InvalidMagic,

    /// Table version mismatch.
    #[error("version mismatch: expected {}, got {}", 3, 0)]
    VersionMismatch,

    /// Structure with this name already exists.
    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// Operation timed out.
    #[error("timeout")]
    Timeout,

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type alias for ZeroSHM operations.
pub type Result<T> = std::result::Result<T, ZeroShmError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_error_display() {
        let err = ZeroShmError::OpenError("segment not found".to_string());
        assert_eq!(format!("{}", err), "failed to open shared memory segment: segment not found");
    }

    #[test]
    fn test_mmap_error_display() {
        let err = ZeroShmError::MmapError("permission denied".to_string());
        assert_eq!(format!("{}", err), "failed to mmap shared memory: permission denied");
    }

    #[test]
    fn test_size_error_display() {
        let err = ZeroShmError::SizeError("negative value".to_string());
        assert_eq!(format!("{}", err), "invalid size: negative value");
    }

    #[test]
    fn test_not_found_error_display() {
        let err = ZeroShmError::NotFoundError("my_struct".to_string());
        assert_eq!(format!("{}", err), "not found: my_struct");
    }

    #[test]
    fn test_table_full_display() {
        let err = ZeroShmError::TableFull;
        assert_eq!(format!("{}", err), "table is full");
    }

    #[test]
    fn test_name_too_short_display() {
        let err = ZeroShmError::NameTooShort(2, 3);
        assert_eq!(format!("{}", err), "name too short: 2 characters (min 3)");
    }

    #[test]
    fn test_name_too_long_display() {
        let err = ZeroShmError::NameTooLong(300, 254);
        assert_eq!(format!("{}", err), "name too long: 300 characters (max 254)");
    }

    #[test]
    fn test_invalid_magic_display() {
        let err = ZeroShmError::InvalidMagic;
        assert_eq!(format!("{}", err), "invalid magic number: expected 0x5A49504D, got 0x0");
    }

    #[test]
    fn test_version_mismatch_display() {
        let err = ZeroShmError::VersionMismatch;
        assert_eq!(format!("{}", err), "version mismatch: expected 3, got 0");
    }

    #[test]
    fn test_already_exists_display() {
        let err = ZeroShmError::AlreadyExists("dup_name".to_string());
        assert_eq!(format!("{}", err), "already exists: dup_name");
    }

    #[test]
    fn test_timeout_display() {
        let err = ZeroShmError::Timeout;
        assert_eq!(format!("{}", err), "timeout");
    }

    #[test]
    fn test_io_error_display() {
        let err = ZeroShmError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert_eq!(format!("{}", err), "I/O error: missing");
    }

    #[test]
    fn test_io_error_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: ZeroShmError = io_err.into();
        assert_eq!(format!("{}", err), "I/O error: denied");
    }
}

