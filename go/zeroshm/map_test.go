package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
)

func setupMapTest[K Numeric, V Numeric](t *testing.T, namePrefix string, capacity int) (*Memory, *Map[K, V]) {
	t.Helper()
	name := fmt.Sprintf("/map_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	m, err := NewMap[K, V](mem, "map_"+namePrefix, capacity)
	if err != nil {
		t.Fatalf("NewMap failed: %v", err)
	}
	t.Cleanup(func() { m.Destroy() })

	return mem, m
}

func TestMapName(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "name", 16)
	if m.Name() != "map_name" {
		t.Errorf("expected name 'map_name', got %q", m.Name())
	}
}

func TestMapCreateEmpty(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "empty", 16)
	if !m.Empty() {
		t.Error("expected map to be empty")
	}
	if m.Size() != 0 {
		t.Errorf("expected size 0, got %d", m.Size())
	}
	if m.Capacity() != 16 {
		t.Errorf("expected capacity 16, got %d", m.Capacity())
	}
}

func TestMapSetAndGet(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "setget", 16)

	if err := m.Set(1, 100); err != nil {
		t.Fatalf("Set failed: %v", err)
	}
	val, err := m.Get(1)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 100 {
		t.Errorf("expected value 100, got %d", val)
	}
	if m.Size() != 1 {
		t.Errorf("expected size 1, got %d", m.Size())
	}
}

func TestMapGetNotFound(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "notfound", 16)

	_, err := m.Get(999)
	if err != MapNotFound {
		t.Errorf("expected MapNotFound, got %v", err)
	}
}

func TestMapSetUpdate(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "update", 16)

	if err := m.Set(1, 100); err != nil {
		t.Fatalf("Set failed: %v", err)
	}
	if err := m.Set(1, 200); err != nil {
		t.Fatalf("Update failed: %v", err)
	}
	val, err := m.Get(1)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 200 {
		t.Errorf("expected updated value 200, got %d", val)
	}
	if m.Size() != 1 {
		t.Errorf("expected size 1 after update, got %d", m.Size())
	}
}

func TestMapSetMultiple(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "multi", 16)

	for i := 1; i <= 10; i++ {
		if err := m.Set(int32(i), int32(i*10)); err != nil {
			t.Fatalf("Set %d failed: %v", i, err)
		}
	}
	if m.Size() != 10 {
		t.Errorf("expected size 10, got %d", m.Size())
	}
	for i := 1; i <= 10; i++ {
		val, err := m.Get(int32(i))
		if err != nil {
			t.Fatalf("Get %d failed: %v", i, err)
		}
		if val != int32(i*10) {
			t.Errorf("expected value %d for key %d, got %d", i*10, i, val)
		}
	}
}

func TestMapDelete(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "delete", 16)

	m.Set(1, 100)
	m.Set(2, 200)

	if err := m.Delete(1); err != nil {
		t.Fatalf("Delete failed: %v", err)
	}
	if m.Contains(1) {
		t.Error("expected map not to contain key 1 after delete")
	}
	if !m.Contains(2) {
		t.Error("expected map to still contain key 2")
	}
	if m.Size() != 1 {
		t.Errorf("expected size 1 after delete, got %d", m.Size())
	}
}

func TestMapDeleteNotFound(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "delfail", 16)

	m.Set(1, 100)

	if err := m.Delete(999); err != MapNotFound {
		t.Errorf("expected MapNotFound, got %v", err)
	}
	if m.Size() != 1 {
		t.Errorf("expected size 1 after failed delete, got %d", m.Size())
	}
}

func TestMapContains(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "contains", 16)

	m.Set(1, 100)

	if !m.Contains(1) {
		t.Error("expected map to contain key 1")
	}
	if m.Contains(999) {
		t.Error("expected map not to contain key 999")
	}
}

func TestMapFull(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "full", 4)

	for i := 0; i < 4; i++ {
		if err := m.Set(int32(i), int32(i)); err != nil {
			t.Fatalf("Set %d failed: %v", i, err)
		}
	}
	if err := m.Set(10, 10); err != MapFull {
		t.Errorf("expected MapFull, got %v", err)
	}
}

func TestMapClear(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "clear", 16)

	for i := 1; i <= 5; i++ {
		m.Set(int32(i), int32(i*10))
	}
	if m.Size() != 5 {
		t.Errorf("expected size 5 before clear, got %d", m.Size())
	}

	m.Clear()

	if m.Size() != 0 {
		t.Errorf("expected size 0 after clear, got %d", m.Size())
	}
	if !m.Empty() {
		t.Error("expected map to be empty after clear")
	}
}

