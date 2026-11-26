package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestSemaphoreName tests the Name method.
func TestSemaphoreName(t *testing.T) {
	name := fmt.Sprintf("/test_sem_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "my_sem", 1, 5)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	if sem.Name() != "my_sem" {
		t.Errorf("Name() = %q, want my_sem", sem.Name())
	}
}

// TestSemaphoreMaxCount tests the MaxCount method.
func TestSemaphoreMaxCount(t *testing.T) {
	name := fmt.Sprintf("/test_sem_maxcount_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Unbounded
	sem, err := NewSemaphore(mem, "unbounded", 0, 0)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}
	if sem.MaxCount() != 0 {
		t.Errorf("MaxCount() = %d, want 0 (unbounded)", sem.MaxCount())
	}

	// Bounded
	sem2, err := NewSemaphore(mem, "bounded", 0, 10)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}
	if sem2.MaxCount() != 10 {
		t.Errorf("MaxCount() = %d, want 10", sem2.MaxCount())
	}
}

// TestSemaphoreWaiting tests the Waiting method.
func TestSemaphoreWaiting(t *testing.T) {
	name := fmt.Sprintf("/test_sem_waiting_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "wait_sem", 0, 10)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	if sem.Waiting() != 0 {
		t.Errorf("Waiting() = %d, want 0", sem.Waiting())
	}

	// Start a goroutine that will block on Acquire
	done := make(chan struct{})
	go func() {
		sem.Acquire() // will block since count=0
		close(done)
	}()

	// Give the goroutine time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Waiting should be > 0
	if sem.Waiting() <= 0 {
		t.Errorf("Waiting() = %d, want > 0", sem.Waiting())
	}

	// Release to unblock
	sem.Release()

	// Wait for goroutine to finish
	<-done

	// Waiting should be back to 0
	if sem.Waiting() != 0 {
		t.Errorf("Waiting() after release = %d, want 0", sem.Waiting())
	}
}

// TestSemaphoreAcquire tests the blocking Acquire method.
func TestSemaphoreAcquire(t *testing.T) {
	name := fmt.Sprintf("/test_sem_acquire_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "acq_sem", 2, 10)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// Acquire twice (should succeed immediately)
	sem.Acquire()
	if sem.Count() != 1 {
		t.Errorf("Count after 1 acquire = %d, want 1", sem.Count())
	}
	sem.Acquire()
	if sem.Count() != 0 {
		t.Errorf("Count after 2 acquires = %d, want 0", sem.Count())
	}

	// Start a goroutine that will block on Acquire (count=0)
	done := make(chan struct{})
	go func() {
		sem.Acquire() // will block
		close(done)
	}()

	time.Sleep(10 * time.Millisecond)
	if sem.Count() != 0 {
		t.Errorf("Count should still be 0 while goroutine is waiting")
	}

	// Release to unblock
	sem.Release()
	<-done

	if sem.Count() != 0 {
		t.Errorf("Count after acquire = %d, want 0", sem.Count())
	}
}

// TestSemaphoreAcquireTimeoutSuccess tests AcquireTimeout with success.
func TestSemaphoreAcquireTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/test_sem_acq_timeout_ok_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "timeout_ok", 1, 10)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// Should succeed immediately (count=1)
	if !sem.AcquireTimeout(100 * time.Millisecond) {
		t.Error("AcquireTimeout should succeed when count > 0")
	}
	if sem.Count() != 0 {
		t.Errorf("Count = %d, want 0", sem.Count())
	}
}

