package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

// TestRWLockName tests the Name method.
func TestRWLockName(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_name_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "my_rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	if rwlock.Name() != "my_rwlock" {
		t.Errorf("Name() = %q, want my_rwlock", rwlock.Name())
	}
}

// TestRWLockReaderLock tests basic reader lock/unlock.
func TestRWLockReaderLock(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_read_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// Multiple readers should be able to lock concurrently
	rwlock.ReaderLock()
	rwlock.ReaderLock()
	rwlock.ReaderLock()

	// All three should be unlocked
	rwlock.ReaderUnlock()
	rwlock.ReaderUnlock()
	rwlock.ReaderUnlock()
}

// TestRWLockWriterLock tests basic writer lock/unlock.
func TestRWLockWriterLock(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_write_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// Writer can lock and unlock
	rwlock.WriterLock()
	rwlock.WriterUnlock()
}

// TestRWLockWriterBlocksReaders tests that writer blocks new readers.
func TestRWLockWriterBlocksReaders(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_writeblock_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// Writer holds the lock
	rwlock.WriterLock()

	// TryReaderLock should fail
	if rwlock.TryReaderLock() {
		t.Error("TryReaderLock should fail when writer holds the lock")
	}

	rwlock.WriterUnlock()

	// Now TryReaderLock should succeed
	if !rwlock.TryReaderLock() {
		t.Error("TryReaderLock should succeed after writer unlocks")
	}
	rwlock.ReaderUnlock()
}

// TestRWLockReaderBlocksWriter tests that readers block writer.
func TestRWLockReaderBlocksWriter(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_readblock_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// Reader holds the lock
	rwlock.ReaderLock()

	// TryWriterLock should fail
	if rwlock.TryWriterLock() {
		t.Error("TryWriterLock should fail when reader holds the lock")
	}

	rwlock.ReaderUnlock()

	// Now TryWriterLock should succeed
	if !rwlock.TryWriterLock() {
		t.Error("TryWriterLock should succeed after reader unlocks")
	}
	rwlock.WriterUnlock()
}

// TestRWLockTryReaderLock tests TryReaderLock success and failure.
func TestRWLockTryReaderLock(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_tryread_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// First TryReaderLock should succeed
	if !rwlock.TryReaderLock() {
		t.Error("First TryReaderLock should succeed")
	}

	// Second TryReaderLock should also succeed (readers are concurrent)
	if !rwlock.TryReaderLock() {
		t.Error("Second TryReaderLock should succeed (concurrent readers)")
	}

	// Unlock both
	rwlock.ReaderUnlock()
	rwlock.ReaderUnlock()
}

// TestRWLockTryWriterLock tests TryWriterLock success and failure.
func TestRWLockTryWriterLock(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_trywrite_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// First TryWriterLock should succeed
	if !rwlock.TryWriterLock() {
		t.Error("First TryWriterLock should succeed")
	}

	// Second TryWriterLock should fail (exclusive)
	if rwlock.TryWriterLock() {
		t.Error("Second TryWriterLock should fail when writer holds the lock")
	}

	rwlock.WriterUnlock()
}

// TestRWLockConcurrentReaders tests multiple concurrent readers.
func TestRWLockConcurrentReaders(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_concurrent_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	var wg sync.WaitGroup
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			rwlock.ReaderLock()
			// Simulate read work
			time.Sleep(time.Millisecond)
			rwlock.ReaderUnlock()
		}()
	}

	wg.Wait()
}

// TestRWLockWriterExclusive tests writer exclusivity with concurrent readers.
func TestRWLockWriterExclusive(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_exclusive_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock, err := NewRWLock(mem, "rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	var data int32
	var wg sync.WaitGroup

	// Start readers
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 10; j++ {
				rwlock.ReaderLock()
				_ = data // Read
				rwlock.ReaderUnlock()
				time.Sleep(time.Microsecond)
			}
		}()
	}

	// Start writers
	for i := 0; i < 2; i++ {
		wg.Add(1)
		go func(val int32) {
			defer wg.Done()
			for j := 0; j < 5; j++ {
				rwlock.WriterLock()
				data = val
				rwlock.WriterUnlock()
				time.Sleep(time.Microsecond)
			}
		}(int32(i + 1))
	}

	wg.Wait()
}

// TestRWLockNewNameTooLong tests NewRWLock with name too long.
func TestRWLockNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_longname_%d", os.Getpid())
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

	_, err = NewRWLock(mem, longName)
	if err == nil {
		t.Error("NewRWLock with name too long should fail")
	}
}

// TestRWLockNewAlreadyExists tests NewRWLock when name already exists.
func TestRWLockNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_exists_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	if _, err := NewRWLock(mem, "dup_rwlock"); err != nil {
		t.Fatalf("First NewRWLock failed: %v", err)
	}

	_, err = NewRWLock(mem, "dup_rwlock")
	if err == nil {
		t.Error("NewRWLock with existing name should fail")
	}
}

// TestRWLockOpenNotExists tests OpenRWLock with non-existent rwlock.
func TestRWLockOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/rw_onx_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenRWLock(mem, "nonexistent")
	if err == nil {
		t.Error("OpenRWLock with non-existent name should fail")
	}
}

// TestRWLockOpenCrossProcess tests OpenRWLock seeing state from creator.
func TestRWLockOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/test_rwlock_cross_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	rwlock1, err := NewRWLock(mem, "cross_rwlock")
	if err != nil {
		t.Fatalf("NewRWLock failed: %v", err)
	}

	// Lock writer through rwlock1
	rwlock1.WriterLock()

	// Open the same rwlock
	rwlock2, err := OpenRWLock(mem, "cross_rwlock")
	if err != nil {
		t.Fatalf("OpenRWLock failed: %v", err)
	}

	// TryReaderLock through rwlock2 should fail
	if rwlock2.TryReaderLock() {
		t.Error("rwlock2.TryReaderLock should fail when writer holds the lock")
	}

	// Unlock through rwlock1, verify rwlock2 sees it
	rwlock1.WriterUnlock()

	// TryReaderLock through rwlock2 should now succeed
	if !rwlock2.TryReaderLock() {
		t.Error("rwlock2.TryReaderLock should succeed after writer unlocks")
	}
	rwlock2.ReaderUnlock()
}
