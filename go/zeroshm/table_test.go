package zeroshm

import (
	"testing"
)

// TestTableNewCreate tests creating a new table.
func TestTableNewCreate(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	h := table.Header()
	if h.Magic != TableMagic {
		t.Errorf("Magic = 0x%08X, want 0x%08X", h.Magic, TableMagic)
	}
	if h.Version != TableVersion {
		t.Errorf("Version = %d, want %d", h.Version, TableVersion)
	}
	if h.EntryCount != 0 {
		t.Errorf("EntryCount = %d, want 0", h.EntryCount)
	}
	if h.MaxEntries != 64 {
		t.Errorf("MaxEntries = %d, want 64", h.MaxEntries)
	}
	if h.MemorySize != 1024*1024 {
		t.Errorf("MemorySize = %d, want %d", h.MemorySize, 1024*1024)
	}
}

// TestTableNewOpen tests opening an existing table.
func TestTableNewOpen(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Add an entry
	if err := table.Add("test_entry", 1000, 64); err != nil {
		t.Fatalf("Add failed: %v", err)
	}

	// Open the same table (validate mode)
	table2 := NewTable(data, 64, 0, false)
	h := table2.Header()
	if h.EntryCount != 1 {
		t.Errorf("EntryCount = %d, want 1", h.EntryCount)
	}
}

// TestTableHeader tests the Header method.
func TestTableHeader(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	h := table.Header()
	if h == nil {
		t.Fatal("Header() returned nil")
	}
	if h.MaxEntries != 64 {
		t.Errorf("MaxEntries = %d, want 64", h.MaxEntries)
	}
}

// TestTableEntry tests the Entry method.
func TestTableEntry(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Valid index
	e := table.Entry(0)
	if e == nil {
		t.Fatal("Entry(0) returned nil")
	}
	if e.NameString() != "" {
		t.Errorf("Entry(0).NameString() = %q, want empty", e.NameString())
	}

	// Invalid index should panic
	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Entry(-1) should panic")
			}
		}()
		table.Entry(-1)
	}()

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Entry(64) should panic")
			}
		}()
		table.Entry(64)
	}()
}

// TestTableFind tests the Find method.
func TestTableFind(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Find non-existent entry
	e := table.Find("nonexistent")
	if e != nil {
		t.Error("Find(nonexistent) should return nil")
	}

	// Add entries and find them
	if err := table.Add("alpha", 1000, 64); err != nil {
		t.Fatalf("Add alpha failed: %v", err)
	}
	if err := table.Add("beta", 2000, 128); err != nil {
		t.Fatalf("Add beta failed: %v", err)
	}

	e = table.Find("alpha")
	if e == nil {
		t.Fatal("Find(alpha) should return entry")
	}
	if e.NameString() != "alpha" {
		t.Errorf("NameString() = %q, want alpha", e.NameString())
	}
	if e.Offset != 1000 {
		t.Errorf("Offset = %d, want 1000", e.Offset)
	}
	if e.Size != 64 {
		t.Errorf("Size = %d, want 64", e.Size)
	}

	e = table.Find("beta")
	if e == nil {
		t.Fatal("Find(beta) should return entry")
	}
	if e.Offset != 2000 {
		t.Errorf("Offset = %d, want 2000", e.Offset)
	}
}

// TestTableAdd tests the Add method.
func TestTableAdd(t *testing.T) {
	data := make([]byte, CalculateTableSize(4))
	table := NewTable(data, 4, 1024*1024, true)

	// Add valid entries
	if err := table.Add("entry1", 1000, 64); err != nil {
		t.Fatalf("Add entry1 failed: %v", err)
	}
	if err := table.Add("entry2", 2000, 128); err != nil {
		t.Fatalf("Add entry2 failed: %v", err)
	}
	if err := table.Add("entry3", 3000, 256); err != nil {
		t.Fatalf("Add entry3 failed: %v", err)
	}
	if err := table.Add("entry4", 4000, 512); err != nil {
		t.Fatalf("Add entry4 failed: %v", err)
	}

	// Table should now be full
	if table.EntryCount() != 4 {
		t.Errorf("EntryCount = %d, want 4", table.EntryCount())
	}

	// Adding to full table should fail
	err := table.Add("entry5", 5000, 64)
	if err == nil {
		t.Error("Add to full table should fail")
	}
}

