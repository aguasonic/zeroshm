package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"unsafe"

	"github.com/zeebo/xxh3"
)

// Constants are defined in constants_generated.go (auto-generated from constants.pkl)

const (
	// MinNameSize is the minimum length of entry names
	MinNameSize = 3

	// MaxNameSize is the maximum length of entry names (255 bytes including null terminator)
	MaxNameSize = 254

	// NameSize is the maximum length of entry names (including null terminator)
	// Deprecated: use MaxNameSize instead
	NameSize = 255
)

// Header is the table header stored at the beginning of shared memory.
// Binary layout (32 bytes):
//   - magic: uint32 (offset 0)
//   - version: uint32 (offset 4)
//   - entry_count: uint32 (offset 8)
//   - reserved: uint32 (offset 12)
//   - memory_size: uint64 (offset 16)
//   - next_offset: uint64 (offset 24)
type Header struct {
	Magic      uint32
	Version    uint32
	EntryCount uint32
	MaxEntries uint32
	MemorySize uint64
	NextOffset uint64
}

// Entry field offsets within the 384-byte entry slot.
// These constants define the binary layout shared across C++, Go, and Rust.
const (
	EntryHashOffset   = 0
	EntryOffsetOffset = 8
	EntrySizeOffset   = 16
	EntryNameOffset   = 24
	EntryNameEnd      = EntryNameOffset + NameFieldSize
)

// Entry is a table entry describing a named structure.
// Binary layout (280 bytes, matches C/C++ v3):
//   - hash: uint64 (offset 0) -- xxhash3-64 of name with seed 0x2FE7
//   - offset: uint64 (offset 8) -- Supports offsets >4GB
//   - size: uint64 (offset 16) -- Supports sizes >4GB
//   - name: [256]byte (offset 24) -- null-terminated name (max 254 chars + 1 null + 1 padding)
type Entry struct {
	Hash   uint64
	Offset uint64
	Size   uint64
	Name   [NameFieldSize]byte
}

// NameString returns the name as a Go string (null-terminated).
func (e *Entry) NameString() string {
	for i, b := range e.Name {
		if b == 0 {
			return string(e.Name[:i])
		}
	}
	return string(e.Name[:])
}

// Table manages named structures in shared memory.
type Table struct {
	data       []byte
	maxEntries int
	memorySize int
}

// NewTable creates or opens a table in shared memory.
// If create is true, initializes a new table; otherwise validates existing.
func NewTable(data []byte, maxEntries int, memorySize int, create bool) *Table {
	t := &Table{
		data:       data,
		maxEntries: maxEntries,
		memorySize: memorySize,
	}

	if create {
		t.initialize()
	} else {
		t.validate()
	}

	return t
}

func (t *Table) initialize() {
	h := t.Header()
	h.Magic = TableMagic
	h.Version = TableVersion
	h.EntryCount = 0
	h.MaxEntries = uint32(t.maxEntries)
	h.MemorySize = uint64(t.memorySize)
	h.NextOffset = uint64(CalculateTableSize(t.maxEntries))

	t.writeHeader(h)

	// Zero out entries
	entriesStart := HeaderSize
	entriesEnd := entriesStart + t.maxEntries*EntrySize
	for i := entriesStart; i < entriesEnd; i++ {
		t.data[i] = 0
	}
}

func (t *Table) validate() {
	h := t.Header()

	if h.Magic != TableMagic {
		panic(fmt.Sprintf("invalid table magic: 0x%08X (expected 0x%08X)", h.Magic, TableMagic))
	}

	if h.Version != TableVersion {
		panic(fmt.Sprintf("incompatible table version: %d (expected %d)", h.Version, TableVersion))
	}

	if int(h.EntryCount) > t.maxEntries {
		panic(fmt.Sprintf("table corruption: entry count %d exceeds maximum %d", h.EntryCount, t.maxEntries))
	}

	// Use stored memory size
	t.memorySize = int(h.MemorySize)
}

// Header returns a copy of the table header.
func (t *Table) Header() *Header {
	return &Header{
		Magic:      binary.LittleEndian.Uint32(t.data[TableMagicOffset : TableMagicOffset+U32Size]),
		Version:    binary.LittleEndian.Uint32(t.data[TableVersionOffset : TableVersionOffset+U32Size]),
		EntryCount: binary.LittleEndian.Uint32(t.data[TableEntryCountOffset : TableEntryCountOffset+U32Size]),
		MaxEntries: binary.LittleEndian.Uint32(t.data[TableMaxEntriesOffset : TableMaxEntriesOffset+U32Size]),
		MemorySize: binary.LittleEndian.Uint64(t.data[TableMemorySizeOffset : TableMemorySizeOffset+U64Size]),
		NextOffset: binary.LittleEndian.Uint64(t.data[TableNextOffsetOffset : TableNextOffsetOffset+U64Size]),
	}
}

func (t *Table) writeHeader(h *Header) {
	binary.LittleEndian.PutUint32(t.data[TableMagicOffset:TableMagicOffset+U32Size], h.Magic)
	binary.LittleEndian.PutUint32(t.data[TableVersionOffset:TableVersionOffset+U32Size], h.Version)
	binary.LittleEndian.PutUint32(t.data[TableEntryCountOffset:TableEntryCountOffset+U32Size], h.EntryCount)
	binary.LittleEndian.PutUint32(t.data[TableMaxEntriesOffset:TableMaxEntriesOffset+U32Size], h.MaxEntries)
	binary.LittleEndian.PutUint64(t.data[TableMemorySizeOffset:TableMemorySizeOffset+U64Size], h.MemorySize)
	binary.LittleEndian.PutUint64(t.data[TableNextOffsetOffset:TableNextOffsetOffset+U64Size], h.NextOffset)
}

