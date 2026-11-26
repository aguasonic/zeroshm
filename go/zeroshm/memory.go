// Package zeroshm provides high-performance cross-language shared memory system.
//
// ZeroSHM implements a binary format for data structures in shared memory
// that can be accessed from multiple processes and programming languages.
// This Go implementation is binary-compatible with the C++ and Rust versions.
//
// # Platform support
//
// The shared-memory backend is POSIX shared memory and is supported on Linux
// and macOS: Linux maps names under /dev/shm (see memory_linux.go), while macOS
// uses the POSIX shm_open API (see memory_darwin.go). The package still compiles
// on every GOOS, but on unsupported targets NewMemory and OpenMemory return a
// clear "not supported" error rather than failing the build (see memory_windows.go
// and memory_unsupported.go). A backend for another OS only needs to implement the
// small set of methods in memory_linux.go; contributions are welcome. Note that
// cross-language interop additionally requires the C++/Rust sides to use the
// same OS shared-memory scheme, so a Go-only backend would be Go-to-Go on that platform.
package zeroshm

import (
	"fmt"
	"os"
	"runtime"
	"unsafe"

	"github.com/zeebo/xxh3"
)

// Memory wraps a POSIX shared memory segment and its metadata table.
type Memory struct {
	name       string
	size       int
	maxEntries int
	fd         int
	data       []byte
	table      *Table
	owner      bool
}

// osSegmentName converts a logical segment name (message topic, up to 254 chars)
// to a short OS-level name suitable for shm_open (macOS) or /dev/shm (Linux).
// Uses xxhash3 with the same seed as the table for consistency.
// The result is "/z" + 16 hex digits = 18 chars, well under the 30-char macOS limit.
func osSegmentName(name string) string {
	if len(name) == 0 || name[0] != '/' {
		name = "/" + name
	}
	hash := xxh3.HashStringSeed(name, XXHashSeed)
	return fmt.Sprintf("/z%016x", hash)
}

// unsupportedOSError reports that POSIX shared memory is unavailable on the
// current GOOS. zeroshm supports Linux (via /dev/shm) and macOS (via shm_open).
func unsupportedOSError() error {
	return fmt.Errorf("zeroshm: shared memory requires Linux or macOS (POSIX shared memory); "+
		"GOOS %q is not supported (contributions welcome)", runtime.GOOS)
}

// NewMemory creates a new shared memory segment.
// name should start with "/" (e.g., "/myshm")
// size is the total size in bytes
// maxEntries is the maximum number of table entries (default 64 if 0)
func NewMemory(name string, size int, maxEntries int) (*Memory, error) {
	if maxEntries == 0 {
		maxEntries = 64
	}

	m := &Memory{
		name:       name,
		size:       size,
		maxEntries: maxEntries,
		fd:         -1,
		owner:      true,
	}

	if err := m.create(); err != nil {
		return nil, err
	}

	// Initialize table
	m.table = NewTable(m.data, maxEntries, size, true)

	return m, nil
}

// OpenMemory opens an existing shared memory segment.
func OpenMemory(name string, maxEntries int) (*Memory, error) {
	if maxEntries == 0 {
		maxEntries = 64
	}

	m := &Memory{
		name:       name,
		maxEntries: maxEntries,
		fd:         -1,
		owner:      false,
	}

	if err := m.open(); err != nil {
		return nil, err
	}

	// Open existing table
	m.table = NewTable(m.data, maxEntries, m.size, false)

	return m, nil
}

// Data returns the raw byte slice of the shared memory.
func (m *Memory) Data() []byte {
	return m.data
}

// At returns a pointer to memory at a specific offset.
func (m *Memory) At(offset int) unsafe.Pointer {
	if offset < 0 || offset >= m.size {
		panic(fmt.Sprintf("offset %d out of bounds (size=%d)", offset, m.size))
	}
	return unsafe.Pointer(&m.data[offset])
}

// Table returns the metadata table.
func (m *Memory) Table() *Table {
	return m.table
}

// Size returns the total size of the shared memory segment.
func (m *Memory) Size() int {
	return m.size
}

// Name returns the name of the shared memory segment.
func (m *Memory) Name() string {
	return m.name
}

// IsOwner returns true if this instance created the shared memory.
func (m *Memory) IsOwner() bool {
	return m.owner
}

// Allocate reserves space in shared memory and adds an entry to the table.
func (m *Memory) Allocate(name string, size int) (int, error) {
	offset := m.table.Allocate(size, 8)
	if err := m.table.Add(name, uint64(offset), uint64(size)); err != nil {
		return 0, err
	}
	return offset, nil
}

// Find looks up an entry in the table.
func (m *Memory) Find(name string) *Entry {
	return m.table.Find(name)
}

// Dump writes shared memory info to a file for debugging.
func (m *Memory) Dump(path string) error {
	return os.WriteFile(path, m.data, 0644)
}
