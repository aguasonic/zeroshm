package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
)

func setupRingTest[T Numeric](t *testing.T, namePrefix string, capacity int) (*Memory, *Ring[T]) {
	t.Helper()
	name := fmt.Sprintf("/ring_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	ring, err := NewRing[T](mem, "ring_"+namePrefix, capacity)
	if err != nil {
		t.Fatalf("NewRing failed: %v", err)
	}
	t.Cleanup(func() { ring.Destroy() })

	return mem, ring
}

func TestRingName(t *testing.T) {
	_, ring := setupRingTest[int32](t, "name", 16)
	if ring.Name() != "ring_name" {
		t.Errorf("expected name 'ring_name', got %q", ring.Name())
	}
}

func TestRingCreateEmpty(t *testing.T) {
	_, ring := setupRingTest[int32](t, "empty", 16)
	if !ring.Empty() {
		t.Error("expected ring to be empty")
	}
	if ring.Size() != 0 {
		t.Errorf("expected size 0, got %d", ring.Size())
	}
	if ring.Capacity() != 16 {
		t.Errorf("expected capacity 16, got %d", ring.Capacity())
	}
	if ring.FreeSpace() != 16 {
		t.Errorf("expected free space 16, got %d", ring.FreeSpace())
	}
}

func TestRingPushAndPop(t *testing.T) {
	_, ring := setupRingTest[int32](t, "pushpop", 16)

	if err := ring.Push(42); err != nil {
		t.Fatalf("Push failed: %v", err)
	}
	if ring.Size() != 1 {
		t.Errorf("expected size 1, got %d", ring.Size())
	}

	val, err := ring.Pop()
	if err != nil {
		t.Fatalf("Pop failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected value 42, got %d", val)
	}
	if ring.Size() != 0 {
		t.Errorf("expected size 0 after pop, got %d", ring.Size())
	}
}

func TestRingPopEmpty(t *testing.T) {
	_, ring := setupRingTest[int32](t, "popempty", 16)

	_, err := ring.Pop()
	if err != RingEmpty {
		t.Errorf("expected RingEmpty, got %v", err)
	}
}

func TestRingPushFull(t *testing.T) {
	_, ring := setupRingTest[int32](t, "pushfull", 4)

	for i := 0; i < 4; i++ {
		if err := ring.Push(int32(i)); err != nil {
			t.Fatalf("Push %d failed: %v", i, err)
		}
	}
	if !ring.Full() {
		t.Error("expected ring to be full")
	}
	if ring.FreeSpace() != 0 {
		t.Errorf("expected free space 0, got %d", ring.FreeSpace())
	}

	if err := ring.Push(10); err != RingFull {
		t.Errorf("expected RingFull, got %v", err)
	}
}

func TestRingPeek(t *testing.T) {
	_, ring := setupRingTest[int32](t, "peek", 16)

	ring.Push(42)
	ring.Push(99)

	val, err := ring.Peek()
	if err != nil {
		t.Fatalf("Peek failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected peek value 42, got %d", val)
	}
	// Peek should not remove the element
	if ring.Size() != 2 {
		t.Errorf("expected size 2 after peek, got %d", ring.Size())
	}

	// Pop should now return 42
	val, err = ring.Pop()
	if err != nil {
		t.Fatalf("Pop failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected pop value 42, got %d", val)
	}
}

func TestRingPeekEmpty(t *testing.T) {
	_, ring := setupRingTest[int32](t, "peekempty", 16)

	_, err := ring.Peek()
	if err != RingEmpty {
		t.Errorf("expected RingEmpty, got %v", err)
	}
}

func TestRingFIFOOrder(t *testing.T) {
	_, ring := setupRingTest[int32](t, "fifo", 16)

	for i := 1; i <= 5; i++ {
		ring.Push(int32(i * 10))
	}

	for i := 1; i <= 5; i++ {
		val, err := ring.Pop()
		if err != nil {
			t.Fatalf("Pop %d failed: %v", i, err)
		}
		if val != int32(i*10) {
			t.Errorf("expected value %d, got %d", i*10, val)
		}
	}
}

func TestRingWrapAround(t *testing.T) {
	_, ring := setupRingTest[int32](t, "wrap", 4)

	// Fill and drain to advance positions
	for i := 0; i < 4; i++ {
		ring.Push(int32(i))
	}
	for i := 0; i < 4; i++ {
		ring.Pop()
	}

	// Now push again -- should wrap around
	ring.Push(100)
	ring.Push(200)

	val, _ := ring.Pop()
	if val != 100 {
		t.Errorf("expected 100, got %d", val)
	}
	val, _ = ring.Pop()
	if val != 200 {
		t.Errorf("expected 200, got %d", val)
	}
}

func TestRingElementSplitAcrossBoundary(t *testing.T) {
	// Use int64 (8 bytes) with small capacity to force split
	_, ring := setupRingTest[int64](t, "split", 3)

	// Push 3 elements (24 bytes capacity)
	ring.Push(int64(1))
	ring.Push(int64(2))
	ring.Push(int64(3))

	// Pop 2 elements (16 bytes consumed, read_pos at 16)
	ring.Pop()
	ring.Pop()

	// Push 2 more -- the 3rd element will wrap around the boundary
	// capacity = 24 bytes, write_pos = 24, read_pos = 16
	// 3rd push: offset = 24 % 24 = 0, writes at 0-8 (contiguous)
	// 4th push: offset = 32 % 24 = 8, writes at 8-16 (contiguous)
	ring.Push(int64(4))
	ring.Push(int64(5))

	// Pop the remaining 3 elements
	val, _ := ring.Pop()
	if val != 3 {
		t.Errorf("expected 3, got %d", val)
	}
	val, _ = ring.Pop()
	if val != 4 {
		t.Errorf("expected 4, got %d", val)
	}
	val, _ = ring.Pop()
	if val != 5 {
		t.Errorf("expected 5, got %d", val)
	}
}

func TestRingReset(t *testing.T) {
	_, ring := setupRingTest[int32](t, "reset", 16)

	for i := 0; i < 5; i++ {
		ring.Push(int32(i))
	}
	if ring.Size() != 5 {
		t.Errorf("expected size 5 before reset, got %d", ring.Size())
	}

	ring.Reset()

	if ring.Size() != 0 {
		t.Errorf("expected size 0 after reset, got %d", ring.Size())
	}
	if !ring.Empty() {
		t.Error("expected ring to be empty after reset")
	}
}

func TestRingOpenCrossProcess(t *testing.T) {
	_, ring := setupRingTest[int32](t, "crossproc", 16)

	ring.Push(100)
	ring.Push(200)

	// Open in a new "process" (same memory, new handle)
	ring2, err := OpenRing[int32](ring.memory, "ring_crossproc")
	if err != nil {
		t.Fatalf("OpenRing failed: %v", err)
	}
	defer ring2.Destroy()

	val, err := ring2.Pop()
	if err != nil {
		t.Fatalf("Pop from opened ring failed: %v", err)
	}
	if val != 100 {
		t.Errorf("expected value 100, got %d", val)
	}

	val, err = ring2.Pop()
	if err != nil {
		t.Fatalf("Pop from opened ring failed: %v", err)
	}
	if val != 200 {
		t.Errorf("expected value 200, got %d", val)
	}

	if ring2.Size() != 0 {
		t.Errorf("expected size 0, got %d", ring2.Size())
	}
}

func TestRingOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/ring_notfound_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenRing[int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for non-existent ring")
	}
}

func TestRingNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/ring_longname_%d", os.Getpid())
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
	_, err = NewRing[int32](mem, longName, 16)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestRingNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/ring_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewRing[int32](mem, "ab", 16)
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestRingNewAlreadyExists(t *testing.T) {
	_, ring := setupRingTest[int32](t, "dup", 16)
	_ = ring

	_, err := NewRing[int32](ring.memory, "ring_dup", 16)
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestRingNewInvalidCapacity(t *testing.T) {
	name := fmt.Sprintf("/ring_invalidcap_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewRing[int32](mem, "ring_invalidcap", 0)
	if err == nil {
		t.Error("expected error for zero capacity")
	}
}

func TestRingOpenTypeMismatch(t *testing.T) {
	_, ring := setupRingTest[int32](t, "typemismatch", 16)
	_ = ring

	// Try to open with different type sizes
	_, err := OpenRing[int64](ring.memory, "ring_typemismatch")
	if err == nil {
		t.Error("expected error for type mismatch")
	}
}

func TestRingSPSC(t *testing.T) {
	_, ring := setupRingTest[int32](t, "spsc", 64)

	var wg sync.WaitGroup

	// Single producer
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < 64; i++ {
			ring.Push(int32(i))
		}
	}()

	// Single consumer
	results := make([]int32, 64)
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < 64; i++ {
			for {
				val, err := ring.Pop()
				if err == RingEmpty {
					continue
				}
				if err != nil {
					t.Errorf("Pop failed: %v", err)
					return
				}
				results[i] = val
				break
			}
		}
	}()

	wg.Wait()

	// Verify FIFO order
	for i := 0; i < 64; i++ {
		if results[i] != int32(i) {
			t.Errorf("expected %d at position %d, got %d", i, i, results[i])
		}
	}
}