// TestTableAddDuplicate tests adding a duplicate name.
func TestTableAddDuplicate(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	if err := table.Add("dup", 1000, 64); err != nil {
		t.Fatalf("Add dup failed: %v", err)
	}

	err := table.Add("dup", 2000, 128)
	if err == nil {
		t.Error("Add duplicate name should fail")
	}
}

// TestTableAddLongName tests adding a name that's too long.
func TestTableAddLongName(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// NameSize is 255, so a 255-char name should fail
	longName := ""
	for i := 0; i < 255; i++ {
		longName += "a"
	}

	err := table.Add(longName, 1000, 64)
	if err == nil {
		t.Error("Add with name too long should fail")
	}
}

// TestTableAllocate tests the Allocate method.
func TestTableAllocate(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Initial NextOffset should be the table size
	tableSize := CalculateTableSize(64)
	if table.NextOffset() != uint64(tableSize) {
		t.Errorf("Initial NextOffset = %d, want %d", table.NextOffset(), tableSize)
	}

	// Allocate 64 bytes with alignment 8
	offset := table.Allocate(64, 8)
	if offset != tableSize {
		t.Errorf("First Allocate offset = %d, want %d", offset, tableSize)
	}

	// Next allocation should be after the first
	nextOffset := table.NextOffset()
	expectedNext := tableSize + 64
	if nextOffset != uint64(expectedNext) {
		t.Errorf("NextOffset after allocate = %d, want %d", nextOffset, expectedNext)
	}

	offset2 := table.Allocate(128, 8)
	if offset2 != expectedNext {
		t.Errorf("Second Allocate offset = %d, want %d", offset2, expectedNext)
	}
}

// TestTableAllocateAlignment tests alignment handling.
func TestTableAllocateAlignment(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Allocate to get to a misaligned offset
	table.Allocate(1, 8) // offset = tableSize, next = tableSize+1

	// Now allocate with alignment 8 - should round up to next 8-byte boundary
	offset := table.Allocate(8, 8)
	if offset%8 != 0 {
		t.Errorf("Aligned offset = %d, should be multiple of 8", offset)
	}
}

// TestTableAllocateOverflow tests allocation that exceeds memory size.
func TestTableAllocateOverflow(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Allocate exceeding memory size should panic")
			}
		}()
		table.Allocate(2*1024*1024, 8)
	}()
}

// TestTableEntryCount tests the EntryCount method.
func TestTableEntryCount(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	if table.EntryCount() != 0 {
		t.Errorf("Initial EntryCount = %d, want 0", table.EntryCount())
	}

	table.Add("aaa", 1000, 64)
	if table.EntryCount() != 1 {
		t.Errorf("EntryCount after 1 add = %d, want 1", table.EntryCount())
	}

	table.Add("bbb", 2000, 128)
	if table.EntryCount() != 2 {
		t.Errorf("EntryCount after 2 adds = %d, want 2", table.EntryCount())
	}
}

// TestTableMaxEntries tests the MaxEntries method.
func TestTableMaxEntries(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	if table.MaxEntries() != 64 {
		t.Errorf("MaxEntries = %d, want 64", table.MaxEntries())
	}
}

// TestTableNextOffset tests the NextOffset method.
func TestTableNextOffset(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	tableSize := CalculateTableSize(64)
	if table.NextOffset() != uint64(tableSize) {
		t.Errorf("NextOffset = %d, want %d", table.NextOffset(), tableSize)
	}

	table.Allocate(64, 8)
	if table.NextOffset() != uint64(tableSize+64) {
		t.Errorf("NextOffset after allocate = %d, want %d", table.NextOffset(), tableSize+64)
	}
}