// Entry returns a pointer to the entry at the given index.
func (t *Table) Entry(index int) *Entry {
	if index < 0 || index >= t.maxEntries {
		panic(fmt.Sprintf("entry index %d out of bounds", index))
	}
	offset := HeaderSize + index*EntrySize
	return (*Entry)(unsafe.Pointer(&t.data[offset]))
}

// Find looks up an entry by name (hash-first comparison with string fallback).
func (t *Table) Find(name string) *Entry {
	if err := validateName(name); err != nil {
		return nil
	}
	h := t.Header()
	hash := xxh3.HashStringSeed(name, XXHashSeed)
	for i := 0; i < int(h.EntryCount); i++ {
		e := t.Entry(i)
		if e.Hash == hash && e.NameString() == name {
			return e
		}
	}
	return nil
}

// validateName checks that a name is within the allowed length range.
func validateName(name string) error {
	if len(name) < MinNameSize {
		return fmt.Errorf("name too short: %d characters (min %d)", len(name), MinNameSize)
	}
	if len(name) > MaxNameSize {
		return fmt.Errorf("name too long: %d characters (max %d)", len(name), MaxNameSize)
	}
	return nil
}

// Add adds a new entry to the table.
func (t *Table) Add(name string, offset uint64, size uint64) error {
	if err := validateName(name); err != nil {
		return err
	}

	h := t.Header()
	if int(h.EntryCount) >= t.maxEntries {
		return errors.New("table full")
	}

	if t.Find(name) != nil {
		return errors.New("name already exists")
	}

	// Get the entry slot
	e := t.Entry(int(h.EntryCount))

	// Compute hash and store
	e.Hash = xxh3.HashStringSeed(name, XXHashSeed)

	// Clear and set name
	for i := range e.Name {
		e.Name[i] = 0
	}
	copy(e.Name[:], name)

	// Set offset and size using struct fields (Go lays them out at the correct offsets)
	e.Offset = offset
	e.Size = size

	// Increment entry count
	h.EntryCount++
	binary.LittleEndian.PutUint32(t.data[TableEntryCountOffset:TableEntryCountOffset+U32Size], h.EntryCount)

	return nil
}

// Allocate reserves space for a new structure.
func (t *Table) Allocate(size int, alignment int) int {
	h := t.Header()

	// Align the offset
	aligned := (int(h.NextOffset) + alignment - 1) &^ (alignment - 1)
	result := aligned

	// Check bounds
	if aligned+size > t.memorySize {
		panic(fmt.Sprintf("allocation of %d bytes at offset %d would exceed memory size %d",
			size, aligned, t.memorySize))
	}

	// Update next offset
	binary.LittleEndian.PutUint64(t.data[TableNextOffsetOffset:TableNextOffsetOffset+U64Size], uint64(aligned+size))

	return result
}

// EntryCount returns the number of entries in the table.
func (t *Table) EntryCount() int {
	return int(t.Header().EntryCount)
}

// MaxEntries returns the maximum number of entries this table can hold.
func (t *Table) MaxEntries() int {
	return t.maxEntries
}

// NextOffset returns the next allocation offset.
func (t *Table) NextOffset() uint64 {
	return t.Header().NextOffset
}

// Entries returns a slice of all entries.
func (t *Table) Entries() []*Entry {
	count := t.EntryCount()
	entries := make([]*Entry, count)
	for i := 0; i < count; i++ {
		entries[i] = t.Entry(i)
	}
	return entries
}

// Remove removes an entry from the table by name.
// Subsequent entries are shifted down to fill the gap.
func (t *Table) Remove(name string) error {
	h := t.Header()
	if h.EntryCount == 0 {
		return errors.New("table is empty")
	}

	// Find the entry index
	index := -1
	for i := 0; i < int(h.EntryCount); i++ {
		e := t.Entry(i)
		if e.NameString() == name {
			index = i
			break
		}
	}

	if index == -1 {
		return fmt.Errorf("entry '%s' not found", name)
	}

	// Shift entries down
	for i := index; i < int(h.EntryCount)-1; i++ {
		currentOffset := HeaderSize + i*EntrySize
		nextOffset := HeaderSize + (i+1)*EntrySize
		copy(t.data[currentOffset:currentOffset+EntrySize], t.data[nextOffset:nextOffset+EntrySize])
	}

	// Clear the last entry
	lastIndex := int(h.EntryCount) - 1
	lastEntry := t.Entry(lastIndex)
	lastEntry.Hash = 0
	for i := range lastEntry.Name {
		lastEntry.Name[i] = 0
	}
	lastEntry.Offset = 0
	lastEntry.Size = 0

	// Decrement entry count
	h.EntryCount--
	binary.LittleEndian.PutUint32(t.data[TableEntryCountOffset:TableEntryCountOffset+U32Size], h.EntryCount)

	return nil
}

// CalculateTableSize returns the total size of a table with the given max entries.
func CalculateTableSize(maxEntries int) int {
	return HeaderSize + maxEntries*EntrySize
}
