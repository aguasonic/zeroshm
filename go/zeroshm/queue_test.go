package zeroshm

import (
	"fmt"
	"os"
	"testing"
)

// TestQueueName tests the Name method.
func TestQueueName(t *testing.T) {
	name := fmt.Sprintf("/test_queue_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	q, err := NewQueue[int32](mem, "my_queue", 10)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}

	if q.Name() != "my_queue" {
		t.Errorf("Name() = %q, want my_queue", q.Name())
	}
}

// TestQueueTryPushTryPop tests the TryPush and TryPop aliases.
func TestQueueTryPushTryPop(t *testing.T) {
	name := fmt.Sprintf("/test_queue_try_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	q, err := NewQueue[int64](mem, "try_queue", 5)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}

	// TryPush
	if !q.TryPush(42) {
		t.Error("TryPush should succeed on empty queue")
	}

	// TryPop
	val, ok := q.TryPop()
	if !ok {
		t.Error("TryPop should succeed")
	}
	if val != 42 {
		t.Errorf("TryPop returned %d, want 42", val)
	}

	// TryPop on empty queue should fail
	_, ok = q.TryPop()
	if ok {
		t.Error("TryPop on empty queue should fail")
	}

	// TryPush on full queue should fail
	for i := 0; i < q.Capacity(); i++ {
		q.TryPush(int64(i))
	}
	if q.TryPush(99) {
		t.Error("TryPush on full queue should fail")
	}
}

// TestQueueSize tests the Size method.
func TestQueueSize(t *testing.T) {
	name := fmt.Sprintf("/test_queue_size_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	q, err := NewQueue[int32](mem, "size_queue", 10)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}

	if q.Size() != 0 {
		t.Errorf("Initial Size() = %d, want 0", q.Size())
	}

	q.Push(1)
	if q.Size() != 1 {
		t.Errorf("Size after 1 push = %d, want 1", q.Size())
	}

	q.Push(2)
	q.Push(3)
	if q.Size() != 3 {
		t.Errorf("Size after 3 pushes = %d, want 3", q.Size())
	}

	q.Pop()
	if q.Size() != 2 {
		t.Errorf("Size after 1 pop = %d, want 2", q.Size())
	}
}

// TestQueueNewNameTooLong tests NewQueue with name too long.
func TestQueueNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_queue_longname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	longName := ""
	for i := 0; i < 255; i++ {
		longName += "a"
	}

	_, err = NewQueue[int32](mem, longName, 10)
	if err == nil {
		t.Error("NewQueue with name too long should fail")
	}
}

// TestQueueNewCapacityZero tests NewQueue with zero capacity.
func TestQueueNewCapacityZero(t *testing.T) {
	name := fmt.Sprintf("/test_queue_zero_cap_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewQueue[int32](mem, "zero_cap", 0)
	if err == nil {
		t.Error("NewQueue with zero capacity should fail")
	}

	_, err = NewQueue[int32](mem, "neg_cap", -1)
	if err == nil {
		t.Error("NewQueue with negative capacity should fail")
	}
}

// TestQueueOpenNotExists tests OpenQueue with non-existent queue.
func TestQueueOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_queue_open_notexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenQueue[int32](mem, "nonexistent")
	if err == nil {
		t.Error("OpenQueue with non-existent name should fail")
	}
}

// TestQueueOpenSizeMismatch tests OpenQueue with wrong element type.
func TestQueueOpenSizeMismatch(t *testing.T) {
	name := fmt.Sprintf("/test_queue_size_mismatch_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewQueue[int32](mem, "mismatch", 10); err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}

	_, err = OpenQueue[int64](mem, "mismatch")
	if err == nil {
		t.Error("OpenQueue with wrong element size should fail")
	}
}

// TestQueueNewAlreadyExists tests NewQueue when name already exists.
func TestQueueNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_queue_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewQueue[int32](mem, "dup_queue", 10); err != nil {
		t.Fatalf("First NewQueue failed: %v", err)
	}

	_, err = NewQueue[int32](mem, "dup_queue", 10)
	if err == nil {
		t.Error("NewQueue with existing name should fail")
	}
}

// TestQueueOpenCrossProcess tests OpenQueue seeing data from creator.
func TestQueueOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_queue_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	q1, err := NewQueue[float64](mem, "cross_queue", 10)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}

	q1.Push(1.5)
	q1.Push(2.5)
	q1.Push(3.5)

	// Open the same queue
	q2, err := OpenQueue[float64](mem, "cross_queue")
	if err != nil {
		t.Fatalf("OpenQueue failed: %v", err)
	}

	// Verify data is visible
	val, ok := q2.Pop()
	if !ok || val != 1.5 {
		t.Errorf("q2.Pop() = %f, %v; want 1.5, true", val, ok)
	}

	// Push through q2, verify q1 sees it
	q2.Push(99.9)
	val, ok = q1.Pop()
	if !ok || val != 2.5 {
		t.Errorf("q1.Pop() = %f, %v; want 2.5, true", val, ok)
	}
	val, ok = q1.Pop()
	if !ok || val != 3.5 {
		t.Errorf("q1.Pop() = %f, %v; want 3.5, true", val, ok)
	}
	val, ok = q1.Pop()
	if !ok || val != 99.9 {
		t.Errorf("q1.Pop() = %f, %v; want 99.9, true", val, ok)
	}
}

// TestQueueCapacityRounding tests that capacity is rounded up to power of two.
func TestQueueCapacityRounding(t *testing.T) {
	name := fmt.Sprintf("/test_queue_rounding_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Request 10, should round up to 16
	q, err := NewQueue[int32](mem, "rounding_queue", 10)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}
	if q.Capacity() != 16 {
		t.Errorf("Capacity = %d, want 16", q.Capacity())
	}

	// Request 17, should round up to 32
	q2, err := NewQueue[int32](mem, "rounding_queue2", 17)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}
	if q2.Capacity() != 32 {
		t.Errorf("Capacity = %d, want 32", q2.Capacity())
	}

	// Request 32, should stay 32
	q3, err := NewQueue[int32](mem, "rounding_queue3", 32)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}
	if q3.Capacity() != 32 {
		t.Errorf("Capacity = %d, want 32", q3.Capacity())
	}
}

// TestQueueFIFOOrder tests FIFO ordering.
func TestQueueFIFOOrder(t *testing.T) {
	name := fmt.Sprintf("/test_queue_fifo_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	q, err := NewQueue[int32](mem, "fifo_queue", 10)
	if err != nil {
		t.Fatalf("NewQueue failed: %v", err)
	}

	// Push 1, 2, 3, 4, 5
	for i := int32(1); i <= 5; i++ {
		q.Push(i)
	}

	// Pop should return 1, 2, 3, 4, 5 in order
	for i := int32(1); i <= 5; i++ {
		val, ok := q.Pop()
		if !ok {
			t.Fatalf("Pop %d failed", i)
		}
		if val != i {
			t.Errorf("Pop returned %d, want %d", val, i)
		}
	}
}
