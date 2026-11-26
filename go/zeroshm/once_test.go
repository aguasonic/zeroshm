package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// TestOnceName tests the Name method.
func TestOnceName(t *testing.T) {
	name := fmt.Sprintf("/test_once_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o, err := NewOnce(mem, "my_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	if o.Name() != "my_once" {
		t.Errorf("Name() = %q, want my_once", o.Name())
	}
}

// TestOnceIsCalled tests the IsCalled method.
func TestOnceIsCalled(t *testing.T) {
	name := fmt.Sprintf("/test_once_iscalled_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o, err := NewOnce(mem, "iscalled_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	// Should not be called yet
	if o.IsCalled() {
		t.Error("IsCalled() should be false initially")
	}

	// Execute once
	o.Do(func() {})

	// Should be called now
	if !o.IsCalled() {
		t.Error("IsCalled() should be true after Do()")
	}
}

// TestOnceCall tests the Call method (alias for Do).
func TestOnceCall(t *testing.T) {
	name := fmt.Sprintf("/test_once_call_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o, err := NewOnce(mem, "call_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	callCount := 0
	o.Call(func() {
		callCount++
	})

	if callCount != 1 {
		t.Errorf("Call() executed fn %d times, want 1", callCount)
	}

	// Second Call should not execute
	o.Call(func() {
		callCount++
	})

	if callCount != 1 {
		t.Errorf("Second Call() should not execute fn, count = %d, want 1", callCount)
	}
}

// TestOnceAlreadyCalled tests the AlreadyCalled method (alias for IsCalled).
func TestOnceAlreadyCalled(t *testing.T) {
	name := fmt.Sprintf("/test_once_already_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o, err := NewOnce(mem, "already_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	if o.AlreadyCalled() {
		t.Error("AlreadyCalled() should be false initially")
	}

	o.Do(func() {})

	if !o.AlreadyCalled() {
		t.Error("AlreadyCalled() should be true after Do()")
	}
}

// TestOnceResetUnsafe tests the ResetUnsafe method.
func TestOnceResetUnsafe(t *testing.T) {
	name := fmt.Sprintf("/test_once_reset_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o, err := NewOnce(mem, "reset_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	callCount := 0
	o.Do(func() {
		callCount++
	})

	if callCount != 1 {
		t.Errorf("First Do() executed fn %d times, want 1", callCount)
	}

	if !o.IsCalled() {
		t.Error("IsCalled() should be true after Do()")
	}

	// Reset and execute again
	o.ResetUnsafe()

	if o.IsCalled() {
		t.Error("IsCalled() should be false after ResetUnsafe()")
	}

	o.Do(func() {
		callCount++
	})

	if callCount != 2 {
		t.Errorf("After reset, Do() should execute fn again, count = %d, want 2", callCount)
	}
}

// TestOnceNewNameTooLong tests NewOnce with name too long.
func TestOnceNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_once_longname_%d", os.Getpid())
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

	_, err = NewOnce(mem, longName)
	if err == nil {
		t.Error("NewOnce with name too long should fail")
	}
}

// TestOnceNewAlreadyExists tests NewOnce when name already exists.
func TestOnceNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_once_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewOnce(mem, "dup_once"); err != nil {
		t.Fatalf("First NewOnce failed: %v", err)
	}

	_, err = NewOnce(mem, "dup_once")
	if err == nil {
		t.Error("NewOnce with existing name should fail")
	}
}

// TestOnceOpenNotExists tests OpenOnce with non-existent Once flag.
func TestOnceOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_o_open_nexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenOnce(mem, "nonexistent")
	if err == nil {
		t.Error("OpenOnce with non-existent name should fail")
	}
}

// TestOnceOpenCrossProcess tests OpenOnce seeing state from creator.
func TestOnceOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_once_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o1, err := NewOnce(mem, "cross_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	// Open the same Once flag
	o2, err := OpenOnce(mem, "cross_once")
	if err != nil {
		t.Fatalf("OpenOnce failed: %v", err)
	}

	// Both should see uncalled state initially
	if o1.IsCalled() || o2.IsCalled() {
		t.Error("Both should see uncalled state initially")
	}

	// Execute via o1
	count1 := 0
	count2 := 0
	o1.Do(func() {
		count1++
	})

	// Verify state is visible to o2
	if !o2.IsCalled() {
		t.Error("o2.IsCalled() should be true after o1.Do()")
	}

	// Execute via o2 should NOT run fn again
	o2.Do(func() {
		count2++
	})

	if count1 != 1 {
		t.Errorf("count1 = %d, want 1", count1)
	}
	if count2 != 0 {
		t.Errorf("count2 = %d, want 0 (fn should not execute again)", count2)
	}
}

// TestOnceDoPanic tests that Do() marks done even if fn panics.
func TestOnceDoPanic(t *testing.T) {
	name := fmt.Sprintf("/test_once_panic_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o, err := NewOnce(mem, "panic_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	// First Do should panic and mark as done
	panicked := false
	func() {
		defer func() {
			if r := recover(); r != nil {
				panicked = true
			}
		}()
		o.Do(func() {
			panic("test panic")
		})
	}()

	if !panicked {
		t.Error("First Do() should panic")
	}

	// After panic, Once should be marked as done (matching std::call_once semantics)
	if !o.IsCalled() {
		t.Error("Once should be marked done after panic")
	}

	// Second Do should NOT panic and NOT execute fn
	callCount := 0
	func() {
		defer func() {
			if r := recover(); r != nil {
				t.Errorf("Second Do() should not panic: %v", r)
			}
		}()
		o.Do(func() {
			callCount++
		})
	}()

	if callCount != 0 {
		t.Errorf("Second Do() should not execute fn, count = %d, want 0", callCount)
	}
}

// TestOnceConcurrentCrossProcess tests concurrent Do from multiple OpenOnce instances.
func TestOnceConcurrentCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_once_conc_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	o1, err := NewOnce(mem, "conc_cross_once")
	if err != nil {
		t.Fatalf("NewOnce failed: %v", err)
	}

	o2, err := OpenOnce(mem, "conc_cross_once")
	if err != nil {
		t.Fatalf("OpenOnce failed: %v", err)
	}

	var callCount int64
	var wg sync.WaitGroup

	// Multiple goroutines using both o1 and o2
	for i := 0; i < 4; i++ {
		wg.Add(2)
		go func() {
			defer wg.Done()
			o1.Do(func() {
				atomic.AddInt64(&callCount, 1)
				time.Sleep(5 * time.Millisecond)
			})
		}()
		go func() {
			defer wg.Done()
			o2.Do(func() {
				atomic.AddInt64(&callCount, 1)
				time.Sleep(5 * time.Millisecond)
			})
		}()
	}

	wg.Wait()

	if callCount != 1 {
		t.Errorf("Function executed %d times, want 1", callCount)
	}
}
