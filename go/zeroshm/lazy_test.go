package zeroshm

import (
	"errors"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func setupLazyTest[T Numeric](t *testing.T, namePrefix string) (*Memory, *Lazy[T]) {
	t.Helper()
	name := fmt.Sprintf("/lazy_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	lazy, err := NewLazy[T](mem, "lazy_"+namePrefix)
	if err != nil {
		t.Fatalf("NewLazy failed: %v", err)
	}
	t.Cleanup(func() { lazy.Destroy() })

	return mem, lazy
}

func TestLazyName(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "name")
	if lazy.Name() != "lazy_name" {
		t.Errorf("expected name 'lazy_name', got %q", lazy.Name())
	}
}

func TestLazyInitialState(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "initial")
	if lazy.State() != LazyStateNotComputed {
		t.Errorf("expected NOT_COMPUTED state, got %d", lazy.State())
	}
	if lazy.IsComputed() {
		t.Error("expected lazy to not be computed")
	}
	if lazy.ComputeCount() != 0 {
		t.Errorf("expected compute count 0, got %d", lazy.ComputeCount())
	}
}

func TestLazyForceCompute(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "force")

	val, err := lazy.Force(func() (int32, error) {
		return 42, nil
	})
	if err != nil {
		t.Fatalf("Force failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected 42, got %d", val)
	}
	if lazy.State() != LazyStateComputed {
		t.Errorf("expected COMPUTED state, got %d", lazy.State())
	}
	if !lazy.IsComputed() {
		t.Error("expected lazy to be computed")
	}
	if lazy.ComputeCount() != 1 {
		t.Errorf("expected compute count 1, got %d", lazy.ComputeCount())
	}
}

func TestLazyForceOnce(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "once")

	var computeCalls int32

	val, err := lazy.Force(func() (int32, error) {
		atomic.AddInt32(&computeCalls, 1)
		return 42, nil
	})
	if err != nil {
		t.Fatalf("First Force failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected 42, got %d", val)
	}

	// Second Force should not recompute
	val, err = lazy.Force(func() (int32, error) {
		atomic.AddInt32(&computeCalls, 1)
		return 99, nil // This should NOT be used
	})
	if err != nil {
		t.Fatalf("Second Force failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected 42 (cached), got %d", val)
	}
	if computeCalls != 1 {
		t.Errorf("expected 1 compute call, got %d", computeCalls)
	}
}

func TestLazyForceError(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "error")

	expectedErr := errors.New("computation failed")
	val, err := lazy.Force(func() (int32, error) {
		return 0, expectedErr
	})
	if err == nil {
		t.Fatal("expected error from Force")
	}
	if lazy.State() != LazyStateError {
		t.Errorf("expected ERROR state, got %d", lazy.State())
	}

	// Subsequent Force should return the stored error
	val, err = lazy.Force(func() (int32, error) {
		return 99, nil
	})
	if err == nil {
		t.Fatal("expected error from second Force")
	}
	_ = val
}

func TestLazyGet(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "get")

	var wg sync.WaitGroup

	// Force in a goroutine
	wg.Add(1)
	go func() {
		defer wg.Done()
		time.Sleep(10 * time.Millisecond)
		lazy.Force(func() (int32, error) {
			return 77, nil
		})
	}()

	// Get blocks until computed
	val, err := lazy.Get()
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 77 {
		t.Errorf("expected 77, got %d", val)
	}

	wg.Wait()
}

func TestLazyPeekNotComputed(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "peeknotcomp")

	_, err := lazy.Peek()
	if err != LazyNotReady {
		t.Errorf("expected LazyNotReady, got %v", err)
	}
}

func TestLazyPeekComputed(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "peekcomp")

	lazy.Force(func() (int32, error) {
		return 55, nil
	})

	val, err := lazy.Peek()
	if err != nil {
		t.Fatalf("Peek failed: %v", err)
	}
	if val != 55 {
		t.Errorf("expected 55, got %d", val)
	}
}

func TestLazyReset(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "reset")

	lazy.Force(func() (int32, error) {
		return 42, nil
	})

	if !lazy.IsComputed() {
		t.Fatal("expected lazy to be computed")
	}

	if !lazy.Reset() {
		t.Error("expected Reset to succeed")
	}
	if lazy.State() != LazyStateNotComputed {
		t.Errorf("expected NOT_COMPUTED after reset, got %d", lazy.State())
	}

	// Recompute with new value
	val, err := lazy.Force(func() (int32, error) {
		return 99, nil
	})
	if err != nil {
		t.Fatalf("Force after reset failed: %v", err)
	}
	if val != 99 {
		t.Errorf("expected 99 after recompute, got %d", val)
	}
	if lazy.ComputeCount() != 2 {
		t.Errorf("expected compute count 2, got %d", lazy.ComputeCount())
	}
}

