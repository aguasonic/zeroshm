package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestBarrierName tests the Name method.
func TestBarrierName(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "my_barrier", 2)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	if b.Name() != "my_barrier" {
		t.Errorf("Name() = %q, want my_barrier", b.Name())
	}
}

// TestBarrierNumParticipants tests the NumParticipants method.
func TestBarrierNumParticipants(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_np_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "np_barrier", 5)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	if b.NumParticipants() != 5 {
		t.Errorf("NumParticipants() = %d, want 5", b.NumParticipants())
	}
}

// TestBarrierArrived tests the Arrived method.
func TestBarrierArrived(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_arrived_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "arrived_barrier", 3)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	if b.Arrived() != 0 {
		t.Errorf("Initial Arrived() = %d, want 0", b.Arrived())
	}

	// Start 2 goroutines that will block on Wait
	done1 := make(chan struct{})
	done2 := make(chan struct{})
	go func() {
		b.Wait()
		close(done1)
	}()
	go func() {
		b.Wait()
		close(done2)
	}()

	// Give them time to arrive
	time.Sleep(10 * time.Millisecond)

	if b.Arrived() != 2 {
		t.Errorf("Arrived() = %d, want 2", b.Arrived())
	}

	// Third Wait releases everyone
	b.Wait()

	// Wait for goroutines to finish
	<-done1
	<-done2

	// After reset, Arrived should be 0
	if b.Arrived() != 0 {
		t.Errorf("Arrived() after reset = %d, want 0", b.Arrived())
	}
}

// TestBarrierGeneration tests the Generation method.
func TestBarrierGeneration(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_gen_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "gen_barrier", 2)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	if b.Generation() != 0 {
		t.Errorf("Initial Generation() = %d, want 0", b.Generation())
	}

	// First cycle
	done1 := make(chan struct{})
	go func() {
		b.Wait()
		close(done1)
	}()
	time.Sleep(5 * time.Millisecond)
	b.Wait()
	<-done1

	if b.Generation() != 1 {
		t.Errorf("Generation() after 1 cycle = %d, want 1", b.Generation())
	}

	// Second cycle
	done2 := make(chan struct{})
	go func() {
		b.Wait()
		close(done2)
	}()
	time.Sleep(5 * time.Millisecond)
	b.Wait()
	<-done2

	if b.Generation() != 2 {
		t.Errorf("Generation() after 2 cycles = %d, want 2", b.Generation())
	}
}

// TestBarrierWaitTimeoutSuccess tests WaitTimeout with success.
func TestBarrierWaitTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_wt_ok_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "wt_ok", 2)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	// Start a goroutine that will block on WaitTimeout
	done := make(chan struct{})
	go func() {
		b.WaitTimeout(1 * time.Second)
		close(done)
	}()

	time.Sleep(10 * time.Millisecond)

	// Second Wait should release both (as the last participant)
	if !b.WaitTimeout(100 * time.Millisecond) {
		t.Error("Second WaitTimeout should succeed")
	}

	<-done
}

// TestBarrierWaitTimeoutFail tests WaitTimeout with timeout.
func TestBarrierWaitTimeoutFail(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_wt_fail_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "wt_fail", 2)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	// WaitTimeout should fail (only 1 participant, barrier needs 2)
	start := time.Now()
	if b.WaitTimeout(50 * time.Millisecond) {
		t.Error("WaitTimeout should fail when not all participants arrive")
	}
	elapsed := time.Since(start)
	if elapsed < 40*time.Millisecond {
		t.Errorf("WaitTimeout returned too quickly: %v", elapsed)
	}

	// Arrived should be reset to 0 after timeout
	if b.Arrived() != 0 {
		t.Errorf("Arrived() after timeout = %d, want 0", b.Arrived())
	}
}

// TestBarrierNewNameTooLong tests NewBarrier with name too long.
func TestBarrierNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_longname_%d", os.Getpid())
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

	_, err = NewBarrier(mem, longName, 2)
	if err == nil {
		t.Error("NewBarrier with name too long should fail")
	}
}

// TestBarrierNewZeroParticipants tests NewBarrier with zero participants.
func TestBarrierNewZeroParticipants(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_zero_np_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewBarrier(mem, "zero_np", 0)
	if err == nil {
		t.Error("NewBarrier with zero participants should fail")
	}

	_, err = NewBarrier(mem, "neg_np", -1)
	if err == nil {
		t.Error("NewBarrier with negative participants should fail")
	}
}

// TestBarrierNewAlreadyExists tests NewBarrier when name already exists.
func TestBarrierNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewBarrier(mem, "dup_barrier", 2); err != nil {
		t.Fatalf("First NewBarrier failed: %v", err)
	}

	_, err = NewBarrier(mem, "dup_barrier", 2)
	if err == nil {
		t.Error("NewBarrier with existing name should fail")
	}
}

// TestBarrierOpenNotExists tests OpenBarrier with non-existent barrier.
func TestBarrierOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_b_open_nexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenBarrier(mem, "nonexistent")
	if err == nil {
		t.Error("OpenBarrier with non-existent name should fail")
	}
}

// TestBarrierOpenCrossProcess tests OpenBarrier seeing state from creator.
func TestBarrierOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b1, err := NewBarrier(mem, "cross_barrier", 2)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	// Open the same barrier
	b2, err := OpenBarrier(mem, "cross_barrier")
	if err != nil {
		t.Fatalf("OpenBarrier failed: %v", err)
	}

	// Verify state is visible
	if b2.NumParticipants() != 2 {
		t.Errorf("b2.NumParticipants() = %d, want 2", b2.NumParticipants())
	}

	// Both barriers should release together
	done1 := make(chan struct{})
	done2 := make(chan struct{})
	go func() {
		b1.Wait()
		close(done1)
	}()
	go func() {
		b2.Wait()
		close(done2)
	}()

	// Wait for both to complete
	select {
	case <-done1:
	case <-time.After(1 * time.Second):
		t.Fatal("b1.Wait() timed out")
	}
	select {
	case <-done2:
	case <-time.After(1 * time.Second):
		t.Fatal("b2.Wait() timed out")
	}
}

// TestBarrierConcurrent tests concurrent barrier synchronization.
func TestBarrierConcurrent(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	const numParticipants = 5
	b, err := NewBarrier(mem, "concurrent_barrier", numParticipants)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	var wg sync.WaitGroup

	for i := 0; i < numParticipants-1; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			b.Wait()
		}()
	}

	// Main goroutine also waits
	b.Wait()

	// Wait for all goroutines to pass the barrier
	wg.Wait()
}

// TestBarrierReusable tests that the barrier can be reused multiple times.
func TestBarrierReusable(t *testing.T) {
	name := fmt.Sprintf("/test_barrier_reuse_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	b, err := NewBarrier(mem, "reuse_barrier", 2)
	if err != nil {
		t.Fatalf("NewBarrier failed: %v", err)
	}

	for cycle := 0; cycle < 3; cycle++ {
		done := make(chan struct{})
		go func() {
			b.Wait()
			close(done)
		}()
		time.Sleep(5 * time.Millisecond)
		b.Wait()
		<-done

		if b.Generation() != int32(cycle+1) {
			t.Errorf("Cycle %d: Generation() = %d, want %d", cycle, b.Generation(), cycle+1)
		}
	}
}
