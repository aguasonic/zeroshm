//! Generic array in shared memory.
//!
//! Provides the `Array<T>` type for fixed-size arrays of POD (plain old data)
//! types stored in shared memory. The binary layout matches other language
//! implementations: 8-byte capacity header followed by raw element data.
//!
//! # Binary Layout
//! ```text
//! [capacity: u64 (8 bytes)][data: T * capacity]
//! ```

use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use std::marker::PhantomData;
use std::sync::Arc;

/// Marker trait for types that can be stored in shared memory arrays.
///
/// # Safety
///
/// Implementors must be:
/// - `Copy` so values can be read/written by value
/// - `'static` so they have no lifetime dependencies
/// - Binary-representable as a flat byte sequence (no padding traps,
///   no interior mutability, no complex layouts like Rust enums or strings)
///
/// The `SIZE` and `ALIGN` associated constants must match `std::mem::size_of::<Self>()`
/// and `std::mem::align_of::<Self>()` respectively.
pub unsafe trait Pod: Copy + 'static {
    /// Size of the type in bytes.
    const SIZE: usize;
    /// Alignment of the type in bytes.
    const ALIGN: usize;
}

/// Static assertion that all Pod types have alignment <= 8.
/// This is required because the spec guarantees 8-byte section alignment.
const _: () = assert!(size_of::<u64>() == 8);

// Implement Pod for primitive numeric types.
macro_rules! impl_pod {
    ($ty:ty, $size:expr, $align:expr) => {
        // SAFETY: $ty is a primitive type with known size and alignment.
        unsafe impl Pod for $ty {
            const SIZE: usize = $size;
            const ALIGN: usize = $align;
        }
    };
}

impl_pod!(u8, 1, 1);
impl_pod!(i8, 1, 1);
impl_pod!(u16, 2, 2);
impl_pod!(i16, 2, 2);
impl_pod!(u32, 4, 4);
impl_pod!(i32, 4, 4);
impl_pod!(u64, 8, 8);
impl_pod!(i64, 8, 8);
impl_pod!(f32, 4, 4);
impl_pod!(f64, 8, 8);
impl_pod!(bool, 1, 1);

/// Verify at compile time that all Pod implementations have ALIGN <= 8.
mod pod_alignment_check {
    use super::Pod;

    macro_rules! assert_align_le_8 {
        ($ty:ty) => {
            const _: () = assert!(<$ty>::ALIGN <= 8);
        };
    }

    assert_align_le_8!(u8);
    assert_align_le_8!(i8);
    assert_align_le_8!(u16);
    assert_align_le_8!(i16);
    assert_align_le_8!(u32);
    assert_align_le_8!(i32);
    assert_align_le_8!(u64);
    assert_align_le_8!(i64);
    assert_align_le_8!(f32);
    assert_align_le_8!(f64);
    assert_align_le_8!(bool);
}

/// Size of the array header in bytes.
const ARRAY_HEADER_SIZE: usize = 8;

/// A fixed-size array stored in shared memory.
///
/// # Binary Layout
/// ```text
/// [capacity: u64 (8 bytes)][data: T * capacity]
/// ```
///
/// # Type Safety
///
/// Users are responsible for using consistent types when creating and opening
/// arrays. Opening an array with the wrong element type will fail size
/// validation (e.g., opening an f32 array as f64 will be rejected because
/// `8 + capacity * 8 != 8 + capacity * 4`).
pub struct Array<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    offset: usize,
    capacity: usize,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Array<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Array")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("offset", &self.offset)
            .finish()
    }
}

// SAFETY: Array holds an Arc<Memory> which is Send + Sync, and T: Pod
// guarantees the element type is safe for concurrent access via raw pointers
// in process-shared memory.
unsafe impl<T: Pod> Send for Array<T> {}
unsafe impl<T: Pod> Sync for Array<T> {}

