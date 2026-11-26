//! Table implementation matching the ZeroSHM v3.0 binary specification.
//!
//! The table is stored at the beginning of shared memory and tracks all
//! allocated structures by name, offset, and size. Lookup uses hash-first
//! comparison with full-string fallback for collision resolution.
//!
//! Binary layout (v3.0):
//! - Header: 32 bytes (magic, version, entry_count, max_entries, memory_size, next_offset)
//! - Entry: 384 bytes each (hash, offset, size, name[256] + padding)
//! - All integers are little-endian.

use crate::error::{Result, ZeroShmError};
use crate::hash::hash_name;
use crate::layout::{
    align_up, ENTRY_SIZE, HEADER_SIZE, MAX_NAME_SIZE, MIN_NAME_SIZE, TABLE_MAGIC, TABLE_VERSION,
    NAME_FIELD_SIZE,
    TABLE_MAGIC_OFFSET, TABLE_VERSION_OFFSET, TABLE_ENTRY_COUNT_OFFSET,
    TABLE_MAX_ENTRIES_OFFSET, TABLE_MEMORY_SIZE_OFFSET, TABLE_NEXT_OFFSET_OFFSET,
};

/// Entry field offsets within the 384-byte entry slot.
/// These constants define the binary layout shared across C++, Go, and Rust.
pub const ENTRY_HASH_OFFSET: usize = 0;
pub const ENTRY_OFFSET_OFFSET: usize = 8;
pub const ENTRY_SIZE_OFFSET: usize = 16;
pub const ENTRY_NAME_OFFSET: usize = 24;
pub const ENTRY_NAME_END: usize = 280; // ENTRY_NAME_OFFSET + 256

/// Table header stored at the beginning of shared memory (32 bytes).
///
/// Binary layout (little-endian):
/// - magic: u32 at offset 0
/// - version: u32 at offset 4
/// - entry_count: u32 at offset 8
/// - max_entries: u32 at offset 12
/// - memory_size: u64 at offset 16
/// - next_offset: u64 at offset 24
#[derive(Debug, Clone, Copy)]
pub struct TableHeader {
    pub magic: u32,
    pub version: u32,
    pub entry_count: u32,
    pub max_entries: u32,
    pub memory_size: u64,
    pub next_offset: u64,
}

impl TableHeader {
    /// Deserialize a header from 32 bytes of little-endian data.
    pub fn from_bytes(data: &[u8]) -> Self {
        assert!(data.len() >= HEADER_SIZE);
        TableHeader {
            magic: u32::from_le_bytes(data[TABLE_MAGIC_OFFSET..TABLE_MAGIC_OFFSET+4].try_into().unwrap()),
            version: u32::from_le_bytes(data[TABLE_VERSION_OFFSET..TABLE_VERSION_OFFSET+4].try_into().unwrap()),
            entry_count: u32::from_le_bytes(data[TABLE_ENTRY_COUNT_OFFSET..TABLE_ENTRY_COUNT_OFFSET+4].try_into().unwrap()),
            max_entries: u32::from_le_bytes(data[TABLE_MAX_ENTRIES_OFFSET..TABLE_MAX_ENTRIES_OFFSET+4].try_into().unwrap()),
            memory_size: u64::from_le_bytes(data[TABLE_MEMORY_SIZE_OFFSET..TABLE_MEMORY_SIZE_OFFSET+8].try_into().unwrap()),
            next_offset: u64::from_le_bytes(data[TABLE_NEXT_OFFSET_OFFSET..TABLE_NEXT_OFFSET_OFFSET+8].try_into().unwrap()),
        }
    }

