package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestLatchName tests the Name method.
func TestLatchName(t *testing.T) {
	name := fmt.Sprintf("/test_latch_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "my_latch", 5)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	if l.Name() != "my_latch" {
		t.Errorf("Name() = %q, want my_latch", l.Name())
	}
}

// TestLatchCount tests the Count method.
func TestLatchCount(t *testing.T) {
	name := fmt.Sprintf("/test_latch_count_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "count_latch", 5)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	if l.Count() != 5 {
		t.Errorf("Initial Count() = %d, want 5", l.Count())
	}

	l.CountDownOne()
	if l.Count() != 4 {
		t.Errorf("Count() after CountDownOne = %d, want 4", l.Count())
	}

	l.CountDown(2)
	if l.Count() != 2 {
		t.Errorf("Count() after CountDown(2) = %d, want 2", l.Count())
	}

	l.CountDown(10) // Should saturate at 0
	if l.Count() != 0 {
		t.Errorf("Count() after CountDown(10) = %d, want 0", l.Count())
	}
}

// TestLatchInitialCount tests the InitialCount method.
func TestLatchInitialCount(t *testing.T) {
	name := fmt.Sprintf("/test_latch_initial_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "initial_latch", 10)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	if l.InitialCount() != 10 {
		t.Errorf("InitialCount() = %d, want 10", l.InitialCount())
	}

	l.CountDown(7)
	if l.InitialCount() != 10 {
		t.Errorf("InitialCount() after CountDown = %d, want 10 (immutable)", l.InitialCount())
	}
}

// TestLatchCountDown tests the CountDown method.
func TestLatchCountDown(t *testing.T) {
	name := fmt.Sprintf("/test_latch_cd_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "cd_latch", 3)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// CountDown with negative value should fail
	if err := l.CountDown(-1); err == nil {
		t.Error("CountDown(-1) should fail")
	}

	// CountDown with zero should fail
	if err := l.CountDown(0); err == nil {
		t.Error("CountDown(0) should fail")
	}

	// CountDown by 1
	if err := l.CountDown(1); err != nil {
		t.Fatalf("CountDown(1) failed: %v", err)
	}
	if l.Count() != 2 {
		t.Errorf("Count() after CountDown(1) = %d, want 2", l.Count())
	}

	// CountDown by more than remaining (should saturate at 0)
	if err := l.CountDown(10); err != nil {
		t.Fatalf("CountDown(10) failed: %v", err)
	}
	if l.Count() != 0 {
		t.Errorf("Count() after CountDown(10) = %d, want 0", l.Count())
	}

	// CountDown when already at 0 should succeed (no-op)
	if err := l.CountDown(1); err != nil {
		t.Fatalf("CountDown(1) when at 0 failed: %v", err)
	}
	if l.Count() != 0 {
		t.Errorf("Count() after CountDown at 0 = %d, want 0", l.Count())
	}
}

// TestLatchTryWait tests the TryWait method.
func TestLatchTryWait(t *testing.T) {
	name := fmt.Sprintf("/test_latch_trywait_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "trywait_latch", 1)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// Should not be satisfied yet
	if l.TryWait() {
		t.Error("TryWait() should be false when count > 0")
	}

	// Countdown to 0
	l.CountDownOne()

	// Should be satisfied now
	if !l.TryWait() {
		t.Error("TryWait() should be true when count == 0")
	}
}

// TestLatchWait tests the Wait method.
func TestLatchWait(t *testing.T) {
	name := fmt.Sprintf("/test_latch_wait_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "wait_latch", 1)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// Start a goroutine that will block on Wait
	done := make(chan struct{})
	go func() {
		l.Wait()
		close(done)
	}()

	// Give the goroutine time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Verify it's still waiting
	select {
	case <-done:
		t.Error("Wait() should not return yet")
	default:
		// Expected
	}

	// Countdown to release
	l.CountDownOne()

	// Wait should return now
	select {
	case <-done:
		// Success
	case <-time.After(1 * time.Second):
		t.Fatal("Wait() timed out after CountDown")
	}
}

// TestLatchWaitAlreadyZero tests Wait when latch is already at 0.
func TestLatchWaitAlreadyZero(t *testing.T) {
	name := fmt.Sprintf("/test_latch_wait_zero_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "wait_zero_latch", 0)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// Wait should return immediately
	done := make(chan struct{})
	go func() {
		l.Wait()
		close(done)
	}()

	select {
	case <-done:
		// Success - returned immediately
	case <-time.After(100 * time.Millisecond):
		t.Fatal("Wait() on latch with count=0 should return immediately")
	}
}

// TestLatchWaitTimeoutSuccess tests WaitTimeout with success.
func TestLatchWaitTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/test_latch_wt_ok_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "wt_ok", 1)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// Start a goroutine that will block on WaitTimeout
	done := make(chan struct{})
	go func() {
		l.WaitTimeout(1 * time.Second)
		close(done)
	}()

	time.Sleep(10 * time.Millisecond)

	// Countdown to release
	l.CountDownOne()

	<-done
}

// TestLatchWaitTimeoutFail tests WaitTimeout with timeout.
func TestLatchWaitTimeoutFail(t *testing.T) {
	name := fmt.Sprintf("/test_latch_wt_fail_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "wt_fail", 1)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// WaitTimeout should fail (count is 1, won't reach 0)
	start := time.Now()
	if l.WaitTimeout(50 * time.Millisecond) {
		t.Error("WaitTimeout should fail when count doesn't reach 0")
	}
	elapsed := time.Since(start)
	if elapsed < 40*time.Millisecond {
		t.Errorf("WaitTimeout returned too quickly: %v", elapsed)
	}
}

// TestLatchReset tests the Reset method.
func TestLatchReset(t *testing.T) {
	name := fmt.Sprintf("/test_latch_reset_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l, err := NewLatch(mem, "reset_latch", 3)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	l.CountDown(3)
	if l.Count() != 0 {
		t.Errorf("Count() after CountDown(3) = %d, want 0", l.Count())
	}

	l.Reset()
	if l.Count() != 3 {
		t.Errorf("Count() after Reset = %d, want 3", l.Count())
	}
}

// TestLatchNewNameTooLong tests NewLatch with name too long.
func TestLatchNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_latch_longname_%d", os.Getpid())
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

	_, err = NewLatch(mem, longName, 1)
	if err == nil {
		t.Error("NewLatch with name too long should fail")
	}
}

// TestLatchNewNegativeCount tests NewLatch with negative count.
func TestLatchNewNegativeCount(t *testing.T) {
	name := fmt.Sprintf("/test_latch_negcount_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewLatch(mem, "neg_count", -1)
	if err == nil {
		t.Error("NewLatch with negative count should fail")
	}
}

// TestLatchNewAlreadyExists tests NewLatch when name already exists.
func TestLatchNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_latch_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewLatch(mem, "dup_latch", 2); err != nil {
		t.Fatalf("First NewLatch failed: %v", err)
	}

	_, err = NewLatch(mem, "dup_latch", 2)
	if err == nil {
		t.Error("NewLatch with existing name should fail")
	}
}

// TestLatchOpenNotExists tests OpenLatch with non-existent latch.
func TestLatchOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_l_open_nexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenLatch(mem, "nonexistent")
	if err == nil {
		t.Error("OpenLatch with non-existent name should fail")
	}
}

// TestLatchOpenCrossProcess tests OpenLatch seeing state from creator.
func TestLatchOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_latch_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	l1, err := NewLatch(mem, "cross_latch", 3)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	// Open the same latch
	l2, err := OpenLatch(mem, "cross_latch")
	if err != nil {
		t.Fatalf("OpenLatch failed: %v", err)
	}

	// Verify state is visible
	if l2.Count() != 3 {
		t.Errorf("l2.Count() = %d, want 3", l2.Count())
	}
	if l2.InitialCount() != 3 {
		t.Errorf("l2.InitialCount() = %d, want 3", l2.InitialCount())
	}

	// Countdown from l1, verify l2 sees it
	l1.CountDown(2)
	if l2.Count() != 1 {
		t.Errorf("l2.Count() after l1.CountDown(2) = %d, want 1", l2.Count())
	}

	// Countdown from l2, verify l1 sees it
	l2.CountDownOne()
	if l1.Count() != 0 {
		t.Errorf("l1.Count() after l2.CountDownOne = %d, want 0", l1.Count())
	}

	// Wait on l1 should return immediately (count is 0)
	done := make(chan struct{})
	go func() {
		l1.Wait()
		close(done)
	}()

	select {
	case <-done:
		// Success
	case <-time.After(1 * time.Second):
		t.Fatal("l1.Wait() timed out")
	}
}

// TestLatchConcurrentCountDown tests concurrent CountDown from multiple goroutines.
func TestLatchConcurrentCountDown(t *testing.T) {
	name := fmt.Sprintf("/test_latch_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	const numGoroutines = 5
	l, err := NewLatch(mem, "concurrent_latch", numGoroutines)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	var wg sync.WaitGroup

	// Start goroutines that wait for the latch
	for i := 0; i < numGoroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			l.Wait()
		}()
	}

	// Give them time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Countdown from multiple goroutines
	for i := 0; i < numGoroutines; i++ {
		go func() {
			l.CountDownOne()
		}()
	}

	// Wait for all waiters to finish
	wg.Wait()

	if l.Count() != 0 {
		t.Errorf("Count() after concurrent countdown = %d, want 0", l.Count())
	}
}