// TestTableEntries tests the Entries method.
func TestTableEntries(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Empty table
	entries := table.Entries()
	if len(entries) != 0 {
		t.Errorf("Entries() len = %d, want 0", len(entries))
	}

	table.Add("aaa", 1000, 64)
	table.Add("bbb", 2000, 128)
	table.Add("ccc", 3000, 256)

	entries = table.Entries()
	if len(entries) != 3 {
		t.Errorf("Entries() len = %d, want 3", len(entries))
	}
	if entries[0].NameString() != "aaa" {
		t.Errorf("Entries()[0].Name = %q, want aaa", entries[0].NameString())
	}
	if entries[1].NameString() != "bbb" {
		t.Errorf("Entries()[1].Name = %q, want bbb", entries[1].NameString())
	}
	if entries[2].NameString() != "ccc" {
		t.Errorf("Entries()[2].Name = %q, want ccc", entries[2].NameString())
	}
}

// TestCalculateTableSize tests the CalculateTableSize function.
func TestCalculateTableSize(t *testing.T) {
	size := CalculateTableSize(0)
	if size != HeaderSize {
		t.Errorf("CalculateTableSize(0) = %d, want %d", size, HeaderSize)
	}

	size = CalculateTableSize(64)
	expected := HeaderSize + 64*EntrySize
	if size != expected {
		t.Errorf("CalculateTableSize(64) = %d, want %d", size, expected)
	}

	size = CalculateTableSize(1024)
	expected = HeaderSize + 1024*EntrySize
	if size != expected {
		t.Errorf("CalculateTableSize(1024) = %d, want %d", size, expected)
	}
}

// TestEntryNameString tests the NameString method.
func TestEntryNameString(t *testing.T) {
	e := &Entry{}
	for i := 0; i < 5; i++ {
		e.Name[i] = "hello"[i]
	}
	// Name[5] is already 0 (zero value)

	if e.NameString() != "hello" {
		t.Errorf("NameString() = %q, want hello", e.NameString())
	}

	// Empty name
	e2 := &Entry{}
	if e2.NameString() != "" {
		t.Errorf("Empty NameString() = %q, want empty", e2.NameString())
	}
}

// TestTableNameTooShort tests Add and Find with name too short.
func TestTableNameTooShort(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	table := NewTable(data, 64, 1024*1024, true)

	// Name shorter than 3 characters should fail
	err := table.Add("a", 100, 10)
	if err == nil {
		t.Error("Add with 1-char name should fail")
	}

	err = table.Add("ab", 100, 10)
	if err == nil {
		t.Error("Add with 2-char name should fail")
	}

	// Find with short name should return nil
	if table.Find("a") != nil {
		t.Error("Find with 1-char name should return nil")
	}

	// Valid name (3 chars) should succeed
	err = table.Add("abc", 100, 10)
	if err != nil {
		t.Errorf("Add with 3-char name should succeed, got: %v", err)
	}
}

// TestTableValidateBadMagic tests validation with bad magic.
func TestTableValidateBadMagic(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	// Write bad magic
	data[0] = 0xFF
	data[1] = 0xFF
	data[2] = 0xFF
	data[3] = 0xFF

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("NewTable with bad magic should panic")
			}
		}()
		NewTable(data, 64, 1024*1024, false)
	}()
}

// TestTableValidateBadVersion tests validation with bad version.
func TestTableValidateBadVersion(t *testing.T) {
	data := make([]byte, CalculateTableSize(64))
	// Write valid magic but bad version
	table := NewTable(data, 64, 1024*1024, true)
	h := table.Header()
	h.Version = 999
	table.writeHeader(h)

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("NewTable with bad version should panic")
			}
		}()
		NewTable(data, 64, 1024*1024, false)
	}()
}