// TestSemaphoreAcquireTimeoutFail tests AcquireTimeout with timeout.
func TestSemaphoreAcquireTimeoutFail(t *testing.T) {
	name := fmt.Sprintf("/test_sem_to_fail_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "timeout_fail", 0, 10)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// Should time out (count=0, no one releases)
	start := time.Now()
	if sem.AcquireTimeout(50 * time.Millisecond) {
		t.Error("AcquireTimeout should fail when count=0 and no release")
	}
	elapsed := time.Since(start)
	if elapsed < 40*time.Millisecond {
		t.Errorf("AcquireTimeout returned too quickly: %v", elapsed)
	}
}

// TestSemaphoreReleaseExceedsMax tests Release when count would exceed max.
func TestSemaphoreReleaseExceedsMax(t *testing.T) {
	name := fmt.Sprintf("/test_sem_release_max_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "release_max", 5, 5)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// Count is already at max, Release should fail
	if err := sem.Release(); err == nil {
		t.Error("Release at max count should fail")
	}
	if sem.Count() != 5 {
		t.Errorf("Count = %d, want 5 (unchanged)", sem.Count())
	}
}

// TestSemaphoreNewNameTooLong tests NewSemaphore with name too long.
func TestSemaphoreNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_sem_longname_%d", os.Getpid())
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

	_, err = NewSemaphore(mem, longName, 0, 10)
	if err == nil {
		t.Error("NewSemaphore with name too long should fail")
	}
}

// TestSemaphoreNewNegativeInitial tests NewSemaphore with negative initial count.
func TestSemaphoreNewNegativeInitial(t *testing.T) {
	name := fmt.Sprintf("/test_sem_neg_init_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewSemaphore(mem, "neg_init", -1, 10)
	if err == nil {
		t.Error("NewSemaphore with negative initial count should fail")
	}
}

// TestSemaphoreNewNegativeMax tests NewSemaphore with negative max count.
func TestSemaphoreNewNegativeMax(t *testing.T) {
	name := fmt.Sprintf("/test_sem_neg_max_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewSemaphore(mem, "neg_max", 0, -1)
	if err == nil {
		t.Error("NewSemaphore with negative max count should fail")
	}
}

// TestSemaphoreNewInitialExceedsMax tests NewSemaphore with initial > max.
func TestSemaphoreNewInitialExceedsMax(t *testing.T) {
	name := fmt.Sprintf("/test_sem_init_max_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewSemaphore(mem, "init_max", 10, 5)
	if err == nil {
		t.Error("NewSemaphore with initial > max should fail")
	}
}

// TestSemaphoreNewAlreadyExists tests NewSemaphore when name already exists.
func TestSemaphoreNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_sem_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewSemaphore(mem, "dup_sem", 1, 5); err != nil {
		t.Fatalf("First NewSemaphore failed: %v", err)
	}

	_, err = NewSemaphore(mem, "dup_sem", 1, 5)
	if err == nil {
		t.Error("NewSemaphore with existing name should fail")
	}
}

// TestSemaphoreOpenNotExists tests OpenSemaphore with non-existent semaphore.
func TestSemaphoreOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_sem_open_notexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenSemaphore(mem, "nonexistent")
	if err == nil {
		t.Error("OpenSemaphore with non-existent name should fail")
	}
}

// TestSemaphoreOpenCrossProcess tests OpenSemaphore seeing state from creator.
func TestSemaphoreOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_sem_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem1, err := NewSemaphore(mem, "cross_sem", 3, 10)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// Open the same semaphore
	sem2, err := OpenSemaphore(mem, "cross_sem")
	if err != nil {
		t.Fatalf("OpenSemaphore failed: %v", err)
	}

	// Verify state is visible
	if sem2.Count() != 3 {
		t.Errorf("sem2.Count() = %d, want 3", sem2.Count())
	}
	if sem2.MaxCount() != 10 {
		t.Errorf("sem2.MaxCount() = %d, want 10", sem2.MaxCount())
	}

	// Acquire through sem2, verify sem1 sees it
	if !sem2.TryAcquire() {
		t.Error("sem2.TryAcquire should succeed")
	}
	if sem1.Count() != 2 {
		t.Errorf("sem1.Count() = %d after sem2 acquire, want 2", sem1.Count())
	}

	// Release through sem1, verify sem2 sees it
	if err := sem1.Release(); err != nil {
		t.Fatalf("sem1.Release failed: %v", err)
	}
	if sem2.Count() != 3 {
		t.Errorf("sem2.Count() = %d after sem1 release, want 3", sem2.Count())
	}
}

// TestSemaphoreBinaryMode tests binary semaphore (max_count=1).
func TestSemaphoreBinaryMode(t *testing.T) {
	name := fmt.Sprintf("/test_sem_binary_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Binary semaphore: max=1, initial=1 (available)
	sem, err := NewSemaphore(mem, "binary_sem", 1, 1)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// First acquire succeeds
	if !sem.TryAcquire() {
		t.Error("First TryAcquire should succeed")
	}
	if sem.Count() != 0 {
		t.Errorf("Count = %d, want 0", sem.Count())
	}

	// Second acquire fails (binary)
	if sem.TryAcquire() {
		t.Error("Second TryAcquire should fail (binary semaphore)")
	}

	// Release succeeds
	if err := sem.Release(); err != nil {
		t.Fatalf("Release failed: %v", err)
	}
	if sem.Count() != 1 {
		t.Errorf("Count after release = %d, want 1", sem.Count())
	}

	// Release again fails (would exceed max=1)
	if err := sem.Release(); err == nil {
		t.Error("Second Release should fail (would exceed max=1)")
	}
}

// TestSemaphoreUnboundedMode tests unbounded semaphore (max_count=0).
func TestSemaphoreUnboundedMode(t *testing.T) {
	name := fmt.Sprintf("/test_sem_unbounded_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	sem, err := NewSemaphore(mem, "unbounded_sem", 0, 0)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// Release many times (unbounded should allow it)
	for i := 0; i < 100; i++ {
		if err := sem.Release(); err != nil {
			t.Fatalf("Release %d failed: %v", i, err)
		}
	}
	if sem.Count() != 100 {
		t.Errorf("Count = %d, want 100", sem.Count())
	}
}

// TestSemaphoreConcurrentAcquire tests concurrent Acquire with limited permits.
func TestSemaphoreConcurrentAcquire(t *testing.T) {
	name := fmt.Sprintf("/test_sem_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// 5 permits, 10 goroutines try to acquire
	sem, err := NewSemaphore(mem, "concurrent_sem", 5, 5)
	if err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	var wg sync.WaitGroup

	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if sem.TryAcquire() {
				// Holding the permit briefly
				time.Sleep(5 * time.Millisecond)
				sem.Release()
			}
		}()
	}

	wg.Wait()

	if sem.Count() != 5 {
		t.Errorf("Count = %d, want 5 (all released)", sem.Count())
	}
}