func TestRingInt64(t *testing.T) {
	_, ring := setupRingTest[int64](t, "int64", 8)

	ring.Push(int64(1234567890123))
	ring.Push(int64(-9876543210987))

	val, _ := ring.Pop()
	if val != int64(1234567890123) {
		t.Errorf("expected 1234567890123, got %d", val)
	}
	val, _ = ring.Pop()
	if val != int64(-9876543210987) {
		t.Errorf("expected -9876543210987, got %d", val)
	}
}

func TestRingFloat64(t *testing.T) {
	_, ring := setupRingTest[float64](t, "float64", 8)

	ring.Push(3.14159)
	ring.Push(-2.71828)

	val, _ := ring.Pop()
	if val != 3.14159 {
		t.Errorf("expected 3.14159, got %f", val)
	}
	val, _ = ring.Pop()
	if val != -2.71828 {
		t.Errorf("expected -2.71828, got %f", val)
	}
}

func TestRingInt8(t *testing.T) {
	_, ring := setupRingTest[int8](t, "int8", 16)

	ring.Push(int8(-128))
	ring.Push(int8(127))

	val, _ := ring.Pop()
	if val != int8(-128) {
		t.Errorf("expected -128, got %d", val)
	}
	val, _ = ring.Pop()
	if val != int8(127) {
		t.Errorf("expected 127, got %d", val)
	}
}

func TestRingDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/ring_destroy_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	ring, err := NewRing[int32](mem, "ring_destroy", 16)
	if err != nil {
		t.Fatalf("NewRing failed: %v", err)
	}
	ring.Push(100)

	ring.Destroy()

	// Name should be reusable
	ring2, err := NewRing[int32](mem, "ring_destroy", 16)
	if err != nil {
		t.Fatalf("NewRing after destroy failed: %v", err)
	}
	defer ring2.Destroy()

	if ring2.Size() != 0 {
		t.Errorf("expected size 0 in new ring, got %d", ring2.Size())
	}
}

func TestRingCapacityRounding(t *testing.T) {
	// Capacity of 5 elements with int32 (4 bytes) should round to 5*4=20 bytes = 5 elements
	_, ring := setupRingTest[int32](t, "rounding", 5)
	if ring.Capacity() != 5 {
		t.Errorf("expected capacity 5, got %d", ring.Capacity())
	}

	// Capacity of 1 element should work
	_, ring1 := setupRingTest[int32](t, "one", 1)
	if ring1.Capacity() != 1 {
		t.Errorf("expected capacity 1, got %d", ring1.Capacity())
	}
}