    /// Serialize the header into 32 bytes of little-endian data.
    pub fn to_bytes(&self, data: &mut [u8]) {
        assert!(data.len() >= HEADER_SIZE);
        data[TABLE_MAGIC_OFFSET..TABLE_MAGIC_OFFSET+4].copy_from_slice(&self.magic.to_le_bytes());
        data[TABLE_VERSION_OFFSET..TABLE_VERSION_OFFSET+4].copy_from_slice(&self.version.to_le_bytes());
        data[TABLE_ENTRY_COUNT_OFFSET..TABLE_ENTRY_COUNT_OFFSET+4].copy_from_slice(&self.entry_count.to_le_bytes());
        data[TABLE_MAX_ENTRIES_OFFSET..TABLE_MAX_ENTRIES_OFFSET+4].copy_from_slice(&self.max_entries.to_le_bytes());
        data[TABLE_MEMORY_SIZE_OFFSET..TABLE_MEMORY_SIZE_OFFSET+8].copy_from_slice(&self.memory_size.to_le_bytes());
        data[TABLE_NEXT_OFFSET_OFFSET..TABLE_NEXT_OFFSET_OFFSET+8].copy_from_slice(&self.next_offset.to_le_bytes());
    }
}

/// Table entry describing a named structure (384 bytes each).
///
/// Binary layout (little-endian):
/// - hash: u64 at offset 0 (xxhash3-64 of name with seed 0x2FE7)
/// - offset: u64 at offset 8
/// - size: u64 at offset 16
/// - name: [u8; 256] at offset 24 (null-terminated, max 255 bytes)
/// - padding: 104 bytes at offset 280 (zeroed)
#[derive(Debug, Clone, Copy)]
pub struct TableEntry {
    pub hash: u64,
    pub offset: u64,
    pub size: u64,
    pub name: [u8; NAME_FIELD_SIZE],
}

impl TableEntry {
    /// Deserialize an entry from 384 bytes of little-endian data.
    pub fn from_bytes(data: &[u8]) -> Self {
        assert!(data.len() >= ENTRY_SIZE);
        let mut entry = TableEntry {
            hash: u64::from_le_bytes(data[ENTRY_HASH_OFFSET..ENTRY_HASH_OFFSET+8].try_into().unwrap()),
            offset: u64::from_le_bytes(data[ENTRY_OFFSET_OFFSET..ENTRY_OFFSET_OFFSET+8].try_into().unwrap()),
            size: u64::from_le_bytes(data[ENTRY_SIZE_OFFSET..ENTRY_SIZE_OFFSET+8].try_into().unwrap()),
            name: [0u8; NAME_FIELD_SIZE],
        };
        entry.name.copy_from_slice(&data[ENTRY_NAME_OFFSET..ENTRY_NAME_END]);
        entry
    }

    /// Serialize the entry into 384 bytes of little-endian data.
    /// The padding region (bytes ENTRY_NAME_END..ENTRY_SIZE) is zeroed.
    pub fn to_bytes(&self, data: &mut [u8]) {
        assert!(data.len() >= ENTRY_SIZE);
        data[ENTRY_HASH_OFFSET..ENTRY_HASH_OFFSET+8].copy_from_slice(&self.hash.to_le_bytes());
        data[ENTRY_OFFSET_OFFSET..ENTRY_OFFSET_OFFSET+8].copy_from_slice(&self.offset.to_le_bytes());
        data[ENTRY_SIZE_OFFSET..ENTRY_SIZE_OFFSET+8].copy_from_slice(&self.size.to_le_bytes());
        data[ENTRY_NAME_OFFSET..ENTRY_NAME_END].copy_from_slice(&self.name);
        // Zero the padding region
        data[ENTRY_NAME_END..ENTRY_SIZE].fill(0);
    }

    /// Returns the name as a Rust string (strips null terminator and trailing bytes).
    pub fn name_string(&self) -> &str {
        // Find the null terminator
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(256);
        // SAFETY: The name field contains valid UTF-8 (ASCII) bytes up to the null terminator.
        // Names are user-provided strings that were validated on insertion.
        unsafe { std::str::from_utf8_unchecked(&self.name[..len]) }
    }
}

/// Manages named structures in shared memory.
///
/// The table is stored at the beginning of the memory region, with the
/// header at offset 0 and entries following at offset 32.
pub struct Table {
    /// Pointer to the start of the memory region containing the table.
    data: *mut u8,
    /// Maximum number of entries this table can hold.
    max_entries: usize,
    /// Total size of the shared memory segment.
    memory_size: usize,
}