func TestLazyConcurrentForce(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "concurrent")

	var computeCalls int32
	results := make([]int32, 8)
	var wg sync.WaitGroup

	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			val, err := lazy.Force(func() (int32, error) {
				atomic.AddInt32(&computeCalls, 1)
				time.Sleep(time.Millisecond) // Simulate expensive computation
				return int32(100 + i), nil
			})
			if err != nil {
				t.Errorf("Force failed: %v", err)
				return
			}
			results[i] = val
		}(i)
	}

	wg.Wait()

	// Only one goroutine should have computed
	if computeCalls != 1 {
		t.Errorf("expected 1 compute call, got %d", computeCalls)
	}

	// All should get the same (first) value
	for i, val := range results {
		if val == 0 {
			t.Errorf("result %d is zero", i)
		}
	}
}

func TestLazyOpenCrossProcess(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "crossproc")

	lazy.Force(func() (int32, error) {
		return 123, nil
	})

	lazy2, err := OpenLazy[int32](lazy.memory, "lazy_crossproc")
	if err != nil {
		t.Fatalf("OpenLazy failed: %v", err)
	}
	defer lazy2.Destroy()

	val, err := lazy2.Peek()
	if err != nil {
		t.Fatalf("Peek from opened lazy failed: %v", err)
	}
	if val != 123 {
		t.Errorf("expected 123, got %d", val)
	}
}

func TestLazyOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/lazy_notfound_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenLazy[int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for non-existent lazy")
	}
}

func TestLazyNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/lazy_longname_%d", os.Getpid())
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
	_, err = NewLazy[int32](mem, longName)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestLazyNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/lazy_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewLazy[int32](mem, "ab")
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestLazyNewAlreadyExists(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "dup")
	_ = lazy

	_, err := NewLazy[int32](lazy.memory, "lazy_dup")
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestLazyOpenTypeMismatch(t *testing.T) {
	_, lazy := setupLazyTest[int32](t, "typemismatch")
	_ = lazy

	_, err := OpenLazy[int64](lazy.memory, "lazy_typemismatch")
	if err == nil {
		t.Error("expected error for type mismatch")
	}
}

func TestLazyInt64(t *testing.T) {
	_, lazy := setupLazyTest[int64](t, "int64")

	val, err := lazy.Force(func() (int64, error) {
		return int64(1234567890123), nil
	})
	if err != nil {
		t.Fatalf("Force failed: %v", err)
	}
	if val != int64(1234567890123) {
		t.Errorf("expected 1234567890123, got %d", val)
	}
}

func TestLazyFloat64(t *testing.T) {
	_, lazy := setupLazyTest[float64](t, "float64")

	val, err := lazy.Force(func() (float64, error) {
		return 3.14159, nil
	})
	if err != nil {
		t.Fatalf("Force failed: %v", err)
	}
	if val < 3.14158 || val > 3.14160 {
		t.Errorf("expected ~3.14159, got %f", val)
	}
}

func TestLazyWithInitialValue(t *testing.T) {
	name := fmt.Sprintf("/lazy_initval_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	lazy, err := NewLazyWithInitialValue[int32](mem, "lazy_initval", 777)
	if err != nil {
		t.Fatalf("NewLazyWithInitialValue failed: %v", err)
	}
	defer lazy.Destroy()

	if !lazy.IsComputed() {
		t.Error("expected lazy to be computed")
	}
	if lazy.ComputeCount() != 1 {
		t.Errorf("expected compute count 1, got %d", lazy.ComputeCount())
	}

	val, err := lazy.Peek()
	if err != nil {
		t.Fatalf("Peek failed: %v", err)
	}
	if val != 777 {
		t.Errorf("expected 777, got %d", val)
	}
}

func TestLazyDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/lazy_destroy_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	lazy, err := NewLazy[int32](mem, "lazy_destroy")
	if err != nil {
		t.Fatalf("NewLazy failed: %v", err)
	}
	lazy.Destroy()

	// Should be able to create again
	lazy2, err := NewLazy[int32](mem, "lazy_destroy")
	if err != nil {
		t.Fatalf("NewLazy after destroy failed: %v", err)
	}
	lazy2.Destroy()
}
