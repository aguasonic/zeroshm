package zeroshm

import (
	"fmt"
	"os"
	"testing"
)

// TestArrayName tests the Name method.
func TestArrayName(t *testing.T) {
	name := fmt.Sprintf("/test_array_name_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr, err := NewArray[int32](mem, "my_array", 10)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	if arr.Name() != "my_array" {
		t.Errorf("Name() = %q, want my_array", arr.Name())
	}
}

// TestArrayFill tests the Fill method.
func TestArrayFill(t *testing.T) {
	name := fmt.Sprintf("/test_array_fill_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr, err := NewArray[float64](mem, "fill_array", 5)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	// Fill with a value
	arr.Fill(3.14)

	// Verify all elements are filled
	for i := 0; i < arr.Capacity(); i++ {
		v := arr.Get(i)
		if v != 3.14 {
			t.Errorf("arr.Get(%d) = %f, want 3.14", i, v)
		}
	}
}

// TestArraySlice tests the Slice method.
func TestArraySlice(t *testing.T) {
	name := fmt.Sprintf("/test_array_slice_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr, err := NewArray[int64](mem, "slice_array", 4)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	// Set values
	arr.Set(0, 10)
	arr.Set(1, 20)
	arr.Set(2, 30)
	arr.Set(3, 40)

	// Get slice
	slice := arr.Slice()
	if len(slice) != 4 {
		t.Errorf("len(Slice()) = %d, want 4", len(slice))
	}
	expected := []int64{10, 20, 30, 40}
	for i, v := range slice {
		if v != expected[i] {
			t.Errorf("Slice()[%d] = %d, want %d", i, v, expected[i])
		}
	}

	// Verify it's a copy (modifying slice doesn't affect array)
	slice[0] = 999
	if arr.Get(0) != 10 {
		t.Errorf("Slice should be a copy: arr.Get(0) = %d after modifying slice", arr.Get(0))
	}
}

// TestArrayCopyFrom tests the CopyFrom method.
func TestArrayCopyFrom(t *testing.T) {
	name := fmt.Sprintf("/test_array_copyfrom_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr, err := NewArray[uint32](mem, "copy_array", 5)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	// Copy from a slice
	data := []uint32{1, 2, 3, 4, 5}
	arr.CopyFrom(data)

	// Verify values
	for i, v := range data {
		got := arr.Get(i)
		if got != v {
			t.Errorf("arr.Get(%d) = %d, want %d", i, got, v)
		}
	}

	// Copy from a smaller slice (should not error)
	data2 := []uint32{10, 20}
	arr.CopyFrom(data2)
	if arr.Get(0) != 10 {
		t.Errorf("arr.Get(0) = %d, want 10", arr.Get(0))
	}
	if arr.Get(1) != 20 {
		t.Errorf("arr.Get(1) = %d, want 20", arr.Get(1))
	}

	// Copy from a larger slice (should truncate to capacity)
	data3 := []uint32{100, 200, 300, 400, 500, 600, 700}
	arr.CopyFrom(data3)
	for i, v := range data3[:5] {
		got := arr.Get(i)
		if got != v {
			t.Errorf("arr.Get(%d) = %d, want %d (truncated)", i, got, v)
		}
	}
}

// TestArrayRange tests the Range method.
func TestArrayRange(t *testing.T) {
	name := fmt.Sprintf("/test_array_range_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr, err := NewArray[int32](mem, "range_array", 4)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	arr.Set(0, 10)
	arr.Set(1, 20)
	arr.Set(2, 30)
	arr.Set(3, 40)

	// Test full iteration
	var sum int32
	count := 0
	arr.Range(func(index int, value int32) bool {
		sum += value
		count++
		return true
	})
	if sum != 100 {
		t.Errorf("sum = %d, want 100", sum)
	}
	if count != 4 {
		t.Errorf("count = %d, want 4", count)
	}

	// Test early termination
	count = 0
	arr.Range(func(index int, value int32) bool {
		count++
		return value != 20 // stop after 20
	})
	if count != 2 {
		t.Errorf("early termination count = %d, want 2", count)
	}
}

// TestArrayGetSetBounds tests Get/Set bounds checking.
func TestArrayGetSetBounds(t *testing.T) {
	name := fmt.Sprintf("/test_array_bounds_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr, err := NewArray[int32](mem, "bounds_array", 3)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	// Valid access
	arr.Set(0, 1)
	arr.Set(1, 2)
	arr.Set(2, 3)
	if arr.Get(0) != 1 || arr.Get(1) != 2 || arr.Get(2) != 3 {
		t.Error("Valid Get/Set failed")
	}

	// Invalid index should panic
	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Get(-1) should panic")
			}
		}()
		arr.Get(-1)
	}()

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Get(3) should panic")
			}
		}()
		arr.Get(3)
	}()

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Set(-1) should panic")
			}
		}()
		arr.Set(-1, 99)
	}()

	func() {
		defer func() {
			if r := recover(); r == nil {
				t.Error("Set(3) should panic")
			}
		}()
		arr.Set(3, 99)
	}()
}