func TestMapRange(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "range", 16)

	m.Set(1, 10)
	m.Set(2, 20)
	m.Set(3, 30)

	count := 0
	m.Range(func(key, value int32) bool {
		count++
		if value != key*10 {
			t.Errorf("expected value %d for key %d, got %d", key*10, key, value)
		}
		return true
	})
	if count != 3 {
		t.Errorf("expected 3 entries in range, got %d", count)
	}
}

func TestMapRangeEarlyStop(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "rangestop", 16)

	for i := 1; i <= 5; i++ {
		m.Set(int32(i), int32(i))
	}

	count := 0
	m.Range(func(key, value int32) bool {
		count++
		return count < 2
	})
	if count != 2 {
		t.Errorf("expected 2 entries before early stop, got %d", count)
	}
}

func TestMapKeys(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "keys", 16)

	m.Set(1, 10)
	m.Set(3, 30)
	m.Set(5, 50)

	keys := m.Keys()
	if len(keys) != 3 {
		t.Errorf("expected 3 keys, got %d", len(keys))
	}
	keySet := make(map[int32]bool)
	for _, k := range keys {
		keySet[k] = true
	}
	for _, expected := range []int32{1, 3, 5} {
		if !keySet[expected] {
			t.Errorf("expected key %d in keys, got %v", expected, keys)
		}
	}
}

func TestMapEntries(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "entries", 8)

	m.Set(1, 100)
	m.Set(2, 200)

	entries := m.Entries()
	if len(entries) != 8 {
		t.Errorf("expected 8 entries (including empty), got %d", len(entries))
	}
	occupied := 0
	for _, e := range entries {
		if e.State == MapStateOccupied {
			occupied++
		}
	}
	if occupied != 2 {
		t.Errorf("expected 2 occupied entries, got %d", occupied)
	}
}

func TestMapOpenCrossProcess(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "crossproc", 16)

	m.Set(1, 100)
	m.Set(2, 200)

	// Open in a new "process" (same memory, new handle)
	m2, err := OpenMap[int32, int32](m.memory, "map_crossproc")
	if err != nil {
		t.Fatalf("OpenMap failed: %v", err)
	}
	defer m2.Destroy()

	val, err := m2.Get(1)
	if err != nil {
		t.Fatalf("Get from opened map failed: %v", err)
	}
	if val != 100 {
		t.Errorf("expected value 100, got %d", val)
	}

	val, err = m2.Get(2)
	if err != nil {
		t.Fatalf("Get from opened map failed: %v", err)
	}
	if val != 200 {
		t.Errorf("expected value 200, got %d", val)
	}

	if m2.Size() != 2 {
		t.Errorf("expected size 2, got %d", m2.Size())
	}
}

func TestMapOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/map_notfound_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenMap[int32, int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for non-existent map")
	}
}

func TestMapNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/map_longname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	longName := ""
	for i := 0; i < 300; i++ {
		longName += "a"
	}
	_, err = NewMap[int32, int32](mem, longName, 16)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestMapNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/map_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewMap[int32, int32](mem, "ab", 16)
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestMapNewAlreadyExists(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "dup", 16)
	_ = m

	_, err := NewMap[int32, int32](m.memory, "map_dup", 16)
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestMapNewInvalidCapacity(t *testing.T) {
	name := fmt.Sprintf("/map_invalidcap_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewMap[int32, int32](mem, "map_invalidcap", 0)
	if err == nil {
		t.Error("expected error for zero capacity")
	}
}

func TestMapOpenTypeMismatch(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "typemismatch", 16)
	_ = m

	// Try to open with different type sizes
	_, err := OpenMap[int64, int32](m.memory, "map_typemismatch")
	if err == nil {
		t.Error("expected error for type mismatch")
	}
}

func TestMapConcurrentSet(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "concurrentset", 64)

	var wg sync.WaitGroup
	workers := 8
	itemsPerWorker := 8

	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func(worker int) {
			defer wg.Done()
			for i := 0; i < itemsPerWorker; i++ {
				key := int32(worker*itemsPerWorker + i)
				m.Set(key, key*10)
			}
		}(w)
	}
	wg.Wait()

	expected := workers * itemsPerWorker
	if m.Size() != expected {
		t.Errorf("expected size %d, got %d", expected, m.Size())
	}

	// Verify all values
	for w := 0; w < workers; w++ {
		for i := 0; i < itemsPerWorker; i++ {
			key := int32(w*itemsPerWorker + i)
			val, err := m.Get(key)
			if err != nil {
				t.Errorf("Get key %d failed: %v", key, err)
			}
			if val != key*10 {
				t.Errorf("expected value %d for key %d, got %d", key*10, key, val)
			}
		}
	}
}