// SAFETY: Table holds a raw pointer to mmap'd memory with MAP_SHARED,
// which is process-shared and can be safely accessed from multiple threads
// and processes. All pointer accesses are bounds-checked.
unsafe impl Send for Table {}
unsafe impl Sync for Table {}

impl Table {
    /// Create a shallow clone of this table.
    ///
    /// The clone shares the same memory pointer. This is used when
    /// `Memory::clone_shallow` creates a second reference to the same
    /// segment (e.g., for use by `Array`, `Queue`, etc. via `Arc<Memory>`).
    pub fn clone_shallow(&self) -> Self {
        Table {
            data: self.data,
            max_entries: self.max_entries,
            memory_size: self.memory_size,
        }
    }

    /// Create a new table or open an existing one in shared memory.
    ///
    /// # Arguments
    /// * `data` -- Pointer to the start of the memory region
    /// * `max_entries` -- Maximum number of table entries
    /// * `memory_size` -- Total size of the shared memory segment
    /// * `create` -- If true, initialize a new table; if false, validate existing
    pub fn new(data: *mut u8, max_entries: usize, memory_size: usize, create: bool) -> Result<Self> {
        if max_entries == 0 {
            return Err(ZeroShmError::SizeError(
                "max_entries must be greater than zero".to_string(),
            ));
        }

        // Verify there's enough space for the table (only when creating;
        // when opening, the actual size is read from the header in validate())
        if create {
            let table_size = Self::calculate_size(max_entries);
            if table_size > memory_size {
                return Err(ZeroShmError::SizeError(format!(
                    "table size {} exceeds memory size {}",
                    table_size, memory_size
                )));
            }
        }

        let mut table = Table {
            data,
            max_entries,
            memory_size,
        };

        if create {
            table.initialize()?;
        } else {
            table.validate()?;
        }

        Ok(table)
    }

    /// Calculate the total size of a table with the given max entries.
    pub fn calculate_size(max_entries: usize) -> usize {
        HEADER_SIZE + max_entries * ENTRY_SIZE
    }

    /// Initialize a new table: write header, zero entries.
    fn initialize(&self) -> Result<()> {
        // SAFETY: data points to a valid mmap'd region of at least memory_size bytes.
        unsafe {
            let header = TableHeader {
                magic: TABLE_MAGIC,
                version: TABLE_VERSION,
                entry_count: 0,
                max_entries: self.max_entries as u32,
                memory_size: self.memory_size as u64,
                next_offset: Self::calculate_size(self.max_entries) as u64,
            };

            // Write header
            let mut header_bytes = [0u8; HEADER_SIZE];
            header.to_bytes(&mut header_bytes);
            std::ptr::copy_nonoverlapping(
                header_bytes.as_ptr(),
                self.data,
                HEADER_SIZE,
            );

            // Zero out entries
            let entries_start = self.data.add(HEADER_SIZE);
            std::ptr::write_bytes(entries_start, 0, self.max_entries * ENTRY_SIZE);
        }

        Ok(())
    }

    /// Validate an existing table: check magic, version, entry count.
    fn validate(&mut self) -> Result<()> {
        let header = self.header()?;

        if header.magic != TABLE_MAGIC {
            return Err(ZeroShmError::InvalidMagic);
        }

        if header.version != TABLE_VERSION {
            return Err(ZeroShmError::VersionMismatch);
        }

        if header.entry_count > self.max_entries as u32 {
            return Err(ZeroShmError::SizeError(format!(
                "table corruption: entry count {} exceeds maximum {}",
                header.entry_count, self.max_entries
            )));
        }

        // Use the stored memory size when opening existing table
        self.memory_size = header.memory_size as usize;

        Ok(())
    }

    /// Read the table header from memory.
    pub fn header(&self) -> Result<TableHeader> {
        // SAFETY: data points to a valid mmap'd region of at least HEADER_SIZE bytes.
        let header = unsafe {
            let mut bytes = [0u8; HEADER_SIZE];
            std::ptr::copy_nonoverlapping(self.data, bytes.as_mut_ptr(), HEADER_SIZE);
            TableHeader::from_bytes(&bytes)
        };
        Ok(header)
    }

