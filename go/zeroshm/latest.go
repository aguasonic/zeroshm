package zeroshm

/*
#include <pthread.h>
#include <time.h>
#include <errno.h>
*/
import "C"

import (
	"encoding/binary"
	"errors"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// LatestHeaderSize is the size of metadata stored before the value+version payload.
// We store elem_size and aligned_size in the header for cross-language validation.
// Layout: elem_size (uint32) + aligned_size (uint32) = 8 bytes
const LatestHeaderSize = 8

// Latest is a single-producer, multi-reader shared memory value with change detection.
// The producer calls Set() to write a new value and increment the version.
// Consumers call Get() to read the current value, or WaitForChange() to block
// until the version changes (true OS-level blocking via pthread_cond_timedwait).
//
// T must be trivially copyable (no pointers, slices, maps, channels, or function fields).
// Go's type system cannot enforce this, so it is the caller's responsibility.
//
// Binary layout (after header):
//   - value: elem_size bytes (starts at header end)
//   - padding: to 8-byte alignment (if elem_size is not a multiple of 8)
//   - version: uint64 (atomic, starts at align_up(elem_size, 8))
//   - mutex: pthread_mutex_t (aligned, for blocking reads)
//   - cond: pthread_cond_t (aligned, for blocking reads)
//
// Total size = 8 (header) + align_up(elem_size, 8) + 8 (version) + aligned mutex + aligned cond
type Latest[T any] struct {
	memory      *Memory
	name        string
	offset      int // offset of header in shared memory
	elemSize    int
	payloadSize int // align_up(elemSize, 8) + 8 (version)
	mutex       *C.pthread_mutex_t // pointer to mutex in shared memory
	cond        *C.pthread_cond_t  // pointer to condvar in shared memory
}

// latestValueOffset returns the offset of the value field within the payload.
func (l *Latest[T]) latestValueOffset() int {
	return l.offset + LatestHeaderSize
}

// latestVersionOffset returns the offset of the version field in shared memory.
func (l *Latest[T]) latestVersionOffset() int {
	return l.latestValueOffset() + alignUp(l.elemSize, SectionAlign)
}

// LatestTotalSize computes the total size of a Latest[T] structure in shared memory:
// header(8) + align_up(elemSize, SectionAlign) + version(8) + aligned mutex + aligned cond
func LatestTotalSize(elemSize int) int {
	alignedSize := alignUp(elemSize, SectionAlign)
	versionEnd := LatestHeaderSize + alignedSize + 8
	mutexStart := alignUp(versionEnd, pthreadMutexAlign)
	condStart := alignUp(mutexStart+pthreadMutexSize, pthreadCondAlign)
	return condStart + pthreadCondSize
}

// LatestVersionOffset computes the offset of the version field from the base offset.
func LatestVersionOffset(offset int, elemSize int) int {
	alignedSize := alignUp(elemSize, SectionAlign)
	return offset + LatestHeaderSize + alignedSize
}

// LatestMutexOffset computes the offset of the pthread mutex from the base offset.
func LatestMutexOffset(offset int, elemSize int) int {
	alignedSize := alignUp(elemSize, SectionAlign)
	versionEnd := offset + LatestHeaderSize + alignedSize + 8
	return alignUp(versionEnd, pthreadMutexAlign)
}

// LatestCondOffset computes the offset of the pthread condvar from the base offset.
func LatestCondOffset(offset int, elemSize int) int {
	mutexOff := LatestMutexOffset(offset, elemSize)
	return alignUp(mutexOff+pthreadMutexSize, pthreadCondAlign)
}

// versionPtr returns a pointer to the atomic version in shared memory.
func (l *Latest[T]) versionPtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&l.memory.Data()[l.latestVersionOffset()]))
}

// valuePtr returns a pointer to the value in shared memory.
func (l *Latest[T]) valuePtr() unsafe.Pointer {
	return unsafe.Pointer(&l.memory.Data()[l.latestValueOffset()])
}

// NewLatest creates a new Latest value in shared memory.
// initialValue is written as the first value with version 1.
// Initializes the mutex and condvar with PTHREAD_PROCESS_SHARED for blocking.
func NewLatest[T any](memory *Memory, name string, initialValue T) (*Latest[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	if elemSize == 0 {
		return nil, errors.New("Latest does not support zero-size types")
	}

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("latest '%s' already exists", name)
	}

	alignedSize := alignUp(elemSize, SectionAlign)
	payloadSize := alignedSize + 8 // value + padding + version

	// Allocate space: full unified layout (header + value + version + mutex + cond)
	totalSize := LatestTotalSize(elemSize)
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	// Zero-initialize the entire region
	data := memory.Data()
	for i := offset; i < offset+totalSize; i++ {
		data[i] = 0
	}

	// Initialize header: elem_size + aligned_size
	binary.LittleEndian.PutUint32(data[offset:offset+4], uint32(elemSize))
	binary.LittleEndian.PutUint32(data[offset+4:offset+8], uint32(alignedSize))

	// Initialize mutex and condvar with PTHREAD_PROCESS_SHARED
	mutexOff := LatestMutexOffset(offset, elemSize)
	condOff := LatestCondOffset(offset, elemSize)
	initProcessSharedMutex(unsafe.Pointer(&data[mutexOff]))
	initProcessSharedCond(unsafe.Pointer(&data[condOff]))

	l := &Latest[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		elemSize:    elemSize,
		payloadSize: payloadSize,
		mutex:       (*C.pthread_mutex_t)(unsafe.Pointer(&data[mutexOff])),
		cond:        (*C.pthread_cond_t)(unsafe.Pointer(&data[condOff])),
	}

	// Lock mutex, write initial value, init version, unlock
	C.pthread_mutex_lock(l.mutex)
	*(*T)(l.valuePtr()) = initialValue
	atomic.StoreUint64(l.versionPtr(), 1)
	C.pthread_mutex_unlock(l.mutex)

	return l, nil
}

