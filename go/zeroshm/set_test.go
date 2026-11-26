package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
)

func setupSetTest[T Numeric](t *testing.T, namePrefix string, capacity int) (*Memory, *Set[T]) {
	t.Helper()
	name := fmt.Sprintf("/set_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	set, err := NewSet[T](mem, "set_"+namePrefix, capacity)
	if err != nil {
		t.Fatalf("NewSet failed: %v", err)
	}
	t.Cleanup(func() { set.Destroy() })

	return mem, set
}

func TestSetName(t *testing.T) {
	_, set := setupSetTest[int32](t, "name", 16)
	if set.Name() != "set_name" {
		t.Errorf("expected name 'set_name', got %q", set.Name())
	}
}

func TestSetCreateEmpty(t *testing.T) {
	_, set := setupSetTest[int32](t, "empty", 16)
	if !set.Empty() {
		t.Error("expected set to be empty")
	}
	if set.Size() != 0 {
		t.Errorf("expected size 0, got %d", set.Size())
	}
	if set.Capacity() != 16 {
		t.Errorf("expected capacity 16, got %d", set.Capacity())
	}
}

func TestSetInsertAndContains(t *testing.T) {
	_, set := setupSetTest[int32](t, "insert", 16)

	if err := set.Insert(42); err != nil {
		t.Fatalf("Insert failed: %v", err)
	}
	if !set.Contains(42) {
		t.Error("expected set to contain 42")
	}
	if set.Contains(99) {
		t.Error("expected set not to contain 99")
	}
	if set.Size() != 1 {
		t.Errorf("expected size 1, got %d", set.Size())
	}
}

func TestSetInsertDuplicate(t *testing.T) {
	_, set := setupSetTest[int32](t, "dup", 16)

	if err := set.Insert(42); err != nil {
		t.Fatalf("Insert failed: %v", err)
	}
	if err := set.Insert(42); err != nil {
		t.Fatalf("Duplicate insert failed: %v", err)
	}
	if set.Size() != 1 {
		t.Errorf("expected size 1 after duplicate insert, got %d", set.Size())
	}
}

func TestSetInsertMultiple(t *testing.T) {
	_, set := setupSetTest[int32](t, "multi", 16)

	for i := 1; i <= 10; i++ {
		if err := set.Insert(int32(i)); err != nil {
			t.Fatalf("Insert %d failed: %v", i, err)
		}
	}
	if set.Size() != 10 {
		t.Errorf("expected size 10, got %d", set.Size())
	}
	for i := 1; i <= 10; i++ {
		if !set.Contains(int32(i)) {
			t.Errorf("expected set to contain %d", i)
		}
	}
}

func TestSetErase(t *testing.T) {
	_, set := setupSetTest[int32](t, "erase", 16)

	set.Insert(42)
	set.Insert(99)

	if err := set.Erase(42); err != nil {
		t.Fatalf("Erase failed: %v", err)
	}
	if set.Contains(42) {
		t.Error("expected set not to contain 42 after erase")
	}
	if !set.Contains(99) {
		t.Error("expected set to still contain 99")
	}
	if set.Size() != 1 {
		t.Errorf("expected size 1 after erase, got %d", set.Size())
	}
}

func TestSetEraseNotFound(t *testing.T) {
	_, set := setupSetTest[int32](t, "erasenf", 16)

	set.Insert(42)

	err := set.Erase(99)
	if err != SetNotFound {
		t.Errorf("expected SetNotFound, got %v", err)
	}
	if set.Size() != 1 {
		t.Errorf("expected size 1 after failed erase, got %d", set.Size())
	}
}

func TestSetFull(t *testing.T) {
	_, set := setupSetTest[int32](t, "full", 4)

	for i := 0; i < 4; i++ {
		if err := set.Insert(int32(i)); err != nil {
			t.Fatalf("Insert %d failed: %v", i, err)
		}
	}
	if set.Size() != 4 {
		t.Errorf("expected size 4, got %d", set.Size())
	}

	err := set.Insert(99)
	if err != SetFull {
		t.Errorf("expected SetFull, got %v", err)
	}
}

func TestSetClear(t *testing.T) {
	_, set := setupSetTest[int32](t, "clear", 16)

	set.Insert(42)
	set.Insert(99)

	set.Clear()

	if !set.Empty() {
		t.Error("expected set to be empty after clear")
	}
	if set.Size() != 0 {
		t.Errorf("expected size 0 after clear, got %d", set.Size())
	}
}

func TestSetRange(t *testing.T) {
	_, set := setupSetTest[int32](t, "range", 16)

	set.Insert(10)
	set.Insert(20)
	set.Insert(30)

	count := 0
	var sum int32 = 0
	set.Range(func(v int32) bool {
		count++
		sum += v
		return true
	})
	if count != 3 {
		t.Errorf("expected 3 elements, got %d", count)
	}
	if sum != 60 {
		t.Errorf("expected sum 60, got %d", sum)
	}
}

func TestSetEntries(t *testing.T) {
	_, set := setupSetTest[int32](t, "entries", 8)

	set.Insert(1)
	set.Insert(2)

	entries := set.Entries()
	if len(entries) != 8 {
		t.Errorf("expected 8 entries, got %d", len(entries))
	}
	occupied := 0
	for _, e := range entries {
		if e.State == SetStateOccupied {
			occupied++
		}
	}
	if occupied != 2 {
		t.Errorf("expected 2 occupied entries, got %d", occupied)
	}
}

func TestSetOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/set_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	set, err := NewSet[int32](mem, "set_cross", 16)
	if err != nil {
		t.Fatalf("NewSet failed: %v", err)
	}

	set.Insert(42)

	// Open the same set
	opened, err := OpenSet[int32](mem, "set_cross")
	if err != nil {
		t.Fatalf("OpenSet failed: %v", err)
	}
	if !opened.Contains(42) {
		t.Error("opened set should contain 42")
	}
}

func TestSetOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/set_nf_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenSet[int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for nonexistent set")
	}
}

func TestSetNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/set_long_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	longName := make([]byte, 255)
	for i := range longName {
		longName[i] = 'a'
	}
	_, err = NewSet[int32](mem, string(longName), 16)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestSetNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/set_short_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewSet[int32](mem, "ab", 16)
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestSetNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/set_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	if _, err := NewSet[int32](mem, "set_exists", 16); err != nil {
		t.Fatalf("First NewSet failed: %v", err)
	}
	_, err = NewSet[int32](mem, "set_exists", 16)
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestSetNewInvalidCapacity(t *testing.T) {
	name := fmt.Sprintf("/set_invcap_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewSet[int32](mem, "set_invcap", 0)
	if err == nil {
		t.Error("expected error for zero capacity")
	}
}

func TestSetOpenTypeMismatch(t *testing.T) {
	name := fmt.Sprintf("/set_typemis_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	if _, err := NewSet[int32](mem, "set_typemis", 16); err != nil {
		t.Fatalf("NewSet failed: %v", err)
	}
	_, err = OpenSet[int64](mem, "set_typemis")
	if err == nil {
		t.Error("expected error for type mismatch")
	}
}

func TestSetConcurrentInsert(t *testing.T) {
	_, set := setupSetTest[int32](t, "concurrent", 256)

	var wg sync.WaitGroup
	workers := 8
	itemsPerWorker := 32

	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func(workerID int) {
			defer wg.Done()
			for i := 0; i < itemsPerWorker; i++ {
				val := workerID*itemsPerWorker + i
				set.Insert(int32(val))
			}
		}(w)
	}
	wg.Wait()

	if set.Size() != workers*itemsPerWorker {
		t.Errorf("expected size %d, got %d", workers*itemsPerWorker, set.Size())
	}

	// Verify all values are present
	for w := 0; w < workers; w++ {
		for i := 0; i < itemsPerWorker; i++ {
			val := w*itemsPerWorker + i
			if !set.Contains(int32(val)) {
				t.Errorf("expected set to contain %d", val)
			}
		}
	}
}

func TestSetConcurrentInsertErase(t *testing.T) {
	_, set := setupSetTest[int32](t, "inserase", 256)

	// Pre-insert some values
	for i := 0; i < 100; i++ {
		set.Insert(int32(i))
	}

	var wg sync.WaitGroup

	// Inserters
	for w := 0; w < 4; w++ {
		wg.Add(1)
		go func(workerID int) {
			defer wg.Done()
			for i := 0; i < 20; i++ {
				set.Insert(int32(100 + workerID*20 + i))
			}
		}(w)
	}

	// Erasers
	for w := 0; w < 4; w++ {
		wg.Add(1)
		go func(workerID int) {
			defer wg.Done()
			for i := 0; i < 10; i++ {
				set.Erase(int32(workerID*10 + i))
			}
		}(w)
	}

	wg.Wait()

	// Final size should be: 100 (initial) - 40 (erased) + 80 (inserted) = 140
	if set.Size() != 140 {
		t.Errorf("expected size 140, got %d", set.Size())
	}
}

func TestSetUint64(t *testing.T) {
	_, set := setupSetTest[uint64](t, "uint64", 16)

	set.Insert(18446744073709551615) // max uint64
	set.Insert(0)

	if !set.Contains(18446744073709551615) {
		t.Error("expected set to contain max uint64")
	}
	if !set.Contains(0) {
		t.Error("expected set to contain 0")
	}
	if set.Size() != 2 {
		t.Errorf("expected size 2, got %d", set.Size())
	}
}

func TestSetFloat64(t *testing.T) {
	_, set := setupSetTest[float64](t, "float64", 16)

	set.Insert(3.14159)
	set.Insert(2.71828)

	if !set.Contains(3.14159) {
		t.Error("expected set to contain 3.14159")
	}
	if !set.Contains(2.71828) {
		t.Error("expected set to contain 2.71828")
	}
	if set.Size() != 2 {
		t.Errorf("expected size 2, got %d", set.Size())
	}
}

func TestSetDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/set_destroy_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	set, err := NewSet[int32](mem, "set_destroy", 16)
	if err != nil {
		t.Fatalf("NewSet failed: %v", err)
	}
	set.Insert(42)
	set.Destroy()

	// Open should fail after destroy
	_, err = OpenSet[int32](mem, "set_destroy")
	if err == nil {
		t.Error("expected error after destroy")
	}

	// Name should be reusable
	set2, err := NewSet[int32](mem, "set_destroy", 16)
	if err != nil {
		t.Fatalf("NewSet after destroy failed: %v", err)
	}
	if !set2.Empty() {
		t.Error("expected new set to be empty")
	}
}

func TestSetEraseAfterInsertProbing(t *testing.T) {
	// Test that erase works correctly with linear probing through tombstones
	_, set := setupSetTest[int32](t, "probe", 16)

	// Insert values that will cause collisions
	for i := 0; i < 8; i++ {
		set.Insert(int32(i))
	}

	// Erase the first value, creating a tombstone
	set.Erase(0)

	// Values inserted after 0 that collided with it should still be found
	for i := 1; i < 8; i++ {
		if !set.Contains(int32(i)) {
			t.Errorf("expected set to contain %d after erasing 0", i)
		}
	}

	// Re-insert 0 (should reuse the tombstone slot)
	if err := set.Insert(0); err != nil {
		t.Fatalf("Re-insert 0 failed: %v", err)
	}
	if !set.Contains(0) {
		t.Error("expected set to contain 0 after re-insert")
	}
}
