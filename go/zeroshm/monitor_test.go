package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestMonitorName tests the Name method.
func TestMonitorName(t *testing.T) {
	name := fmt.Sprintf("/mon_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "my_monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	if mon.Name() != "my_monitor" {
		t.Errorf("Name() = %q, want my_monitor", mon.Name())
	}
}

// TestMonitorLockUnlock tests basic Lock/Unlock.
func TestMonitorLockUnlock(t *testing.T) {
	name := fmt.Sprintf("/mon_lock_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	mon.Lock()
	mon.Unlock()
}

// TestMonitorTryLock tests TryLock success and failure.
func TestMonitorTryLock(t *testing.T) {
	name := fmt.Sprintf("/mon_try_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	// First TryLock should succeed
	if !mon.TryLock() {
		t.Error("First TryLock should succeed")
	}

	// Second TryLock should fail (already locked)
	if mon.TryLock() {
		t.Error("Second TryLock should fail when monitor is locked")
	}

	mon.Unlock()

	// TryLock should succeed again
	if !mon.TryLock() {
		t.Error("TryLock after Unlock should succeed")
	}
	mon.Unlock()
}

// TestMonitorNotifyOne tests NotifyOne waking a single waiter.
func TestMonitorNotifyOne(t *testing.T) {
	name := fmt.Sprintf("/mon_n1_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	var value int32

	// Start a waiter
	done := make(chan struct{})
	go func() {
		mon.Lock()
		defer mon.Unlock()
		mon.WaitWithPredicate(func() bool { return value == 1 })
		close(done)
	}()

	// Give the waiter time to block
	time.Sleep(50 * time.Millisecond)

	// Set value and notify
	mon.Lock()
	value = 1
	mon.NotifyOne()
	mon.Unlock()

	// Waiter should wake
	select {
	case <-done:
		// Success
	case <-time.After(1 * time.Second):
		t.Error("Waiter did not wake after NotifyOne()")
	}
}

// TestMonitorNotifyAll tests NotifyAll waking all waiters.
func TestMonitorNotifyAll(t *testing.T) {
	name := fmt.Sprintf("/mon_na_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	var value int32
	var wg sync.WaitGroup

	// Start multiple waiters
	for i := 0; i < 3; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			mon.Lock()
			defer mon.Unlock()
			mon.WaitWithPredicate(func() bool { return value == 1 })
		}()
	}

	// Give waiters time to block
	time.Sleep(100 * time.Millisecond)

	// Set value and notify all
	mon.Lock()
	value = 1
	mon.NotifyAll()
	mon.Unlock()

	// Wait for all to finish with timeout
	done := make(chan struct{})
	go func() {
		wg.Wait()
		close(done)
	}()

	select {
	case <-done:
		// All waiters woke up
	case <-time.After(2 * time.Second):
		t.Error("Not all waiters woke up after NotifyAll()")
	}
}

// TestMonitorWaitTimeout tests Wait with timeout.
func TestMonitorWaitTimeout(t *testing.T) {
	name := fmt.Sprintf("/mon_to_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	mon.Lock()

	// Wait should timeout (predicate never becomes true)
	start := time.Now()
	result := mon.WaitTimeoutWithPredicate(50*time.Millisecond, func() bool { return false })
	elapsed := time.Since(start)

	mon.Unlock()

	if result {
		t.Error("WaitTimeout should return false when predicate is never true")
	}
	if elapsed < 40*time.Millisecond {
		t.Errorf("WaitTimeout returned too quickly: %v", elapsed)
	}
}

// TestMonitorWaitTimeoutSuccess tests Wait with timeout success.
func TestMonitorWaitTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/mon_tok_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	var value int32

	// Start a goroutine that sets value and notifies
	go func() {
		time.Sleep(50 * time.Millisecond)
		mon.Lock()
		value = 1
		mon.NotifyOne()
		mon.Unlock()
	}()

	mon.Lock()
	result := mon.WaitTimeoutWithPredicate(500*time.Millisecond, func() bool { return value == 1 })
	mon.Unlock()

	if !result {
		t.Error("WaitTimeout should return true when predicate becomes true")
	}
}

// TestMonitorNewNameTooLong tests NewMonitor with name too long.
func TestMonitorNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/mon_long_%d", os.Getpid())
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

	_, err = NewMonitor(mem, longName)
	if err == nil {
		t.Error("NewMonitor with name too long should fail")
	}
}

// TestMonitorNewAlreadyExists tests NewMonitor when name already exists.
func TestMonitorNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/mon_ex_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewMonitor(mem, "dup_monitor"); err != nil {
		t.Fatalf("First NewMonitor failed: %v", err)
	}

	_, err = NewMonitor(mem, "dup_monitor")
	if err == nil {
		t.Error("NewMonitor with existing name should fail")
	}
}

// TestMonitorOpenNotExists tests OpenMonitor with non-existent monitor.
func TestMonitorOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/mon_onx_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenMonitor(mem, "nonexistent")
	if err == nil {
		t.Error("OpenMonitor with non-existent name should fail")
	}
}

// TestMonitorOpenCrossProcess tests OpenMonitor seeing state from creator.
func TestMonitorOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/mon_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon1, err := NewMonitor(mem, "cross_monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	// Lock through mon1
	mon1.Lock()

	// Open the same monitor
	mon2, err := OpenMonitor(mem, "cross_monitor")
	if err != nil {
		t.Fatalf("OpenMonitor failed: %v", err)
	}

	// TryLock through mon2 should fail
	if mon2.TryLock() {
		t.Error("mon2.TryLock should fail when monitor is locked")
	}

	// Unlock through mon1, verify mon2 can lock
	mon1.Unlock()

	if !mon2.TryLock() {
		t.Error("mon2.TryLock should succeed after mon1 unlocks")
	}
	mon2.Unlock()
}

// TestMonitorCrossGoroutine tests wait/notify across goroutines.
func TestMonitorCrossGoroutine(t *testing.T) {
	name := fmt.Sprintf("/mon_pc_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mon, err := NewMonitor(mem, "pc_monitor")
	if err != nil {
		t.Fatalf("NewMonitor failed: %v", err)
	}

	var value int32

	// Start a waiter in a goroutine
	done := make(chan struct{})
	go func() {
		defer close(done)
		mon.Lock()
		defer mon.Unlock()
		mon.WaitWithPredicate(func() bool { return value == 42 })
	}()

	// Give the waiter time to block
	time.Sleep(100 * time.Millisecond)

	// Set value and notify from the main goroutine
	mon.Lock()
	value = 42
	mon.NotifyOne()
	mon.Unlock()

	// Waiter should wake
	select {
	case <-done:
		// Success
	case <-time.After(2 * time.Second):
		t.Error("Waiter did not wake after NotifyOne()")
	}
}