    /// Write the table header to memory.
    fn write_header(&self, header: &TableHeader) {
        // SAFETY: data points to a valid mmap'd region of at least HEADER_SIZE bytes.
        unsafe {
            let mut bytes = [0u8; HEADER_SIZE];
            header.to_bytes(&mut bytes);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.data, HEADER_SIZE);
        }
    }

    /// Read a table entry from memory at the given index.
    fn entry_at(&self, index: usize) -> Result<TableEntry> {
        if index >= self.max_entries {
            return Err(ZeroShmError::NotFoundError(format!(
                "entry index {} out of bounds",
                index
            )));
        }

        // SAFETY: data points to a valid mmap'd region; index is bounds-checked.
        let entry = unsafe {
            let offset = HEADER_SIZE + index * ENTRY_SIZE;
            let entry_ptr = self.data.add(offset);
            let mut bytes = [0u8; ENTRY_SIZE];
            std::ptr::copy_nonoverlapping(entry_ptr, bytes.as_mut_ptr(), ENTRY_SIZE);
            TableEntry::from_bytes(&bytes)
        };

        Ok(entry)
    }

    /// Write a table entry to memory at the given index.
    fn write_entry(&self, index: usize, entry: &TableEntry) -> Result<()> {
        if index >= self.max_entries {
            return Err(ZeroShmError::NotFoundError(format!(
                "entry index {} out of bounds",
                index
            )));
        }

        // SAFETY: data points to a valid mmap'd region; index is bounds-checked.
        unsafe {
            let mut bytes = [0u8; ENTRY_SIZE];
            entry.to_bytes(&mut bytes);
            let offset = HEADER_SIZE + index * ENTRY_SIZE;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.data.add(offset), ENTRY_SIZE);
        }

        Ok(())
    }

    /// Find an entry by name using hash-first comparison with string fallback.
    ///
    /// Returns `Some(entry)` if found, `None` otherwise.
    pub fn find(&self, name: &str) -> Result<Option<TableEntry>> {
        if name.len() < MIN_NAME_SIZE {
            return Err(ZeroShmError::NameTooShort(name.len(), MIN_NAME_SIZE));
        }
        if name.len() > MAX_NAME_SIZE {
            return Err(ZeroShmError::NameTooLong(name.len(), MAX_NAME_SIZE));
        }

        let header = self.header()?;
        let hash = hash_name(name);

        for i in 0..header.entry_count as usize {
            let entry = self.entry_at(i)?;
            // Hash-first comparison for fast rejection, then full string comparison
            if entry.hash == hash && entry.name_string() == name {
                return Ok(Some(entry));
            }
        }

        Ok(None)
    }

    /// Add a new entry to the table.
    ///
    /// # Errors
    /// - `NameTooLong` if the name exceeds 254 characters
    /// - `TableFull` if the table has reached max_entries
    /// - `AlreadyExists` if an entry with this name already exists
    pub fn add(&self, name: &str, offset: u64, size: u64) -> Result<()> {
        if name.len() < MIN_NAME_SIZE {
            return Err(ZeroShmError::NameTooShort(name.len(), MIN_NAME_SIZE));
        }
        if name.len() > MAX_NAME_SIZE {
            return Err(ZeroShmError::NameTooLong(name.len(), MAX_NAME_SIZE));
        }

        // Check for duplicates
        if self.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        let header = self.header()?;

        if header.entry_count >= self.max_entries as u32 {
            return Err(ZeroShmError::TableFull);
        }

        // Compute hash
        let hash = hash_name(name);

        // Build entry
        let mut entry = TableEntry {
            hash,
            name: [0u8; 256],
            offset,
            size,
        };
        entry.name[..name.len()].copy_from_slice(name.as_bytes());
        // Null terminator is already 0 from initialization

        // Write entry at the next slot
        let index = header.entry_count as usize;
        self.write_entry(index, &entry)?;

        // Update entry count
        let mut new_header = header;
        new_header.entry_count += 1;
        self.write_header(&new_header);

        Ok(())
    }

    /// Allocate space for a new structure.
    ///
    /// Finds the next aligned offset that fits the requested size within
    /// the memory segment, and updates `next_offset` in the header.
    ///
    /// # Arguments
    /// * `size` -- Size in bytes to allocate
    /// * `align` -- Alignment requirement (must be a power of two)
    ///
    /// # Returns
    /// The offset of the allocated space.
    ///
    /// # Errors
    /// - `SizeError` if the allocation would exceed memory bounds
    pub fn allocate(&self, size: usize, align: usize) -> Result<u64> {
        let header = self.header()?;

        // Align the offset
        let aligned = align_up(header.next_offset as usize, align);

        // Check bounds
        if aligned.checked_add(size).map_or(true, |end| end > self.memory_size) {
            return Err(ZeroShmError::SizeError(format!(
                "allocation of {} bytes at offset {} would exceed memory size {}",
                size, aligned, self.memory_size
            )));
        }

        // Update next_offset
        let mut new_header = header;
        new_header.next_offset = (aligned + size) as u64;
        self.write_header(&new_header);

        Ok(aligned as u64)
    }

    /// Iterate over all active entries in the table.
    pub fn entries(&self) -> TableEntries<'_> {
        let header = self.header().unwrap_or(TableHeader {
            magic: 0,
            version: 0,
            entry_count: 0,
            max_entries: 0,
            memory_size: 0,
            next_offset: 0,
        });
        TableEntries {
            table: self,
            index: 0,
            count: header.entry_count as usize,
        }
    }

    /// Returns the number of entries currently in the table.
    pub fn entry_count(&self) -> Result<usize> {
        Ok(self.header()?.entry_count as usize)
    }

    /// Returns the maximum number of entries this table can hold.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Returns the next allocation offset.
    pub fn next_offset(&self) -> Result<u64> {
        Ok(self.header()?.next_offset)
    }

    /// Returns the total size of the shared memory segment.
    pub fn memory_size(&self) -> usize {
        self.memory_size
    }
}