// TestArrayNewNameTooLong tests NewArray with name too long.
func TestArrayNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_array_longname_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	// NameSize is 255, so a 255-char name should fail
	longName := ""
	for i := 0; i < 255; i++ {
		longName += "a"
	}

	_, err = NewArray[int32](mem, longName, 10)
	if err == nil {
		t.Error("NewArray with name too long should fail")
	}
}

// TestArrayNewCapacityZero tests NewArray with zero capacity.
func TestArrayNewCapacityZero(t *testing.T) {
	name := fmt.Sprintf("/test_array_zero_cap_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	_, err = NewArray[int32](mem, "zero_cap", 0)
	if err == nil {
		t.Error("NewArray with zero capacity should fail")
	}

	_, err = NewArray[int32](mem, "neg_cap", -1)
	if err == nil {
		t.Error("NewArray with negative capacity should fail")
	}
}

// TestArrayOpenNotExists tests OpenArray with non-existent array.
func TestArrayOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_array_open_notexist_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	_, err = OpenArray[int32](mem, "nonexistent")
	if err == nil {
		t.Error("OpenArray with non-existent name should fail")
	}
}

// TestArrayOpenSizeMismatch tests OpenArray with wrong element type.
func TestArrayOpenSizeMismatch(t *testing.T) {
	name := fmt.Sprintf("/test_array_size_mismatch_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	// Create as int32
	if _, err := NewArray[int32](mem, "mismatch", 10); err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	// Try to open as int64 (different size)
	_, err = OpenArray[int64](mem, "mismatch")
	if err == nil {
		t.Error("OpenArray with wrong element size should fail")
	}
}

// TestArrayNewAlreadyExists tests NewArray when name already exists.
func TestArrayNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_array_exists_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	if _, err := NewArray[int32](mem, "dup_array", 10); err != nil {
		t.Fatalf("First NewArray failed: %v", err)
	}

	_, err = NewArray[int32](mem, "dup_array", 10)
	if err == nil {
		t.Error("NewArray with existing name should fail")
	}
}

// TestArrayOpenCrossProcess tests OpenArray seeing data from creator.
func TestArrayOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_array_cross_%d", os.Getpid())
	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()
	defer UnlinkName(name)

	arr1, err := NewArray[float32](mem, "cross_array", 5)
	if err != nil {
		t.Fatalf("NewArray failed: %v", err)
	}

	arr1.Set(0, 1.5)
	arr1.Set(1, 2.5)
	arr1.Set(2, 3.5)

	// Open the same array
	arr2, err := OpenArray[float32](mem, "cross_array")
	if err != nil {
		t.Fatalf("OpenArray failed: %v", err)
	}

	// Verify data is visible
	if arr2.Get(0) != 1.5 {
		t.Errorf("arr2.Get(0) = %f, want 1.5", arr2.Get(0))
	}
	if arr2.Get(1) != 2.5 {
		t.Errorf("arr2.Get(1) = %f, want 2.5", arr2.Get(1))
	}
	if arr2.Get(2) != 3.5 {
		t.Errorf("arr2.Get(2) = %f, want 3.5", arr2.Get(2))
	}

	// Modify through arr2, verify arr1 sees it
	arr2.Set(0, 99.9)
	if arr1.Get(0) != 99.9 {
		t.Errorf("arr1.Get(0) = %f after arr2 modification, want 99.9", arr1.Get(0))
	}
}
