package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// Future states.
const (
	FutureStatePending   uint32 = 0
	FutureStateComputing uint32 = 1
	FutureStateReady     uint32 = 2
	FutureStateError     uint32 = 3
)

// FutureHeaderSize is the size of the fixed portion of the future header.
// Layout: state(4) + waiters(4) + completion_time(8) = 16 bytes, plus value(T) and error_msg[256]
const FutureHeaderFixedSize = 16

// FutureErrorMsgSize is the size of the error message buffer.
const FutureErrorMsgSize = 256

// FutureNotReady is returned when trying to get a value from a future that isn't ready.
var FutureNotReady = errors.New("future is not ready")

// FutureAlreadySet is returned when trying to set a value on an already-resolved future.
var FutureAlreadySet = errors.New("future is already set")

// Future is a shared memory future for asynchronous computation results.
// Enables one process to compute a value while others wait for the result.
//
// Binary layout:
//   - state: uint32 (atomic, PENDING/COMPUTING/READY/ERROR)
//   - waiters: uint32 (atomic, number of waiting consumers)
//   - completion_time: uint64 (steady clock time since completion)
//   - value: T
//   - error_msg: [256]byte
type Future[T Numeric] struct {
	memory       *Memory
	name         string
	offset       int
	valueOffset  int
	errorOffset  int
}

// NewFuture creates a new future in shared memory.
func NewFuture[T Numeric](memory *Memory, name string) (*Future[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("future '%s' already exists", name)
	}

	totalSize := FutureHeaderFixedSize + elemSize + FutureErrorMsgSize
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	data := memory.Data()

	// Initialize header
	binary.LittleEndian.PutUint32(data[offset+FutureStateOffset:offset+FutureStateOffset+U32Size], FutureStatePending) // state
	binary.LittleEndian.PutUint32(data[offset+FutureWaitersOffset:offset+FutureWaitersOffset+U32Size], 0)                // waiters
	binary.LittleEndian.PutUint64(data[offset+FutureCompletionTimeOffset:offset+FutureCompletionTimeOffset+U64Size], 0)               // completion_time

	// Zero the value and error_msg
	valueOffset := offset + FutureHeaderFixedSize
	errorOffset := valueOffset + elemSize
	for i := valueOffset; i < valueOffset+elemSize+FutureErrorMsgSize; i++ {
		data[i] = 0
	}

	return &Future[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		valueOffset: valueOffset,
		errorOffset: errorOffset,
	}, nil
}

// OpenFuture opens an existing future in shared memory.
func OpenFuture[T Numeric](memory *Memory, name string) (*Future[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("future '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))
	expectedSize := FutureHeaderFixedSize + elemSize + FutureErrorMsgSize

	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)", expectedSize, entry.Size)
	}

	offset := int(entry.Offset)
	return &Future[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		valueOffset: offset + FutureHeaderFixedSize,
		errorOffset: offset + FutureHeaderFixedSize + elemSize,
	}, nil
}

// statePtr returns a pointer to the atomic state in the header.
func (f *Future[T]) statePtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&f.memory.Data()[f.offset]))
}

// waitersPtr returns a pointer to the atomic waiters counter.
func (f *Future[T]) waitersPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&f.memory.Data()[f.offset+4]))
}

// completionTimePtr returns a pointer to the completion_time field.
func (f *Future[T]) completionTimePtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&f.memory.Data()[f.offset+8]))
}

// valuePtr returns a pointer to the value field.
func (f *Future[T]) valuePtr() *T {
	return (*T)(unsafe.Pointer(&f.memory.Data()[f.valueOffset]))
}

// errorStr reads the error message from shared memory.
func (f *Future[T]) errorStr() string {
	data := f.memory.Data()
	end := f.errorOffset + FutureErrorMsgSize
	if end > len(data) {
		end = len(data)
	}
	// Find null terminator
	for i := f.errorOffset; i < end; i++ {
		if data[i] == 0 {
			return string(data[f.errorOffset:i])
		}
	}
	return string(data[f.errorOffset:end])
}

