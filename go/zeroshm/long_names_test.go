package zeroshm

import (
	"strings"
	"testing"

	"github.com/zeebo/xxh3"
)

func TestLongNames(t *testing.T) {
	testCases := []struct {
		nameLen  int
		shouldOK bool
	}{
		{50, true},
		{100, true},
		{200, true},
		{254, true}, // max allowed (254 chars + 1 null = 255 bytes)
		{255, false}, // too long
		{300, false}, // way too long
	}

	for _, tc := range testCases {
		t.Run(strings.Repeat("a", tc.nameLen)[:10]+"...", func(t *testing.T) {
			// Table with 8 entries: 32 + 8*384 = 3104 bytes (EntrySize is 384 per spec)
			tableSize := HeaderSize + 8*EntrySize
			data := make([]byte, tableSize+1024)  // Extra space for the allocated region
			table := NewTable(data, 8, len(data), true)

			name := strings.Repeat("a", tc.nameLen)
			err := table.Add(name, uint64(tableSize), 100)

			if tc.shouldOK {
				if err != nil {
					t.Errorf("Expected success for name length %d, got error: %v", tc.nameLen, err)
				}
				// Verify entry was found
				entry := table.Find(name)
				if entry == nil {
					t.Errorf("Expected to find name with length %d", tc.nameLen)
				} else {
					// Verify hash is stored correctly
					expectedHash := xxh3.HashStringSeed(name, XXHashSeed)
					if entry.Hash != expectedHash {
						t.Errorf("Hash mismatch for name length %d: stored=0x%016X, expected=0x%016X",
							tc.nameLen, entry.Hash, expectedHash)
					}
				}
			} else {
				if err == nil {
					t.Errorf("Expected error for name length %d, got success", tc.nameLen)
				}
			}
		})
	}
}

func TestHashVerification(t *testing.T) {
	// Table with 4 entries: 32 + 4*280 = 1152 bytes
	data := make([]byte, 4096)
	table := NewTable(data, 4, 4096, true)

	names := []string{"alpha", "beta", "gamma", "delta"}
	for _, name := range names {
		err := table.Add(name, 4096, 100)
		if err != nil {
			t.Fatalf("Failed to add %s: %v", name, err)
		}
	}

	if table.EntryCount() != 4 {
		t.Fatalf("Expected 4 entries, got %d", table.EntryCount())
	}

	// Verify each entry's stored hash matches the computed hash
	for i, name := range names {
		entry := table.Entry(i)
		expectedHash := xxh3.HashStringSeed(name, XXHashSeed)
		if entry.Hash != expectedHash {
			t.Errorf("Entry %d (%s): stored hash 0x%016X != expected 0x%016X",
				i, name, entry.Hash, expectedHash)
		}
		if entry.NameString() != name {
			t.Errorf("Entry %d: name mismatch: stored=%q, expected=%q",
				i, entry.NameString(), name)
		}
	}
}

func TestHashCollisionResolution(t *testing.T) {
	// Test that hash-first comparison falls back to string comparison
	// We can't easily create an xxhash3 collision, but we can verify the logic works
	// by ensuring that Find correctly distinguishes between different names

	data := make([]byte, 4096)
	table := NewTable(data, 8, 4096, true)

	table.Add("test_a", 4096, 100)
	table.Add("test_b", 4196, 200)

	// Find should return the correct entry for each name
	entryA := table.Find("test_a")
	if entryA == nil || entryA.Size != 100 {
		t.Error("Failed to find test_a or wrong size")
	}

	entryB := table.Find("test_b")
	if entryB == nil || entryB.Size != 200 {
		t.Error("Failed to find test_b or wrong size")
	}

	// Non-existent name should return nil
	entryC := table.Find("test_c")
	if entryC != nil {
		t.Error("Found non-existent name test_c")
	}
}