func TestMapConcurrentSetDelete(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "concurrentsd", 64)

	// Pre-populate
	for i := 0; i < 32; i++ {
		m.Set(int32(i), int32(i))
	}

	var wg sync.WaitGroup

	// Writers: set new keys
	for w := 0; w < 4; w++ {
		wg.Add(1)
		go func(worker int) {
			defer wg.Done()
			for i := 0; i < 8; i++ {
				key := int32(32 + worker*8 + i)
				m.Set(key, key)
			}
		}(w)
	}

	// Deleters: delete existing keys
	for d := 0; d < 4; d++ {
		wg.Add(1)
		go func(deleter int) {
			defer wg.Done()
			for i := 0; i < 8; i++ {
				key := int32(deleter*8 + i)
				m.Delete(key)
			}
		}(d)
	}

	wg.Wait()

	// Size should be: 32 initial - 32 deleted + 32 new = 32
	if m.Size() != 32 {
		t.Errorf("expected size 32, got %d", m.Size())
	}

	// Verify new keys exist
	for i := 32; i < 64; i++ {
		if !m.Contains(int32(i)) {
			t.Errorf("expected key %d to exist", i)
		}
	}
}

func TestMapInt64Values(t *testing.T) {
	_, m := setupMapTest[int32, int64](t, "int64", 16)

	m.Set(1, int64(1234567890123))
	m.Set(2, int64(-9876543210987))

	val, err := m.Get(1)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != int64(1234567890123) {
		t.Errorf("expected value 1234567890123, got %d", val)
	}

	val, err = m.Get(2)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != int64(-9876543210987) {
		t.Errorf("expected value -9876543210987, got %d", val)
	}
}

func TestMapFloat64Values(t *testing.T) {
	_, m := setupMapTest[int32, float64](t, "float64", 16)

	m.Set(1, 3.14159)
	m.Set(2, -2.71828)

	val, err := m.Get(1)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 3.14159 {
		t.Errorf("expected value 3.14159, got %f", val)
	}

	val, err = m.Get(2)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != -2.71828 {
		t.Errorf("expected value -2.71828, got %f", val)
	}
}

func TestMapUint64Keys(t *testing.T) {
	_, m := setupMapTest[uint64, int32](t, "uint64key", 16)

	m.Set(uint64(18446744073709551615), 100) // max uint64
	m.Set(uint64(0), 200)

	val, err := m.Get(uint64(18446744073709551615))
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 100 {
		t.Errorf("expected value 100, got %d", val)
	}

	val, err = m.Get(uint64(0))
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 200 {
		t.Errorf("expected value 200, got %d", val)
	}
}

func TestMapDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/map_destroy_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	m, err := NewMap[int32, int32](mem, "map_destroy", 16)
	if err != nil {
		t.Fatalf("NewMap failed: %v", err)
	}
	m.Set(1, 100)

	m.Destroy()

	// Name should be reusable
	m2, err := NewMap[int32, int32](mem, "map_destroy", 16)
	if err != nil {
		t.Fatalf("NewMap after destroy failed: %v", err)
	}
	defer m2.Destroy()

	if m2.Size() != 0 {
		t.Errorf("expected size 0 in new map, got %d", m2.Size())
	}
}

func TestMapDeleteAndReinsert(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "reinsert", 16)

	m.Set(1, 100)
	m.Set(2, 200)

	// Delete key 1, creating a tombstone
	m.Delete(1)

	// Reinsert key 1 - should reuse the tombstone slot
	if err := m.Set(1, 300); err != nil {
		t.Fatalf("Reinsert failed: %v", err)
	}

	val, err := m.Get(1)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 300 {
		t.Errorf("expected value 300, got %d", val)
	}

	if m.Size() != 2 {
		t.Errorf("expected size 2, got %d", m.Size())
	}
}

func TestMapDeleteThroughTombstones(t *testing.T) {
	_, m := setupMapTest[int32, int32](t, "tomstone", 16)

	// Insert multiple keys that will probe through the same chain
	m.Set(1, 10)
	m.Set(2, 20)
	m.Set(3, 30)

	// Delete the first key in the chain
	m.Delete(1)

	// Keys 2 and 3 should still be findable (probe continues through tombstone)
	val, err := m.Get(2)
	if err != nil {
		t.Fatalf("Get 2 failed: %v", err)
	}
	if val != 20 {
		t.Errorf("expected value 20, got %d", val)
	}

	val, err = m.Get(3)
	if err != nil {
		t.Fatalf("Get 3 failed: %v", err)
	}
	if val != 30 {
		t.Errorf("expected value 30, got %d", val)
	}
}
