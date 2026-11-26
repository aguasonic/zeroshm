package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestEventName tests the Name method.
func TestEventName(t *testing.T) {
	name := fmt.Sprintf("/test_event_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "my_event", EventModeAutoReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	if ev.Name() != "my_event" {
		t.Errorf("Name() = %q, want my_event", ev.Name())
	}
}

// TestEventIsSignaled tests the IsSignaled method.
func TestEventIsSignaled(t *testing.T) {
	name := fmt.Sprintf("/test_event_signaled_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "manual_event", EventModeManualReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// Initially not signaled
	if ev.IsSignaled() {
		t.Error("IsSignaled() should be false initially")
	}

	// Signal it
	ev.Signal()
	if !ev.IsSignaled() {
		t.Error("IsSignaled() should be true after Signal()")
	}

	// Reset it
	ev.Reset()
	if ev.IsSignaled() {
		t.Error("IsSignaled() should be false after Reset()")
	}
}

// TestEventAutoReset tests AutoReset event behavior.
func TestEventAutoReset(t *testing.T) {
	name := fmt.Sprintf("/test_event_automanual_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "auto_event", EventModeAutoReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// Signal and wait in a goroutine
	done := make(chan struct{})
	go func() {
		ev.Wait()
		close(done)
	}()

	// Give the waiter time to block
	time.Sleep(50 * time.Millisecond)

	// Signal the event
	ev.Signal()

	// Wait should return
	select {
	case <-done:
		// Success
	case <-time.After(500 * time.Millisecond):
		t.Error("AutoReset event Wait() did not return after Signal()")
	}

	// After AutoReset, the event should be non-signaled
	if ev.IsSignaled() {
		t.Error("AutoReset event should be non-signaled after Wait() consumes the signal")
	}
}

// TestEventManualReset tests ManualReset event behavior.
func TestEventManualReset(t *testing.T) {
	name := fmt.Sprintf("/test_event_manual_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "manual_event", EventModeManualReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// Start two waiters
	var wg sync.WaitGroup
	for i := 0; i < 2; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			ev.Wait()
		}()
	}

	// Give waiters time to block
	time.Sleep(50 * time.Millisecond)

	// Signal once - both waiters should wake (ManualReset stays signaled)
	ev.Signal()

	wg.Wait()

	// Event should still be signaled
	if !ev.IsSignaled() {
		t.Error("ManualReset event should still be signaled after waiters wake")
	}

	// Reset
	ev.Reset()
	if ev.IsSignaled() {
		t.Error("ManualReset event should be non-signaled after Reset()")
	}
}

// TestEventWaitTimeoutSuccess tests WaitTimeout with success.
func TestEventWaitTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/test_event_wait_ok_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "event_ok", EventModeAutoReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// Signal immediately
	ev.Signal()

	// WaitTimeout should succeed immediately
	if !ev.WaitTimeout(100 * time.Millisecond) {
		t.Error("WaitTimeout should succeed when event is already signaled")
	}
}

// TestEventWaitTimeoutFail tests WaitTimeout with timeout.
func TestEventWaitTimeoutFail(t *testing.T) {
	name := fmt.Sprintf("/test_event_wait_fail_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "event_fail", EventModeAutoReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// WaitTimeout should fail (no signal)
	start := time.Now()
	if ev.WaitTimeout(50 * time.Millisecond) {
		t.Error("WaitTimeout should fail when event is not signaled")
	}
	elapsed := time.Since(start)
	if elapsed < 40*time.Millisecond {
		t.Errorf("WaitTimeout returned too quickly: %v", elapsed)
	}
}

// TestEventPulse tests Pulse behavior.
func TestEventPulse(t *testing.T) {
	name := fmt.Sprintf("/test_event_pulse_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "pulse_event", EventModeManualReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// Start a waiter
	done := make(chan struct{})
	go func() {
		ev.Wait()
		close(done)
	}()

	// Give the waiter time to block
	time.Sleep(50 * time.Millisecond)

	// Pulse should wake the waiter and then reset
	ev.Pulse()

	// Wait should return
	select {
	case <-done:
		// Success
	case <-time.After(500 * time.Millisecond):
		t.Error("Wait() did not return after Pulse()")
	}

	// Event should be non-signaled after Pulse
	if ev.IsSignaled() {
		t.Error("Event should be non-signaled after Pulse()")
	}
}

// TestEventNewNameTooShort tests NewEvent with name too short.
func TestEventNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/test_event_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer func() {
		mem.Close()
		mem.Unlink()
	}()

	// Name shorter than 3 characters should fail
	_, err = NewEvent(mem, "a", EventModeAutoReset)
	if err == nil {
		t.Error("NewEvent with 1-char name should fail")
	}
}

// TestEventNewNameTooLong tests NewEvent with name too long.
func TestEventNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_event_longname_%d", os.Getpid())
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

	_, err = NewEvent(mem, longName, EventModeAutoReset)
	if err == nil {
		t.Error("NewEvent with name too long should fail")
	}
}

// TestEventNewAlreadyExists tests NewEvent when name already exists.
func TestEventNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_event_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewEvent(mem, "dup_event", EventModeAutoReset); err != nil {
		t.Fatalf("First NewEvent failed: %v", err)
	}

	_, err = NewEvent(mem, "dup_event", EventModeAutoReset)
	if err == nil {
		t.Error("NewEvent with existing name should fail")
	}
}

// TestEventOpenNotExists tests OpenEvent with non-existent event.
func TestEventOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_event_open_notexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenEvent(mem, "nonexistent")
	if err == nil {
		t.Error("OpenEvent with non-existent name should fail")
	}
}

// TestEventOpenCrossProcess tests OpenEvent seeing state from creator.
func TestEventOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_event_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev1, err := NewEvent(mem, "cross_event", EventModeManualReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	// Signal through ev1
	ev1.Signal()
	if !ev1.IsSignaled() {
		t.Error("ev1 should be signaled")
	}

	// Open the same event
	ev2, err := OpenEvent(mem, "cross_event")
	if err != nil {
		t.Fatalf("OpenEvent failed: %v", err)
	}

	// Verify state is visible
	if !ev2.IsSignaled() {
		t.Error("ev2 should see event as signaled")
	}

	// Reset through ev1, verify ev2 sees it
	ev1.Reset()
	if ev2.IsSignaled() {
		t.Error("ev2 should see event as non-signaled after Reset()")
	}
}

// TestEventConcurrentWaiters tests multiple concurrent waiters on ManualReset.
func TestEventConcurrentWaiters(t *testing.T) {
	name := fmt.Sprintf("/test_event_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	ev, err := NewEvent(mem, "concurrent_event", EventModeManualReset)
	if err != nil {
		t.Fatalf("NewEvent failed: %v", err)
	}

	var wg sync.WaitGroup
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			ev.Wait()
		}()
	}

	// Give waiters time to block
	time.Sleep(100 * time.Millisecond)

	// Signal once - all should wake
	ev.Signal()

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
		t.Error("Not all waiters woke up after Signal()")
	}
}