// TestLatchCompletionPattern tests the completion use case (count=N workers).
func TestLatchCompletionPattern(t *testing.T) {
	name := fmt.Sprintf("/test_latch_completion_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	const numWorkers = 4
	l, err := NewLatch(mem, "completion_latch", numWorkers)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	var wg sync.WaitGroup

	// Coordinator waits for all workers
	done := make(chan struct{})
	go func() {
		l.Wait()
		close(done)
	}()

	// Workers countdown when done
	for i := 0; i < numWorkers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			// Simulate work
			time.Sleep(10 * time.Millisecond)
			l.CountDownOne()
		}()
	}

	wg.Wait()

	// Coordinator should have been released
	select {
	case <-done:
		// Success
	case <-time.After(1 * time.Second):
		t.Fatal("Coordinator Wait() timed out")
	}
}

// TestLatchStartGatePattern tests the start gate use case (count=1).
func TestLatchStartGatePattern(t *testing.T) {
	name := fmt.Sprintf("/test_latch_gate_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Start gate: count=1, workers Wait(), coordinator CountDown()
	l, err := NewLatch(mem, "gate_latch", 1)
	if err != nil {
		t.Fatalf("NewLatch failed: %v", err)
	}

	var wg sync.WaitGroup
	const numWorkers = 3

	// Workers wait for the gate
	for i := 0; i < numWorkers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			l.Wait()
			// Workers should proceed after gate opens
		}()
	}

	// Give workers time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Coordinator opens the gate
	l.CountDownOne()

	// All workers should have passed the gate
	wg.Wait()
}
