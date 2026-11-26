package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
)

func setupPoolTest[T Numeric](t *testing.T, namePrefix string, capacity int) (*Memory, *Pool[T]) {
	t.Helper()
	name := fmt.Sprintf("/pool_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	pool, err := NewPool[T](mem, "pool_"+namePrefix, capacity)
	if err != nil {
		t.Fatalf("NewPool failed: %v", err)
	}
	t.Cleanup(func() { pool.Destroy() })

	return mem, pool
}

func TestPoolName(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "name", 16)
	if pool.Name() != "pool_name" {
		t.Errorf("expected name 'pool_name', got %q", pool.Name())
	}
}

func TestPoolCreateEmpty(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "empty", 16)
	if !pool.Empty() {
		t.Error("expected pool to be empty")
	}
	if pool.Size() != 0 {
		t.Errorf("expected size 0, got %d", pool.Size())
	}
	if pool.Capacity() != 16 {
		t.Errorf("expected capacity 16, got %d", pool.Capacity())
	}
	if pool.Available() != 16 {
		t.Errorf("expected available 16, got %d", pool.Available())
	}
	if pool.Full() {
		t.Error("expected pool to not be full")
	}
}

func TestPoolAllocateFree(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "allocfree", 16)

	slot, err := pool.Allocate()
	if err != nil {
		t.Fatalf("Allocate failed: %v", err)
	}
	if slot < 0 || slot >= 16 {
		t.Errorf("expected valid slot index, got %d", slot)
	}
	if pool.Size() != 1 {
		t.Errorf("expected size 1, got %d", pool.Size())
	}
	if pool.Available() != 15 {
		t.Errorf("expected available 15, got %d", pool.Available())
	}

	if err := pool.Free(slot); err != nil {
		t.Fatalf("Free failed: %v", err)
	}
	if pool.Size() != 0 {
		t.Errorf("expected size 0 after free, got %d", pool.Size())
	}
	if pool.Available() != 16 {
		t.Errorf("expected available 16 after free, got %d", pool.Available())
	}
}

func TestPoolAllocateExhausted(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "exhausted", 4)

	slots := make([]int, 4)
	for i := 0; i < 4; i++ {
		slot, err := pool.Allocate()
		if err != nil {
			t.Fatalf("Allocate %d failed: %v", i, err)
		}
		slots[i] = slot
	}
	if !pool.Full() {
		t.Error("expected pool to be full")
	}
	if pool.Available() != 0 {
		t.Errorf("expected available 0, got %d", pool.Available())
	}

	// All slots should be unique
	seen := make(map[int]bool)
	for _, s := range slots {
		if seen[s] {
			t.Errorf("duplicate slot %d allocated", s)
		}
		seen[s] = true
	}

	_, err := pool.Allocate()
	if err != PoolExhausted {
		t.Errorf("expected PoolExhausted, got %v", err)
	}
}

func TestPoolFreeAndReallocate(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "realloc", 4)

	// Allocate all 4 slots
	slots := make([]int, 4)
	for i := 0; i < 4; i++ {
		slot, _ := pool.Allocate()
		slots[i] = slot
	}

	// Free the first slot
	pool.Free(slots[0])

	// Allocate again -- should reuse a freed slot
	slot, err := pool.Allocate()
	if err != nil {
		t.Fatalf("Reallocate failed: %v", err)
	}
	if pool.Size() != 4 {
		t.Errorf("expected size 4 after reallocate, got %d", pool.Size())
	}
	_ = slot
}

func TestPoolGetSet(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "getset", 16)

	slot, err := pool.Allocate()
	if err != nil {
		t.Fatalf("Allocate failed: %v", err)
	}

	if err := pool.Set(slot, 42); err != nil {
		t.Fatalf("Set failed: %v", err)
	}

	val, err := pool.Get(slot)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected value 42, got %d", val)
	}

	// Update value
	if err := pool.Set(slot, 99); err != nil {
		t.Fatalf("Set failed: %v", err)
	}
	val, err = pool.Get(slot)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 99 {
		t.Errorf("expected value 99, got %d", val)
	}
}

func TestPoolInvalidSlotGet(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "invalidget", 4)

	_, err := pool.Get(-1)
	if err != PoolInvalidSlot {
		t.Errorf("expected PoolInvalidSlot for -1, got %v", err)
	}

	_, err = pool.Get(4)
	if err != PoolInvalidSlot {
		t.Errorf("expected PoolInvalidSlot for 4, got %v", err)
	}
}

func TestPoolInvalidSlotSet(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "invalidset", 4)

	err := pool.Set(-1, 42)
	if err != PoolInvalidSlot {
		t.Errorf("expected PoolInvalidSlot for -1, got %v", err)
	}

	err = pool.Set(4, 42)
	if err != PoolInvalidSlot {
		t.Errorf("expected PoolInvalidSlot for 4, got %v", err)
	}
}

func TestPoolInvalidSlotFree(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "invalidfree", 4)

	err := pool.Free(-1)
	if err != PoolInvalidSlot {
		t.Errorf("expected PoolInvalidSlot for -1, got %v", err)
	}

	err = pool.Free(4)
	if err != PoolInvalidSlot {
		t.Errorf("expected PoolInvalidSlot for 4, got %v", err)
	}
}

