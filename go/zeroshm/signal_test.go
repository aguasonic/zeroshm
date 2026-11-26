package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestSignalName tests the Name method.
func TestSignalName(t *testing.T) {
	name := fmt.Sprintf("/test_signal_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "my_signal", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	if sig.Name() != "my_signal" {
		t.Errorf("Name() = %q, want my_signal", sig.Name())
	}
}

// TestSignalGetSet tests Get and Set operations.
func TestSignalGetSet(t *testing.T) {
	name := fmt.Sprintf("/test_signal_getset_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	if sig.Get() != 0 {
		t.Errorf("Get() = %d, want 0", sig.Get())
	}

	sig.Set(42)
	if sig.Get() != 42 {
		t.Errorf("Get() = %d, want 42", sig.Get())
	}

	sig.Set(100)
	if sig.Get() != 100 {
		t.Errorf("Get() = %d, want 100", sig.Get())
	}
}

// TestSignalUpdate tests the Update method.
func TestSignalUpdate(t *testing.T) {
	name := fmt.Sprintf("/test_signal_update_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(10))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	sig.Update(func(v int32) int32 { return v + 5 })
	if sig.Get() != 15 {
		t.Errorf("Get() = %d, want 15", sig.Get())
	}

	sig.Update(func(v int32) int32 { return v * 2 })
	if sig.Get() != 30 {
		t.Errorf("Get() = %d, want 30", sig.Get())
	}
}

// TestSignalVersion tests version tracking.
func TestSignalVersion(t *testing.T) {
	name := fmt.Sprintf("/test_signal_version_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	if sig.Version() != 0 {
		t.Errorf("Version() = %d, want 0", sig.Version())
	}

	sig.Set(1)
	if sig.Version() != 1 {
		t.Errorf("Version() = %d, want 1", sig.Version())
	}

	sig.Set(2)
	if sig.Version() != 2 {
		t.Errorf("Version() = %d, want 2", sig.Version())
	}

	sig.Update(func(v int32) int32 { return v + 1 })
	if sig.Version() != 3 {
		t.Errorf("Version() = %d, want 3", sig.Version())
	}
}

// TestSignalHasChanged tests the HasChanged method.
func TestSignalHasChanged(t *testing.T) {
	name := fmt.Sprintf("/test_signal_changed_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	lastVersion := sig.Version()

	// No change yet
	if sig.HasChanged(lastVersion) {
		t.Error("HasChanged should be false before any Set")
	}

	sig.Set(42)

	if !sig.HasChanged(lastVersion) {
		t.Error("HasChanged should be true after Set")
	}

	// Update lastVersion
	lastVersion = sig.Version()
	if sig.HasChanged(lastVersion) {
		t.Error("HasChanged should be false after updating lastVersion")
	}
}

// TestSignalWaitForChange tests WaitForChange with success.
func TestSignalWaitForChange(t *testing.T) {
	name := fmt.Sprintf("/test_signal_wait_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	lastVersion := sig.Version()

	// Start a goroutine that changes the value after a short delay
	done := make(chan struct{})
	go func() {
		time.Sleep(50 * time.Millisecond)
		sig.Set(42)
		close(done)
	}()

	// WaitForChange should return true when the value changes
	if !sig.WaitForChange(lastVersion, 500*time.Millisecond) {
		t.Error("WaitForChange should return true when value changes")
	}

	<-done
}

// TestSignalWaitForChangeTimeout tests WaitForChange timeout.
func TestSignalWaitForChangeTimeout(t *testing.T) {
	name := fmt.Sprintf("/test_signal_wait_timeout_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	lastVersion := sig.Version()

	// WaitForChange should timeout if no change occurs
	start := time.Now()
	if sig.WaitForChange(lastVersion, 50*time.Millisecond) {
		t.Error("WaitForChange should return false on timeout")
	}
	elapsed := time.Since(start)
	if elapsed < 40*time.Millisecond {
		t.Errorf("WaitForChange returned too quickly: %v", elapsed)
	}
}

// TestSignalNewNameTooLong tests NewSignal with name too long.
func TestSignalNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_signal_longname_%d", os.Getpid())
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

	_, err = NewSignal(mem, longName, int32(0))
	if err == nil {
		t.Error("NewSignal with name too long should fail")
	}
}

// TestSignalNewAlreadyExists tests NewSignal when name already exists.
func TestSignalNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_signal_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewSignal(mem, "dup_signal", int32(0)); err != nil {
		t.Fatalf("First NewSignal failed: %v", err)
	}

	_, err = NewSignal(mem, "dup_signal", int32(0))
	if err == nil {
		t.Error("NewSignal with existing name should fail")
	}
}

// TestSignalOpenNotExists tests OpenSignal with non-existent signal.
func TestSignalOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/sig_onx_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenSignal[int32](mem, "nonexistent")
	if err == nil {
		t.Error("OpenSignal with non-existent name should fail")
	}
}

// TestSignalOpenCrossProcess tests OpenSignal seeing state from creator.
func TestSignalOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_signal_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig1, err := NewSignal(mem, "cross_signal", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	// Set value through sig1
	sig1.Set(42)

	// Open the same signal
	sig2, err := OpenSignal[int32](mem, "cross_signal")
	if err != nil {
		t.Fatalf("OpenSignal failed: %v", err)
	}

	// Verify value is visible
	if sig2.Get() != 42 {
		t.Errorf("sig2.Get() = %d, want 42", sig2.Get())
	}

	// Verify version is visible
	if sig2.Version() != 1 {
		t.Errorf("sig2.Version() = %d, want 1", sig2.Version())
	}

	// Set through sig2, verify sig1 sees it
	sig2.Set(100)
	if sig1.Get() != 100 {
		t.Errorf("sig1.Get() = %d, want 100", sig1.Get())
	}
}

// TestSignalConcurrentAccess tests concurrent Set from multiple goroutines.
func TestSignalConcurrentAccess(t *testing.T) {
	name := fmt.Sprintf("/test_signal_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "counter", int32(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	var wg sync.WaitGroup
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func(val int32) {
			defer wg.Done()
			for j := int32(0); j < 100; j++ {
				sig.Update(func(v int32) int32 { return v + val })
			}
		}(int32(i + 1))
	}

	wg.Wait()

	// Sum of 1..5 = 15, each repeated 100 times = 1500
	if sig.Get() != 1500 {
		t.Errorf("Get() = %d, want 1500", sig.Get())
	}

	// Version should be 500 (5 goroutines * 100 updates)
	if sig.Version() != 500 {
		t.Errorf("Version() = %d, want 500", sig.Version())
	}
}

// TestSignalFloat64 tests Signal with float64 type.
func TestSignalFloat64(t *testing.T) {
	name := fmt.Sprintf("/test_signal_float_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sig, err := NewSignal(mem, "sensor", float64(0))
	if err != nil {
		t.Fatalf("NewSignal failed: %v", err)
	}

	sig.Set(3.14)
	if sig.Get() != 3.14 {
		t.Errorf("Get() = %v, want 3.14", sig.Get())
	}

	sig.Update(func(v float64) float64 { return v * 2 })
	if sig.Get() != 6.28 {
		t.Errorf("Get() = %v, want 6.28", sig.Get())
	}
}