impl<T: Pod> Array<T> {
    /// Create a new array in shared memory.
    ///
    /// Allocates space for the array header and data, writes the capacity
    /// header, and zero-initializes the data region.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the array (stored in the table)
    /// * `capacity` -- Number of elements
    ///
    /// # Errors
    /// Returns an error if the name is too long, the array already exists,
    /// or there isn't enough space in the memory segment.
    pub fn new(memory: &Memory, name: &str, capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ZeroShmError::SizeError(
                "capacity must be greater than zero".to_string(),
            ));
        }

        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let total_size = ARRAY_HEADER_SIZE + capacity * T::SIZE;

        // Allocate space and add table entry
        let offset = memory.allocate(name, total_size)? as usize;

        // Write capacity header (little-endian)
        let capacity_bytes = (capacity as u64).to_le_bytes();
        unsafe {
            std::ptr::copy_nonoverlapping(
                capacity_bytes.as_ptr(),
                memory.at_mut(offset),
                ARRAY_HEADER_SIZE,
            );
        }

        // Zero-initialize data region
        let data_start = offset + ARRAY_HEADER_SIZE;
        unsafe {
            std::ptr::write_bytes(memory.at_mut(data_start), 0, capacity * T::SIZE);
        }

        Ok(Array {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            _marker: PhantomData,
        })
    }

    /// Open an existing array in shared memory.
    ///
    /// Finds the array by name in the table, reads the capacity from the
    /// header, and validates that the stored size matches the expected size
    /// for the given element type.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the array to open
    ///
    /// # Errors
    /// Returns an error if the array is not found or if the stored size
    /// doesn't match `8 + capacity * T::SIZE` (wrong element type).
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (offset, size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "array '{}' not found",
                    name
                )))
            }
        };

        let offset = offset as usize;

        // Read capacity from header (little-endian)
        let capacity_bytes: [u8; 8] = unsafe {
            std::ptr::read_unaligned(memory.at(offset) as *const [u8; 8])
        };
        let capacity = u64::from_le_bytes(capacity_bytes) as usize;

        // Validate size
        let expected_size = ARRAY_HEADER_SIZE + capacity * T::SIZE;
        if size as usize != expected_size {
            return Err(ZeroShmError::SizeError(format!(
                "size mismatch for '{}': expected {} ({} elements of {} bytes), got {} (wrong element type?)",
                name, expected_size, capacity, T::SIZE, size
            )));
        }

        Ok(Array {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            offset,
            capacity,
            _marker: PhantomData,
        })
    }

    /// Get the element at the given index.
    ///
    /// # Panics
    /// Panics if the index is out of bounds.
    pub fn get(&self, index: usize) -> T {
        if index >= self.capacity {
            panic!(
                "index {} out of bounds (capacity={})",
                index, self.capacity
            );
        }

        let elem_offset = self.offset + ARRAY_HEADER_SIZE + index * T::SIZE;

        // SAFETY: T: Pod guarantees the type is safe to read from raw memory.
        // The offset is bounds-checked and aligned (8-byte section alignment).
        unsafe { *self.memory.at(elem_offset).cast::<T>() }
    }

    /// Set the element at the given index.
    ///
    /// # Panics
    /// Panics if the index is out of bounds.
    pub fn set(&self, index: usize, value: T) {
        if index >= self.capacity {
            panic!(
                "index {} out of bounds (capacity={})",
                index, self.capacity
            );
        }

        let elem_offset = self.offset + ARRAY_HEADER_SIZE + index * T::SIZE;

        // SAFETY: T: Pod guarantees the type is safe to write to raw memory.
        // The offset is bounds-checked and aligned (8-byte section alignment).
        unsafe { *self.memory.at_mut(elem_offset).cast::<T>() = value }
    }

    /// Returns the array capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the array name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset of the array in the memory segment.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;

    #[test]
    fn test_array_create_set_get() {
        let name = "/test_step7_array";
        let size = 64 * 1024; // 64KB
        let capacity = 1000;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create array of 1000 f32
        let array: Array<f32> = Array::new(&memory, "test_array", capacity)
            .expect("create array should succeed");

        assert_eq!(array.capacity(), capacity);
        assert_eq!(array.name(), "test_array");

        // Set index 0 to 23.5, index 999 to 42.0
        array.set(0, 23.5);
        array.set(999, 42.0);

        // Verify values
        assert_eq!(array.get(0), 23.5, "index 0 should be 23.5");
        assert_eq!(array.get(999), 42.0, "index 999 should be 42.0");

        // Verify default values (zero-initialized)
        assert_eq!(array.get(500), 0.0, "middle element should be 0.0");

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_array_memory_layout() {
        let name = "/test_step7_layout";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create array of 4 f32
        let array: Array<f32> =
            Array::new(&memory, "layout_array", 4).expect("create array should succeed");

        array.set(0, 1.0);
        array.set(1, 2.0);
        array.set(2, 3.0);
        array.set(3, 4.0);

        // Dump memory region and verify bytes match expected little-endian f32 encoding
        let offset = array.offset();
        let data = memory.data();

        // Header: capacity = 4 (little-endian u64)
        assert_eq!(&data[offset..offset + 8], &[4, 0, 0, 0, 0, 0, 0, 0]);

        // Data: 4 f32 values in little-endian
        let one: u32 = 1.0f32.to_bits();
        let two: u32 = 2.0f32.to_bits();
        let three: u32 = 3.0f32.to_bits();
        let four: u32 = 4.0f32.to_bits();

        let data_start = offset + 8;
        assert_eq!(
            &data[data_start..data_start + 4],
            &one.to_le_bytes(),
            "element 0 bytes should match f32(1.0)"
        );
        assert_eq!(
            &data[data_start + 4..data_start + 8],
            &two.to_le_bytes(),
            "element 1 bytes should match f32(2.0)"
        );
        assert_eq!(
            &data[data_start + 8..data_start + 12],
            &three.to_le_bytes(),
            "element 2 bytes should match f32(3.0)"
        );
        assert_eq!(
            &data[data_start + 12..data_start + 16],
            &four.to_le_bytes(),
            "element 3 bytes should match f32(4.0)"
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_array_bounds_check() {
        let name = "/test_step7_bounds";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        let array: Array<i32> =
            Array::new(&memory, "bounds_array", 10).expect("create array should succeed");

        // Valid access
        array.set(0, 42);
        assert_eq!(array.get(0), 42);
        array.set(9, 99);
        assert_eq!(array.get(9), 99);

        // Out of bounds should panic
        let result = std::panic::catch_unwind(|| {
            array.get(10);
        });
        assert!(result.is_err(), "get(10) on capacity 10 should panic");

        let result = std::panic::catch_unwind(|| {
            array.set(10, 0);
        });
        assert!(result.is_err(), "set(10) on capacity 10 should panic");

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_array_type_mismatch() {
        let name = "/test_step7_mismatch";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create array of 10 f32 (total size = 8 + 10*4 = 48)
        let _array: Array<f32> =
            Array::new(&memory, "mismatch_array", 10).expect("create array should succeed");

        // Try to open as f64 (expected size = 8 + 10*8 = 88, stored = 48)
        let result: Result<Array<f64>> = Array::open(&memory, "mismatch_array");
        assert!(
            matches!(result, Err(ZeroShmError::SizeError(_))),
            "opening f32 array as f64 should fail with SizeError, got {:?}",
            result
        );

        // Opening with correct type should succeed
        let array: Array<f32> = Array::open(&memory, "mismatch_array").expect("should open");
        assert_eq!(array.capacity(), 10);

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_array_reopen_persistence() {
        let name = "/test_step7_reopen";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create array and set values
        let array: Array<u64> =
            Array::new(&memory, "persist_array", 5).expect("create array should succeed");
        array.set(0, 100);
        array.set(1, 200);
        array.set(2, 300);
        array.set(3, 400);
        array.set(4, 500);

        drop(array);

        // Open the same array (simulating another process or re-open)
        let array2: Array<u64> =
            Array::open(&memory, "persist_array").expect("reopen should succeed");
        assert_eq!(array2.capacity(), 5);
        assert_eq!(array2.get(0), 100);
        assert_eq!(array2.get(1), 200);
        assert_eq!(array2.get(2), 300);
        assert_eq!(array2.get(3), 400);
        assert_eq!(array2.get(4), 500);

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_array_different_types() {
        let name = "/test_step7_types";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create arrays of different types
        let u8_arr: Array<u8> = Array::new(&memory, "u8_arr", 100).expect("create u8 array");
        let i32_arr: Array<i32> = Array::new(&memory, "i32_arr", 50).expect("create i32 array");
        let f64_arr: Array<f64> = Array::new(&memory, "f64_arr", 25).expect("create f64 array");

        // Set and verify values
        u8_arr.set(0, 255);
        assert_eq!(u8_arr.get(0), 255);

        i32_arr.set(0, -12345);
        assert_eq!(i32_arr.get(0), -12345);

        f64_arr.set(0, 3.14159);
        assert_eq!(f64_arr.get(0), 3.14159);

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }

    #[test]
    fn test_array_duplicate_name() {
        let name = "/test_step7_dup";
        let size = 64 * 1024;

        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, size, 64).expect("create should succeed");

        // Create first array
        let _arr1: Array<f32> =
            Array::new(&memory, "dup_array", 10).expect("first create should succeed");

        // Try to create another with same name
        let result: Result<Array<f32>> = Array::new(&memory, "dup_array", 10);
        assert!(
            matches!(result, Err(ZeroShmError::AlreadyExists(_))),
            "duplicate name should fail with AlreadyExists, got {:?}",
            result
        );

        // Clean up
        memory.unlink().expect("unlink should succeed");
        drop(memory);
    }
}

