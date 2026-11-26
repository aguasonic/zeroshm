package zeroshm

import (
	"fmt"
	"os"
	"testing"
)

// TestStackName tests the Name method.
func TestStackName(t *testing.T) {
	name := fmt.Sprintf("/test_stack_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStack[int32](mem, "my_stack", 10)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}

	if s.Name() != "my_stack" {
		t.Errorf("Name() = %q, want my_stack", s.Name())
	}
}

// TestStackTop tests the Top method.
func TestStackTop(t *testing.T) {
	name := fmt.Sprintf("/test_stack_top_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStack[int64](mem, "top_stack", 5)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}

	// Top on empty stack should fail
	_, ok := s.Top()
	if ok {
		t.Error("Top on empty stack should fail")
	}

	s.Push(42)
	val, ok := s.Top()
	if !ok {
		t.Error("Top should succeed")
	}
	if val != 42 {
		t.Errorf("Top returned %d, want 42", val)
	}

	// Top should not modify the stack
	if s.Size() != 1 {
		t.Errorf("Size after Top = %d, want 1", s.Size())
	}

	s.Push(99)
	val, ok = s.Top()
	if !ok || val != 99 {
		t.Errorf("Top returned %d, %v; want 99, true", val, ok)
	}

	// Pop should return 99 (the top)
	val, ok = s.Pop()
	if !ok || val != 99 {
		t.Errorf("Pop returned %d, %v; want 99, true", val, ok)
	}

	// Top should now return 42
	val, ok = s.Top()
	if !ok || val != 42 {
		t.Errorf("Top returned %d, %v; want 42, true", val, ok)
	}
}

// TestStackSize tests the Size method.
func TestStackSize(t *testing.T) {
	name := fmt.Sprintf("/test_stack_size_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStack[int32](mem, "size_stack", 10)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}

	if s.Size() != 0 {
		t.Errorf("Initial Size() = %d, want 0", s.Size())
	}

	s.Push(1)
	if s.Size() != 1 {
		t.Errorf("Size after 1 push = %d, want 1", s.Size())
	}

	s.Push(2)
	s.Push(3)
	if s.Size() != 3 {
		t.Errorf("Size after 3 pushes = %d, want 3", s.Size())
	}

	s.Pop()
	if s.Size() != 2 {
		t.Errorf("Size after 1 pop = %d, want 2", s.Size())
	}
}

// TestStackNewNameTooLong tests NewStack with name too long.
func TestStackNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_stack_longname_%d", os.Getpid())
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

	_, err = NewStack[int32](mem, longName, 10)
	if err == nil {
		t.Error("NewStack with name too long should fail")
	}
}

// TestStackNewCapacityZero tests NewStack with zero capacity.
func TestStackNewCapacityZero(t *testing.T) {
	name := fmt.Sprintf("/test_stack_zero_cap_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewStack[int32](mem, "zero_cap", 0)
	if err == nil {
		t.Error("NewStack with zero capacity should fail")
	}

	_, err = NewStack[int32](mem, "neg_cap", -1)
	if err == nil {
		t.Error("NewStack with negative capacity should fail")
	}
}

// TestStackOpenNotExists tests OpenStack with non-existent stack.
func TestStackOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_stack_open_notexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenStack[int32](mem, "nonexistent")
	if err == nil {
		t.Error("OpenStack with non-existent name should fail")
	}
}

// TestStackOpenSizeMismatch tests OpenStack with wrong element type.
func TestStackOpenSizeMismatch(t *testing.T) {
	name := fmt.Sprintf("/test_stack_size_mismatch_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewStack[int32](mem, "mismatch", 10); err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}

	_, err = OpenStack[int64](mem, "mismatch")
	if err == nil {
		t.Error("OpenStack with wrong element size should fail")
	}
}

// TestStackNewAlreadyExists tests NewStack when name already exists.
func TestStackNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_stack_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewStack[int32](mem, "dup_stack", 10); err != nil {
		t.Fatalf("First NewStack failed: %v", err)
	}

	_, err = NewStack[int32](mem, "dup_stack", 10)
	if err == nil {
		t.Error("NewStack with existing name should fail")
	}
}

// TestStackOpenCrossProcess tests OpenStack seeing data from creator.
func TestStackOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_stack_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s1, err := NewStack[float64](mem, "cross_stack", 10)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}

	s1.Push(1.5)
	s1.Push(2.5)
	s1.Push(3.5)

	// Open the same stack
	s2, err := OpenStack[float64](mem, "cross_stack")
	if err != nil {
		t.Fatalf("OpenStack failed: %v", err)
	}

	// Verify data is visible (LIFO: 3.5 was pushed last, so it pops first)
	val, ok := s2.Pop()
	if !ok || val != 3.5 {
		t.Errorf("s2.Pop() = %f, %v; want 3.5, true", val, ok)
	}

	// Push through s2, verify s1 sees it
	s2.Push(99.9)
	val, ok = s1.Pop()
	if !ok || val != 99.9 {
		t.Errorf("s1.Pop() = %f, %v; want 99.9, true", val, ok)
	}
	val, ok = s1.Pop()
	if !ok || val != 2.5 {
		t.Errorf("s1.Pop() = %f, %v; want 2.5, true", val, ok)
	}
	val, ok = s1.Pop()
	if !ok || val != 1.5 {
		t.Errorf("s1.Pop() = %f, %v; want 1.5, true", val, ok)
	}
}

// TestStackCapacityExact tests that capacity is used as-is (no rounding).
func TestStackCapacityExact(t *testing.T) {
	name := fmt.Sprintf("/test_stack_capacity_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Request 10, should stay 10 (Stack doesn't round up like Queue)
	s, err := NewStack[int32](mem, "cap_stack", 10)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}
	if s.Capacity() != 10 {
		t.Errorf("Capacity = %d, want 10", s.Capacity())
	}

	// Request 17, should stay 17
	s2, err := NewStack[int32](mem, "cap_stack2", 17)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}
	if s2.Capacity() != 17 {
		t.Errorf("Capacity = %d, want 17", s2.Capacity())
	}

	// Request 32, should stay 32
	s3, err := NewStack[int32](mem, "cap_stack3", 32)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}
	if s3.Capacity() != 32 {
		t.Errorf("Capacity = %d, want 32", s3.Capacity())
	}
}

// TestStackLIFOOrder tests LIFO ordering.
func TestStackLIFOOrder(t *testing.T) {
	name := fmt.Sprintf("/test_stack_lifo_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStack[int32](mem, "lifo_stack", 10)
	if err != nil {
		t.Fatalf("NewStack failed: %v", err)
	}

	// Push 1, 2, 3, 4, 5
	for i := int32(1); i <= 5; i++ {
		s.Push(i)
	}

	// Pop should return 5, 4, 3, 2, 1 (LIFO)
	for i := int32(5); i >= 1; i-- {
		val, ok := s.Pop()
		if !ok {
			t.Fatalf("Pop %d failed", i)
		}
		if val != i {
			t.Errorf("Pop returned %d, want %d", val, i)
		}
	}
}