// OpenLatest opens an existing Latest value in shared memory.
// Locates the existing mutex and condvar in shared memory (no re-initialization).
func OpenLatest[T any](memory *Memory, name string) (*Latest[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("latest '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	if elemSize == 0 {
		return nil, errors.New("Latest does not support zero-size types")
	}

	offset := int(entry.Offset)
	data := memory.Data()

	// Read header
	storedElemSize := binary.LittleEndian.Uint32(data[offset : offset+4])
	storedAlignedSize := binary.LittleEndian.Uint32(data[offset+4 : offset+8])

	if int(storedElemSize) != elemSize {
		return nil, fmt.Errorf("type mismatch: expected elem_size %d, got %d", elemSize, storedElemSize)
	}

	// Validate size
	alignedSize := alignUp(elemSize, SectionAlign)
	if int(storedAlignedSize) != alignedSize {
		return nil, fmt.Errorf("alignment mismatch: expected %d, got %d", alignedSize, storedAlignedSize)
	}

	// Validate against unified layout size (includes mutex + cond)
	expectedSize := LatestTotalSize(elemSize)
	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type or old layout?)",
			expectedSize, entry.Size)
	}

	// Locate existing mutex and condvar in shared memory
	mutexOff := LatestMutexOffset(offset, elemSize)
	condOff := LatestCondOffset(offset, elemSize)

	return &Latest[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		elemSize:    elemSize,
		payloadSize: alignedSize + 8,
		mutex:       (*C.pthread_mutex_t)(unsafe.Pointer(&data[mutexOff])),
		cond:        (*C.pthread_cond_t)(unsafe.Pointer(&data[condOff])),
	}, nil
}

// readValueAndVersion reads the current value and version assuming the mutex is
// already held. Used by Get() and WaitForChange() to avoid double-locking.
func (l *Latest[T]) readValueAndVersion() (T, uint64) {
	version := atomic.LoadUint64(l.versionPtr())
	value := *(*T)(l.valuePtr())
	return value, version
}

// Get reads the current value and version. Safe for concurrent readers.
// Locks the mutex to ensure a consistent snapshot of value + version.
func (l *Latest[T]) Get() (T, uint64) {
	C.pthread_mutex_lock(l.mutex)
	defer C.pthread_mutex_unlock(l.mutex)
	return l.readValueAndVersion()
}

// Set writes a new value, increments the version, and wakes all blocked readers.
// Single producer only. Locks the mutex to protect the value + version update
// and broadcasts the condition variable to wake waiting readers.
func (l *Latest[T]) Set(value T) {
	C.pthread_mutex_lock(l.mutex)

	// Write the value
	*(*T)(l.valuePtr()) = value

	// Atomically increment version with release ordering
	for {
		current := atomic.LoadUint64(l.versionPtr())
		if atomic.CompareAndSwapUint64(l.versionPtr(), current, current+1) {
			break
		}
	}

	// Wake all blocked readers
	C.pthread_cond_broadcast(l.cond)

	C.pthread_mutex_unlock(l.mutex)
}

// Version returns the current version number. Safe for concurrent readers.
func (l *Latest[T]) Version() uint64 {
	return atomic.LoadUint64(l.versionPtr())
}

// WaitForChange blocks until the version is greater than lastVersion or timeout expires.
// Returns (value, version, changed) where changed is true if the version advanced.
// Uses pthread_cond_timedwait for true OS-level blocking (zero CPU while waiting).
func (l *Latest[T]) WaitForChange(lastVersion uint64, timeout time.Duration) (T, uint64, bool) {
	C.pthread_mutex_lock(l.mutex)
	defer C.pthread_mutex_unlock(l.mutex)

	// Convert timeout to an absolute timespec for pthread_cond_timedwait.
	// pthread_cond_timedwait expects CLOCK_REALTIME.
	// time.Duration is int64 nanoseconds, so the remainder is already in ns.
	var ts C.struct_timespec
	C.clock_gettime(C.CLOCK_REALTIME, &ts)
	ts.tv_sec += C.time_t(timeout / time.Second)
	ts.tv_nsec += C.long(timeout % time.Second)
	if ts.tv_nsec >= 1000000000 {
		ts.tv_sec += ts.tv_nsec / 1000000000
		ts.tv_nsec %= 1000000000
	}

	for {
		value, version := l.readValueAndVersion()
		if version > lastVersion {
			return value, version, true
		}

		ret := C.pthread_cond_timedwait(l.cond, l.mutex, &ts)
		if ret == C.ETIMEDOUT {
			// Timeout -- return current value
			curValue, curVersion := l.readValueAndVersion()
			return curValue, curVersion, false
		}
		// Spurious wakeup or broadcast -- loop and re-check version
	}
}

// Name returns the Latest value name.
func (l *Latest[T]) Name() string {
	return l.name
}

// Destroy removes the Latest value from the shared memory table.
// Destroys the mutex and condvar before removing the entry.
func (l *Latest[T]) Destroy() {
	if l.mutex != nil {
		destroyMutex(unsafe.Pointer(l.mutex))
	}
	if l.cond != nil {
		destroyCond(unsafe.Pointer(l.cond))
	}
	l.memory.Table().Remove(l.name)
}