/// Iterator over active table entries.
pub struct TableEntries<'a> {
    table: &'a Table,
    index: usize,
    count: usize,
}

impl<'a> Iterator for TableEntries<'a> {
    type Item = Result<TableEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let result = self.table.entry_at(self.index);
        self.index += 1;
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Memory;

    #[test]
    fn test_table_create_and_allocate() {
        let name = "/test_step6_table";
        let size = 64 * 1024; // 64KB
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");

        // Create table
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Verify header
        let header = table.header().expect("header should be readable");
        assert_eq!(header.magic, TABLE_MAGIC);
        assert_eq!(header.version, TABLE_VERSION);
        assert_eq!(header.entry_count, 0);
        assert_eq!(header.max_entries, max_entries as u32);
        assert_eq!(header.memory_size, size as u64);
        assert_eq!(header.next_offset, Table::calculate_size(max_entries) as u64);

        // Allocate space
        let offset = table.allocate(1000, 8).expect("allocate should succeed");
        assert_eq!(offset, Table::calculate_size(max_entries) as u64);

        // Verify next_offset was updated
        let header2 = table.header().expect("header should be readable");
        assert_eq!(header2.next_offset, offset + 1000);

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_add_find() {
        let name = "/test_step6_add_find";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Add entries
        table
            .add("sensor_data", 1000, 500)
            .expect("add sensor_data should succeed");
        table
            .add("event_queue", 2000, 1000)
            .expect("add event_queue should succeed");
        table
            .add("temperature", 3000, 200)
            .expect("add temperature should succeed");

        // Verify entry count
        assert_eq!(table.entry_count().expect("count"), 3);

        // Find entries
        let entry = table.find("sensor_data").expect("find").expect("should exist");
        assert_eq!(entry.name_string(), "sensor_data");
        assert_eq!(entry.offset, 1000);
        assert_eq!(entry.size, 500);

        let entry = table.find("event_queue").expect("find").expect("should exist");
        assert_eq!(entry.name_string(), "event_queue");
        assert_eq!(entry.offset, 2000);
        assert_eq!(entry.size, 1000);

        // Find non-existent
        assert!(table.find("nonexistent").expect("find").is_none());

        // Verify hash-first rejection: a name with different hash should not match
        // even if it happens to have the same prefix
        assert!(table.find("sensor").expect("find").is_none());

        // Adding duplicate should fail
        let result = table.add("sensor_data", 4000, 100);
        assert!(
            matches!(result, Err(ZeroShmError::AlreadyExists(_))),
            "duplicate add should fail"
        );

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_full() {
        let name = "/test_step6_full";
        let size = 64 * 1024;
        let max_entries = 2;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Fill the table
        table.add("aaa", 100, 10).expect("add aaa");
        table.add("bbb", 200, 10).expect("add bbb");

        // Table should be full
        let result = table.add("ccc", 300, 10);
        assert!(
            matches!(result, Err(ZeroShmError::TableFull)),
            "table full should be detected"
        );

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_reopen_persistence() {
        let name = "/test_step6_persist";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Add entries and allocate
        table
            .add("persistent_data", 1000, 500)
            .expect("add should succeed");
        let offset = table.allocate(2000, 8).expect("allocate should succeed");

        // Drop the table and memory
        drop(table);
        drop(mem);

        // Reopen the segment
        let mem2 = Memory::open(name, max_entries).expect("reopen should succeed");
        let table2 = Table::new(mem2.data_mut().as_mut_ptr(), max_entries, 0, false)
            .expect("table open should succeed");

        // Verify entries persist
        let entry = table2
            .find("persistent_data")
            .expect("find")
            .expect("entry should persist");
        assert_eq!(entry.name_string(), "persistent_data");
        assert_eq!(entry.offset, 1000);
        assert_eq!(entry.size, 500);

        // Verify next_offset persists
        assert_eq!(table2.next_offset().expect("next_offset"), offset + 2000);

        // Clean up
        mem2.unlink().expect("unlink should succeed");
        drop(mem2);
    }

    #[test]
    fn test_table_entries_iterator() {
        let name = "/test_step6_iter";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        table.add("first", 100, 10).expect("add first");
        table.add("second", 200, 20).expect("add second");
        table.add("third", 300, 30).expect("add third");

        // Iterate
        let names: Vec<String> = table
            .entries()
            .map(|e| e.expect("entry").name_string().to_string())
            .collect();
        assert_eq!(names, vec!["first", "second", "third"]);

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_validate_invalid_magic() {
        let name = "/test_step6_badmagic";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");

        // Corrupt the magic number
        let data = mem.data_mut();
        data[0] = 0xFF; // Corrupt magic

        // Opening should fail with InvalidMagic
        let result = Table::new(data.as_mut_ptr(), max_entries, 0, false);
        assert!(
            matches!(result, Err(ZeroShmError::InvalidMagic)),
            "invalid magic should be detected"
        );

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_name_too_short() {
        let name = "/test_step6_shortname";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Name shorter than 3 characters should fail
        let short_name = "a";
        let result = table.add(short_name, 100, 10);
        assert!(
            matches!(result, Err(ZeroShmError::NameTooShort(_, _))),
            "short name should be rejected"
        );

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_name_too_long() {
        let name = "/test_step6_longname";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Name longer than 255 bytes should fail
        let long_name = "a".repeat(256);
        let result = table.add(&long_name, 100, 10);
        assert!(
            matches!(result, Err(ZeroShmError::NameTooLong(_, _))),
            "long name should be rejected"
        );

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }

    #[test]
    fn test_table_allocate_bounds() {
        let name = "/test_step6_bounds";
        let size = 64 * 1024;
        let max_entries = 16;

        let _ = crate::platform::unlink_shm(name);
        let mem = Memory::create(name, size, max_entries).expect("create should succeed");
        let table = Table::new(mem.data_mut().as_mut_ptr(), max_entries, size, true)
            .expect("table create should succeed");

        // Allocate more than available space should fail
        let result = table.allocate(size, 8);
        assert!(
            matches!(result, Err(ZeroShmError::SizeError(_))),
            "allocation exceeding bounds should fail"
        );

        // Clean up
        mem.unlink().expect("unlink should succeed");
        drop(mem);
    }
}

