package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestMutexName tests the Name method.
func TestMutexName(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "my_mutex")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	if mtx.Name() != "my_mutex" {
		t.Errorf("Name() = %q, want my_mutex", mtx.Name())
	}
}

// TestMutexTryLock tests the TryLock method.
func TestMutexTryLock(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_trylock_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "trylock_mutex")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	// First TryLock should succeed
	if !mtx.TryLock() {
		t.Error("First TryLock should succeed")
	}

	// Second TryLock should fail (already locked)
	if mtx.TryLock() {
		t.Error("Second TryLock should fail when mutex is locked")
	}

	// Unlock
	mtx.Unlock()

	// TryLock should succeed again
	if !mtx.TryLock() {
		t.Error("TryLock after Unlock should succeed")
	}
	mtx.Unlock()
}

// TestMutexIsLocked tests the IsLocked method.
func TestMutexIsLocked(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_islocked_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "islocked_mutex")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	// Initially not locked
	if mtx.IsLocked() {
		t.Error("IsLocked() should be false initially")
	}

	// Lock it
	mtx.Lock()
	if !mtx.IsLocked() {
		t.Error("IsLocked() should be true after Lock()")
	}

	// Unlock it
	mtx.Unlock()
	if mtx.IsLocked() {
		t.Error("IsLocked() should be false after Unlock()")
	}
}

// TestMutexLockTimeoutSuccess tests LockTimeout with success.
func TestMutexLockTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_lt_ok_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "timeout_ok")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	// Should succeed immediately (unlocked)
	if !mtx.LockTimeout(100 * time.Millisecond) {
		t.Error("LockTimeout should succeed when mutex is unlocked")
	}
	if !mtx.IsLocked() {
		t.Error("Mutex should be locked after LockTimeout")
	}
	mtx.Unlock()
}

// TestMutexLockTimeoutFail tests LockTimeout with timeout.
func TestMutexLockTimeoutFail(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_lt_fail_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "timeout_fail")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	// Lock it first
	mtx.Lock()

	// LockTimeout should fail (mutex is locked, no one unlocks)
	start := time.Now()
	if mtx.LockTimeout(50 * time.Millisecond) {
		t.Error("LockTimeout should fail when mutex is locked")
	}
	elapsed := time.Since(start)
	if elapsed < 40*time.Millisecond {
		t.Errorf("LockTimeout returned too quickly: %v", elapsed)
	}

	mtx.Unlock()
}

// TestMutexNewNameTooLong tests NewMutex with name too long.
func TestMutexNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_longname_%d", os.Getpid())
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

	_, err = NewMutex(mem, longName)
	if err == nil {
		t.Error("NewMutex with name too long should fail")
	}
}

// TestMutexNewAlreadyExists tests NewMutex when name already exists.
func TestMutexNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewMutex(mem, "dup_mutex"); err != nil {
		t.Fatalf("First NewMutex failed: %v", err)
	}

	_, err = NewMutex(mem, "dup_mutex")
	if err == nil {
		t.Error("NewMutex with existing name should fail")
	}
}

// TestMutexOpenNotExists tests OpenMutex with non-existent mutex.
func TestMutexOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_open_notexist_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenMutex(mem, "nonexistent")
	if err == nil {
		t.Error("OpenMutex with non-existent name should fail")
	}
}

// TestMutexOpenNotBinary tests OpenMutex on a non-binary semaphore.
func TestMutexOpenNotBinary(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_not_binary_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Create a counting semaphore (max=5), not a mutex
	if _, err := NewSemaphore(mem, "counting_sem", 3, 5); err != nil {
		t.Fatalf("NewSemaphore failed: %v", err)
	}

	// OpenMutex should fail because it's not a binary semaphore
	_, err = OpenMutex(mem, "counting_sem")
	if err == nil {
		t.Error("OpenMutex on non-binary semaphore should fail")
	}
}

// TestMutexOpenCrossProcess tests OpenMutex seeing state from creator.
func TestMutexOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx1, err := NewMutex(mem, "cross_mutex")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	// Lock through mtx1
	mtx1.Lock()
	if !mtx1.IsLocked() {
		t.Error("mtx1 should be locked")
	}

	// Open the same mutex
	mtx2, err := OpenMutex(mem, "cross_mutex")
	if err != nil {
		t.Fatalf("OpenMutex failed: %v", err)
	}

	// Verify state is visible
	if !mtx2.IsLocked() {
		t.Error("mtx2 should see mutex as locked")
	}

	// TryLock through mtx2 should fail
	if mtx2.TryLock() {
		t.Error("mtx2.TryLock should fail when mutex is locked")
	}

	// Unlock through mtx1, verify mtx2 sees it
	mtx1.Unlock()
	if mtx2.IsLocked() {
		t.Error("mtx2 should see mutex as unlocked")
	}

	// TryLock through mtx2 should now succeed
	if !mtx2.TryLock() {
		t.Error("mtx2.TryLock should succeed after unlock")
	}
	mtx2.Unlock()
}

// TestMutexConcurrentLock tests concurrent Lock with mutex protection.
func TestMutexConcurrentLock(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "concurrent_mutex")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	// Shared counter protected by mutex
	var counter int32

	var wg sync.WaitGroup
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 100; j++ {
				mtx.Lock()
				counter++
				mtx.Unlock()
			}
		}()
	}

	wg.Wait()

	if counter != 1000 {
		t.Errorf("Counter = %d, want 1000 (data race or mutex failure)", counter)
	}
}

// TestMutexLockUnlockSequence tests multiple lock/unlock cycles.
func TestMutexLockUnlockSequence(t *testing.T) {
	name := fmt.Sprintf("/test_mutex_seq_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	mtx, err := NewMutex(mem, "seq_mutex")
	if err != nil {
		t.Fatalf("NewMutex failed: %v", err)
	}

	for i := 0; i < 10; i++ {
		mtx.Lock()
		if !mtx.IsLocked() {
			t.Errorf("Iteration %d: mutex should be locked", i)
		}
		mtx.Unlock()
		if mtx.IsLocked() {
			t.Errorf("Iteration %d: mutex should be unlocked", i)
		}
	}
}
