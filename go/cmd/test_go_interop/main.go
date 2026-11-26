// Test Go <-> C++ interoperability for ZeroSHM table with long names and hash verification
//
// Usage:
//   test_go_interop write <shm_name>  -- create shared memory, add entries with long names
//   test_go_interop read <shm_name>   -- open shared memory, verify entries and hashes
package main

import (
	"encoding/binary"
	"fmt"
	"os"
	"strings"

	"github.com/aguasonic/zeroshm/zeroshm"
	"github.com/zeebo/xxh3"
)

const xxHashSeed uint64 = 0x2FE7

// makeName creates a name of the given length filled with the specified character.
func makeName(length int, ch byte) string {
	return strings.Repeat(string(ch), length)
}

func writeEntries(mem *zeroshm.Memory, shmName string) {
	fmt.Printf("Go writer: Creating shared memory '%s'\n", shmName)

	// Create entries with long names (matching C/C++ test pattern)
	name50 := makeName(50, 'a')
	name100 := makeName(100, 'b')
	name200 := makeName(200, 'c')
	name254 := makeName(254, 'd')

	// Add entries using table's Add which computes offsets via Allocate
	offset, err := mem.Allocate(name50, 100)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Go writer: Failed to add %s: %v\n", name50[:10]+"...", err)
		os.Exit(1)
	}
	off50 := offset

	offset, err = mem.Allocate(name100, 200)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Go writer: Failed to add %s: %v\n", name100[:10]+"...", err)
		os.Exit(1)
	}
	off100 := offset

	offset, err = mem.Allocate(name200, 300)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Go writer: Failed to add %s: %v\n", name200[:10]+"...", err)
		os.Exit(1)
	}
	off200 := offset

	offset, err = mem.Allocate(name254, 400)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Go writer: Failed to add %s: %v\n", name254[:10]+"...", err)
		os.Exit(1)
	}
	off254 := offset

	offset, err = mem.Allocate("short_name", 500)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Go writer: Failed to add short_name: %v\n", err)
		os.Exit(1)
	}
	offShort := offset

	// Write some data at the allocated offsets
	data := mem.Data()
	copy(data[off50:], "data50")
	copy(data[off100:], "data100")
	copy(data[off200:], "data200")
	copy(data[off254:], "data254")
	copy(data[offShort:], "data_short")

	fmt.Printf("Go writer: Added 5 entries (including long names up to 254 chars)\n")
	fmt.Printf("Go writer: Entry count: %d\n", mem.Table().EntryCount())
}

func readEntries(mem *zeroshm.Memory, shmName string) {
	fmt.Printf("Go reader: Opening shared memory '%s'\n", shmName)

	// Verify entries are found via hash-first lookup
	name50 := makeName(50, 'a')
	name100 := makeName(100, 'b')
	name200 := makeName(200, 'c')
	name254 := makeName(254, 'd')

	check := func(name string, expectedSize uint64) {
		entry := mem.Find(name)
		if entry == nil {
			fmt.Fprintf(os.Stderr, "Go reader: FAILED to find '%s' (len=%d)\n", name[:10]+"...", len(name))
			os.Exit(1)
		}
		if entry.Size != expectedSize {
			fmt.Fprintf(os.Stderr, "Go reader: FAILED size mismatch for '%s': got %d, expected %d\n",
				name[:10]+"...", entry.Size, expectedSize)
			os.Exit(1)
		}
		fmt.Printf("Go reader: Found '%s...' (offset=%d, size=%d)\n", name[:10], entry.Offset, entry.Size)
	}

	check(name50, 100)
	check(name100, 200)
	check(name200, 300)
	check(name254, 400)
	check("short_name", 500)

	// Verify hash values match by reading raw entry data
	table := mem.Table()
	data := mem.Data()
	count := table.EntryCount()

	for i := 0; i < count; i++ {
		// Read hash directly from entry
		entryOffset := zeroshm.HeaderSize + i*zeroshm.EntrySize
		storedHash := binary.LittleEndian.Uint64(data[entryOffset+zeroshm.EntryHashOffset : entryOffset+zeroshm.EntryHashOffset+8])

		// Read name from entry
		nameBytes := data[entryOffset+zeroshm.EntryNameOffset : entryOffset+zeroshm.EntryNameEnd]
		var name string
		for j, b := range nameBytes {
			if b == 0 {
				name = string(nameBytes[:j])
				break
			}
		}

		expectedHash := xxh3.HashStringSeed(name, xxHashSeed)
		if storedHash != expectedHash {
			fmt.Fprintf(os.Stderr, "Go reader: FAILED hash mismatch for '%s': stored=0x%016X, expected=0x%016X\n",
				name[:10]+"...", storedHash, expectedHash)
			os.Exit(1)
		}
	}

	fmt.Printf("Go reader: All %d entries have correct hash values\n", count)
	fmt.Printf("Go reader: Verified entry count: %d\n", count)
}

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintf(os.Stderr, "Usage: %s <write|read> <shm_name>\n", os.Args[0])
		os.Exit(1)
	}

	mode := os.Args[1]
	shmName := os.Args[2]

	if mode == "write" {
		// Clean up any existing segment before creating new one
		zeroshm.UnlinkName(shmName)

		// Create shared memory with enough space for table + data
		// Table with 64 entries at 280 bytes each = 32 + 64*280 = 17952 bytes
		// Plus data area: 4MB total
		mem, err := zeroshm.NewMemory(shmName, 4*1024*1024, 64)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Go writer: Failed to create memory: %v\n", err)
			os.Exit(1)
		}
		defer mem.Close()

		writeEntries(mem, shmName)
	} else if mode == "read" {
		mem, err := zeroshm.OpenMemory(shmName, 64)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Go reader: Failed to open memory: %v\n", err)
			os.Exit(1)
		}
		defer mem.Close()

		readEntries(mem, shmName)
	} else {
		fmt.Fprintf(os.Stderr, "Unknown mode: %s\n", mode)
		os.Exit(1)
	}
}