func TestPoolMultipleSlots(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "multislot", 8)

	values := map[int]int32{}
	for i := 0; i < 5; i++ {
		slot, err := pool.Allocate()
		if err != nil {
			t.Fatalf("Allocate %d failed: %v", i, err)
		}
		if err := pool.Set(slot, int32(i*100)); err != nil {
			t.Fatalf("Set failed: %v", err)
		}
		values[slot] = int32(i * 100)
	}

	if pool.Size() != 5 {
		t.Errorf("expected size 5, got %d", pool.Size())
	}

	// Verify all values
	for slot, expected := range values {
		val, err := pool.Get(slot)
		if err != nil {
			t.Fatalf("Get slot %d failed: %v", slot, err)
		}
		if val != expected {
			t.Errorf("slot %d: expected %d, got %d", slot, expected, val)
		}
	}
}

func TestPoolOpenCrossProcess(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "crossproc", 16)

	slot, _ := pool.Allocate()
	pool.Set(slot, 123)

	// Open in a new "process" (same memory, new handle)
	pool2, err := OpenPool[int32](pool.memory, "pool_crossproc")
	if err != nil {
		t.Fatalf("OpenPool failed: %v", err)
	}
	defer pool2.Destroy()

	val, err := pool2.Get(slot)
	if err != nil {
		t.Fatalf("Get from opened pool failed: %v", err)
	}
	if val != 123 {
		t.Errorf("expected value 123, got %d", val)
	}

	// Allocate from the second handle
	slot2, err := pool2.Allocate()
	if err != nil {
		t.Fatalf("Allocate from opened pool failed: %v", err)
	}
	if slot2 == slot {
		t.Error("expected different slot")
	}
	if pool2.Size() != 2 {
		t.Errorf("expected size 2, got %d", pool2.Size())
	}
}

func TestPoolOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/pool_notfound_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenPool[int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for non-existent pool")
	}
}

func TestPoolNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/pool_longname_%d", os.Getpid())
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
	_, err = NewPool[int32](mem, longName, 16)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestPoolNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/pool_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewPool[int32](mem, "ab", 16)
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestPoolNewAlreadyExists(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "dup", 16)
	_ = pool

	_, err := NewPool[int32](pool.memory, "pool_dup", 16)
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestPoolNewInvalidCapacity(t *testing.T) {
	name := fmt.Sprintf("/pool_invalidcap_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewPool[int32](mem, "pool_invalidcap", 0)
	if err == nil {
		t.Error("expected error for zero capacity")
	}
}

func TestPoolOpenTypeMismatch(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "typemismatch", 16)
	_ = pool

	// Try to open with different type sizes
	_, err := OpenPool[int64](pool.memory, "pool_typemismatch")
	if err == nil {
		t.Error("expected error for type mismatch")
	}
}

func TestPoolConcurrentAllocate(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "concurrent", 32)

	var wg sync.WaitGroup
	const numGoroutines = 8
	const allocsPerGoroutine = 4

	slots := make([][]int, numGoroutines)
	for i := range slots {
		slots[i] = make([]int, allocsPerGoroutine)
	}

	// Concurrent allocations
	for i := 0; i < numGoroutines; i++ {
		wg.Add(1)
		go func(gi int) {
			defer wg.Done()
			for j := 0; j < allocsPerGoroutine; j++ {
				for {
					slot, err := pool.Allocate()
					if err == PoolExhausted {
						continue // pool is full, skip
					}
					if err != nil {
						t.Errorf("Allocate failed: %v", err)
						return
					}
					slots[gi][j] = slot
					break
				}
			}
		}(i)
	}

	wg.Wait()

	// Verify no duplicate slots were allocated
	seen := make(map[int]bool)
	total := 0
	for gi := 0; gi < numGoroutines; gi++ {
		for j := 0; j < allocsPerGoroutine; j++ {
			s := slots[gi][j]
			if s < 0 {
				continue
			}
			if seen[s] {
				t.Errorf("duplicate slot %d allocated concurrently", s)
			}
			seen[s] = true
			total++
		}
	}
	if total != pool.Size() {
		t.Errorf("expected pool size %d, got %d", total, pool.Size())
	}
}

func TestPoolInt64(t *testing.T) {
	_, pool := setupPoolTest[int64](t, "int64", 8)

	slot, _ := pool.Allocate()
	pool.Set(slot, int64(1234567890123))

	val, _ := pool.Get(slot)
	if val != int64(1234567890123) {
		t.Errorf("expected 1234567890123, got %d", val)
	}
}

func TestPoolFloat64(t *testing.T) {
	_, pool := setupPoolTest[float64](t, "float64", 8)

	slot, _ := pool.Allocate()
	pool.Set(slot, 3.14159)

	val, _ := pool.Get(slot)
	if val < 3.14158 || val > 3.14160 {
		t.Errorf("expected ~3.14159, got %f", val)
	}
}

func TestPoolUint8(t *testing.T) {
	_, pool := setupPoolTest[uint8](t, "uint8", 8)

	slot, _ := pool.Allocate()
	pool.Set(slot, 255)

	val, _ := pool.Get(slot)
	if val != 255 {
		t.Errorf("expected 255, got %d", val)
	}
}

func TestPoolFreeMultipleTimes(t *testing.T) {
	_, pool := setupPoolTest[int32](t, "freemulti", 4)

	slot, _ := pool.Allocate()

	// Free the same slot twice should not crash
	err := pool.Free(slot)
	if err != nil {
		t.Fatalf("First Free failed: %v", err)
	}
	err = pool.Free(slot)
	if err != nil {
		t.Fatalf("Second Free failed: %v", err)
	}
}
