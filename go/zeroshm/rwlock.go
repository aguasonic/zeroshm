package zeroshm

import (
	"fmt"
	"sync/atomic"
	"unsafe"
)

// RWLockStateSize is the size of the RWLock state in bytes.
// Layout: readers(4) + writer_active(4) = 8 bytes
const RWLockStateSize = 8

// RWLock is a read-write lock for shared memory.
//
// Allows multiple concurrent readers OR one exclusive writer.
// Optimized for read-heavy workloads where reads vastly outnumber writes.
//
// Binary layout:
//   - readers: int32 (atomic, offset 0) - number of active readers
//   - writer_active: int32 (atomic, offset 4) - 1 if writer active, 0 otherwise
//
// Example:
//
//	mem, _ := NewMemory("/data", 1024*1024, 64)
//	rwlock, _ := NewRWLock(mem, "data_lock")
//
//	// Many readers (concurrent)
//	rwlock.ReaderLock()
//	// ... read ...
//	rwlock.ReaderUnlock()
//
//	// Single writer (exclusive)
//	rwlock.WriterLock()
//	// ... write ...
//	rwlock.WriterUnlock()
type RWLock struct {
	memory       *Memory
	name         string
	offset       int
	readerMutex  *Mutex
	writerMutex  *Mutex
}

// NewRWLock creates a new read-write lock in shared memory.
func NewRWLock(memory *Memory, name string) (*RWLock, error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("rwlock '%s' already exists", name)
	}

	// Allocate state
	stateName := name + "_state"
	stateOffset, err := memory.Allocate(stateName, RWLockStateSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate rwlock state: %w", err)
	}

	// Initialize state (zeroed by default from allocation, but set explicitly)
	data := memory.Data()
	*(*int32)(unsafe.Pointer(&data[stateOffset+RwlockReadersOffset])) = 0        // readers = 0
	*(*int32)(unsafe.Pointer(&data[stateOffset+RwlockWriterActiveOffset])) = 0      // writer_active = 0

	// Create companion mutexes
	readerMutex, err := NewMutex(memory, name+"_rmtx")
	if err != nil {
		return nil, fmt.Errorf("failed to create reader mutex: %w", err)
	}

	writerMutex, err := NewMutex(memory, name+"_wmtx")
	if err != nil {
		return nil, fmt.Errorf("failed to create writer mutex: %w", err)
	}

	// Add marker entry
	_, err = memory.Allocate(name, 1)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate rwlock marker: %w", err)
	}

	return &RWLock{
		memory:      memory,
		name:        name,
		offset:      stateOffset,
		readerMutex: readerMutex,
		writerMutex: writerMutex,
	}, nil
}

// OpenRWLock opens an existing read-write lock in shared memory.
func OpenRWLock(memory *Memory, name string) (*RWLock, error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("rwlock '%s' not found", name)
	}

	// Find state
	stateEntry := memory.Find(name + "_state")
	if stateEntry == nil {
		return nil, fmt.Errorf("rwlock state not found for '%s'", name)
	}

	if int(stateEntry.Size) != RWLockStateSize {
		return nil, fmt.Errorf("invalid rwlock size: %d, expected %d", stateEntry.Size, RWLockStateSize)
	}

	// Open companion mutexes
	readerMutex, err := OpenMutex(memory, name+"_rmtx")
	if err != nil {
		return nil, fmt.Errorf("failed to open reader mutex: %w", err)
	}

	writerMutex, err := OpenMutex(memory, name+"_wmtx")
	if err != nil {
		return nil, fmt.Errorf("failed to open writer mutex: %w", err)
	}

	return &RWLock{
		memory:      memory,
		name:        name,
		offset:      int(stateEntry.Offset),
		readerMutex: readerMutex,
		writerMutex: writerMutex,
	}, nil
}

// readersPtr returns a pointer to the atomic readers counter.
func (r *RWLock) readersPtr() *int32 {
	return (*int32)(unsafe.Pointer(&r.memory.Data()[r.offset]))
}

// writerActivePtr returns a pointer to the atomic writer_active flag.
func (r *RWLock) writerActivePtr() *int32 {
	return (*int32)(unsafe.Pointer(&r.memory.Data()[r.offset+RwlockWriterActiveOffset]))
}

// ReaderLock acquires a read lock (shared access).
// Multiple threads can hold read locks simultaneously.
// Blocks if a writer currently holds the lock.
func (r *RWLock) ReaderLock() {
	r.readerMutex.Lock()

	// Wait for any active writer to finish
	for atomic.LoadInt32(r.writerActivePtr()) != 0 {
		r.readerMutex.Unlock()
		r.readerMutex.Lock()
	}

	atomic.AddInt32(r.readersPtr(), 1)
	r.readerMutex.Unlock()
}

// TryReaderLock tries to acquire a read lock without blocking.
// Returns true if the lock was acquired, false if a writer is active.
func (r *RWLock) TryReaderLock() bool {
	if !r.readerMutex.TryLock() {
		return false
	}

	if atomic.LoadInt32(r.writerActivePtr()) != 0 {
		r.readerMutex.Unlock()
		return false
	}

	atomic.AddInt32(r.readersPtr(), 1)
	r.readerMutex.Unlock()
	return true
}

// ReaderUnlock releases a read lock.
func (r *RWLock) ReaderUnlock() {
	atomic.AddInt32(r.readersPtr(), -1)
}

// WriterLock acquires a write lock (exclusive access).
// Only one writer can hold the lock. Blocks until all readers
// release their locks and no other writer holds the lock.
func (r *RWLock) WriterLock() {
	r.writerMutex.Lock()

	// Mark writer as active under readerMutex to prevent race
	r.readerMutex.Lock()
	atomic.StoreInt32(r.writerActivePtr(), 1)
	r.readerMutex.Unlock()

	// Wait for all readers to finish
	for atomic.LoadInt32(r.readersPtr()) > 0 {
		// Spin
	}
}

// TryWriterLock tries to acquire a write lock without blocking.
// Returns true if the lock was acquired, false if readers are active or another writer holds it.
func (r *RWLock) TryWriterLock() bool {
	if !r.writerMutex.TryLock() {
		return false
	}

	if !r.readerMutex.TryLock() {
		r.writerMutex.Unlock()
		return false
	}

	if atomic.LoadInt32(r.readersPtr()) > 0 {
		r.readerMutex.Unlock()
		r.writerMutex.Unlock()
		return false
	}

	atomic.StoreInt32(r.writerActivePtr(), 1)
	r.readerMutex.Unlock()
	return true
}

// WriterUnlock releases a write lock.
func (r *RWLock) WriterUnlock() {
	atomic.StoreInt32(r.writerActivePtr(), 0)
	r.writerMutex.Unlock()
}

// Destroy removes the RWLock and its resources from shared memory.
func (r *RWLock) Destroy() {
	if r.readerMutex != nil {
		r.readerMutex.Destroy()
	}
	if r.writerMutex != nil {
		r.writerMutex.Destroy()
	}
	_ = r.memory.Table().Remove(r.name + "_state")
	_ = r.memory.Table().Remove(r.name)
}

// Name returns the RWLock name.
func (r *RWLock) Name() string {
	return r.name
}