// SetValue sets the future's result value. Returns true on success, false if already resolved.
func (f *Future[T]) SetValue(value T) bool {
	statePtr := f.statePtr()
	expected := FutureStatePending
	if !atomic.CompareAndSwapUint32(statePtr, expected, FutureStateComputing) {
		return false
	}

	// Store the value
	*f.valuePtr() = value

	// Record completion time
	now := time.Now().UnixNano()
	atomic.StoreUint64(f.completionTimePtr(), uint64(now))

	// Mark as ready
	atomic.StoreUint32(statePtr, FutureStateReady)

	return true
}

// SetError sets the future to an error state. Returns true on success, false if already resolved.
func (f *Future[T]) SetError(msg string) bool {
	statePtr := f.statePtr()
	expected := FutureStatePending
	if !atomic.CompareAndSwapUint32(statePtr, expected, FutureStateError) {
		return false
	}

	// Store error message
	data := f.memory.Data()
	copyLen := len(msg)
	if copyLen > FutureErrorMsgSize-1 {
		copyLen = FutureErrorMsgSize - 1
	}
	copy(data[f.errorOffset:f.errorOffset+copyLen], msg)
	data[f.errorOffset+copyLen] = 0

	// Record completion time
	now := time.Now().UnixNano()
	atomic.StoreUint64(f.completionTimePtr(), uint64(now))

	return true
}

// Get returns the future's result, blocking until it's ready.
// Returns the value and nil on success, or error details if the future resolved with an error.
func (f *Future[T]) Get() (T, error) {
	var zero T

	// Wait until ready or error
	for {
		state := atomic.LoadUint32(f.statePtr())
		if state == FutureStateReady || state == FutureStateError {
			break
		}
		time.Sleep(time.Microsecond)
	}

	state := atomic.LoadUint32(f.statePtr())
	if state == FutureStateError {
		return zero, errors.New(f.errorStr())
	}

	return *f.valuePtr(), nil
}

// TryGet returns the value if ready without blocking.
// Returns the value and nil if ready, FutureNotReady if not yet resolved, or error if resolved with error.
func (f *Future[T]) TryGet() (T, error) {
	var zero T
	state := atomic.LoadUint32(f.statePtr())

	if state == FutureStateReady {
		return *f.valuePtr(), nil
	}
	if state == FutureStateError {
		return zero, errors.New(f.errorStr())
	}
	return zero, FutureNotReady
}

// GetTimeout returns the value with a timeout.
// Returns the value and true if ready within timeout, or zero and false on timeout.
// If the future resolved with an error, returns the error.
func (f *Future[T]) GetTimeout(timeout time.Duration) (T, bool, error) {
	var zero T
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		state := atomic.LoadUint32(f.statePtr())
		if state == FutureStateReady {
			return *f.valuePtr(), true, nil
		}
		if state == FutureStateError {
			return zero, true, errors.New(f.errorStr())
		}
		time.Sleep(time.Microsecond)
	}

	return zero, false, nil
}

// IsReady returns true if the future has resolved (ready or error).
func (f *Future[T]) IsReady() bool {
	state := atomic.LoadUint32(f.statePtr())
	return state == FutureStateReady || state == FutureStateError
}

// State returns the current state of the future.
func (f *Future[T]) State() uint32 {
	return atomic.LoadUint32(f.statePtr())
}

// Waiters returns the current number of waiters.
func (f *Future[T]) Waiters() int {
	return int(atomic.LoadUint32(f.waitersPtr()))
}

// Name returns the future name.
func (f *Future[T]) Name() string {
	return f.name
}

// Destroy removes the future from the shared memory table.
func (f *Future[T]) Destroy() {
	f.memory.Table().Remove(f.name)
}
