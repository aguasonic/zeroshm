package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// Lazy states.
const (
	LazyStateNotComputed uint32 = 0
	LazyStateComputing   uint32 = 1
	LazyStateComputed    uint32 = 2
	LazyStateError       uint32 = 3
)

// LazyHeaderFixedSize is the size of the fixed portion of the lazy header.
// Layout: state(4) + compute_count(4) + completion_time(8) = 16 bytes, plus value(T) and error_msg[256]
const LazyHeaderFixedSize = 16

// LazyErrorMsgSize is the size of the error message buffer.
const LazyErrorMsgSize = 256

// LazyNotReady is returned when trying to peek a lazy value that hasn't been computed.
var LazyNotReady = errors.New("lazy value not computed yet")

// Lazy[T] is a lazy evaluation container that computes its value at most once.
// The first caller to Force() with a compute function triggers the computation;
// subsequent callers receive the cached result.
//
// Binary layout:
//   - state: uint32 (atomic, NOT_COMPUTED/COMPUTING/COMPUTED/ERROR)
//   - compute_count: uint32 (atomic, number of times computed)
//   - completion_time: uint64 (time of completion)
//   - value: T
//   - error_msg: [256]byte
type Lazy[T Numeric] struct {
	memory       *Memory
	name         string
	offset       int
	valueOffset  int
	errorOffset  int
}

// NewLazy creates a new lazy container in shared memory (initially not computed).
func NewLazy[T Numeric](memory *Memory, name string) (*Lazy[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("lazy '%s' already exists", name)
	}

	totalSize := LazyHeaderFixedSize + elemSize + LazyErrorMsgSize
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	data := memory.Data()

	// Initialize header
	binary.LittleEndian.PutUint32(data[offset+LazyStateOffset:offset+LazyStateOffset+U32Size], LazyStateNotComputed) // state
	binary.LittleEndian.PutUint32(data[offset+LazyComputeCountOffset:offset+LazyComputeCountOffset+U32Size], 0)                   // compute_count
	binary.LittleEndian.PutUint64(data[offset+LazyCompletionTimeOffset:offset+LazyCompletionTimeOffset+U64Size], 0)                  // completion_time

	// Zero the value and error_msg
	valueOffset := offset + LazyHeaderFixedSize
	errorOffset := valueOffset + elemSize
	for i := valueOffset; i < valueOffset+elemSize+LazyErrorMsgSize; i++ {
		data[i] = 0
	}

	return &Lazy[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		valueOffset: valueOffset,
		errorOffset: errorOffset,
	}, nil
}

// NewLazyWithInitialValue creates a new lazy container that is already computed with the given value.
func NewLazyWithInitialValue[T Numeric](memory *Memory, name string, value T) (*Lazy[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("lazy '%s' already exists", name)
	}

	totalSize := LazyHeaderFixedSize + elemSize + LazyErrorMsgSize
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	data := memory.Data()

	// Initialize header as computed
	binary.LittleEndian.PutUint32(data[offset+LazyStateOffset:offset+LazyStateOffset+U32Size], LazyStateComputed) // state
	binary.LittleEndian.PutUint32(data[offset+LazyComputeCountOffset:offset+LazyComputeCountOffset+U32Size], 1)               // compute_count
	binary.LittleEndian.PutUint64(data[offset+LazyCompletionTimeOffset:offset+LazyCompletionTimeOffset+U64Size], uint64(time.Now().UnixNano()))

	// Set the value
	valueOffset := offset + LazyHeaderFixedSize
	errorOffset := valueOffset + elemSize
	for i := valueOffset; i < valueOffset+elemSize+LazyErrorMsgSize; i++ {
		data[i] = 0
	}
	*(*T)(unsafe.Pointer(&data[valueOffset])) = value

	return &Lazy[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		valueOffset: valueOffset,
		errorOffset: errorOffset,
	}, nil
}

// OpenLazy opens an existing lazy container in shared memory.
func OpenLazy[T Numeric](memory *Memory, name string) (*Lazy[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("lazy '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))
	expectedSize := LazyHeaderFixedSize + elemSize + LazyErrorMsgSize

	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)", expectedSize, entry.Size)
	}

	offset := int(entry.Offset)
	return &Lazy[T]{
		memory:      memory,
		name:        name,
		offset:      offset,
		valueOffset: offset + LazyHeaderFixedSize,
		errorOffset: offset + LazyHeaderFixedSize + elemSize,
	}, nil
}

// statePtr returns a pointer to the atomic state.
func (l *Lazy[T]) statePtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&l.memory.Data()[l.offset]))
}

// computeCountPtr returns a pointer to the compute_count counter.
func (l *Lazy[T]) computeCountPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&l.memory.Data()[l.offset+LazyComputeCountOffset]))
}

// completionTimePtr returns a pointer to the completion_time field.
func (l *Lazy[T]) completionTimePtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&l.memory.Data()[l.offset+LazyCompletionTimeOffset]))
}

// valuePtr returns a pointer to the value field.
func (l *Lazy[T]) valuePtr() *T {
	return (*T)(unsafe.Pointer(&l.memory.Data()[l.valueOffset]))
}

// errorStr reads the error message from shared memory.
func (l *Lazy[T]) errorStr() string {
	data := l.memory.Data()
	end := l.errorOffset + LazyErrorMsgSize
	if end > len(data) {
		end = len(data)
	}
	for i := l.errorOffset; i < end; i++ {
		if data[i] == 0 {
			return string(data[l.errorOffset:i])
		}
	}
	return string(data[l.errorOffset:end])
}

// Force computes the value using the provided function if not already computed.
// Only the first caller actually computes; others wait for the result.
// Returns the value and nil on success, or error if computation failed.
func (l *Lazy[T]) Force(compute func() (T, error)) (T, error) {
	statePtr := l.statePtr()

	// Try to claim computation
	expected := LazyStateNotComputed
	if atomic.CompareAndSwapUint32(statePtr, expected, LazyStateComputing) {
		// We get to compute it
		result, err := compute()
		if err != nil {
			// Store error
			data := l.memory.Data()
			copyLen := len(err.Error())
			if copyLen > LazyErrorMsgSize-1 {
				copyLen = LazyErrorMsgSize - 1
			}
			copy(data[l.errorOffset:l.errorOffset+copyLen], err.Error())
			data[l.errorOffset+copyLen] = 0
			atomic.StoreUint32(statePtr, LazyStateError)
			return result, err
		}

		// Store result
		*l.valuePtr() = result
		atomic.StoreUint64(l.completionTimePtr(), uint64(time.Now().UnixNano()))
		atomic.StoreUint32(statePtr, LazyStateComputed)
		atomic.AddUint32(l.computeCountPtr(), 1)
		return result, nil
	}

	// Wait for computation by another goroutine
	for {
		state := atomic.LoadUint32(statePtr)
		if state == LazyStateComputed {
			return *l.valuePtr(), nil
		}
		if state == LazyStateError {
			return *l.valuePtr(), errors.New(l.errorStr())
		}
		time.Sleep(time.Microsecond)
	}
}

// Get returns the value, blocking until it's computed.
// The caller must have called Force() from some goroutine.
func (l *Lazy[T]) Get() (T, error) {
	var zero T

	for {
		state := atomic.LoadUint32(l.statePtr())
		if state == LazyStateComputed {
			return *l.valuePtr(), nil
		}
		if state == LazyStateError {
			return zero, errors.New(l.errorStr())
		}
		time.Sleep(time.Microsecond)
	}
}

// Peek returns the value if already computed, without blocking.
// Returns the value and nil if computed, or LazyNotReady if not yet computed.
func (l *Lazy[T]) Peek() (T, error) {
	var zero T
	state := atomic.LoadUint32(l.statePtr())

	if state == LazyStateComputed {
		return *l.valuePtr(), nil
	}
	if state == LazyStateError {
		return zero, errors.New(l.errorStr())
	}
	return zero, LazyNotReady
}

// IsComputed returns true if the lazy value has been computed.
func (l *Lazy[T]) IsComputed() bool {
	state := atomic.LoadUint32(l.statePtr())
	return state == LazyStateComputed
}

// State returns the current state of the lazy value.
func (l *Lazy[T]) State() uint32 {
	return atomic.LoadUint32(l.statePtr())
}

// ComputeCount returns how many times the value has been computed.
func (l *Lazy[T]) ComputeCount() int {
	return int(atomic.LoadUint32(l.computeCountPtr()))
}

// Reset marks the lazy value as not computed, allowing recomputation.
// Returns true if it was reset, false if it wasn't computed.
func (l *Lazy[T]) Reset() bool {
	statePtr := l.statePtr()
	expected := LazyStateComputed
	return atomic.CompareAndSwapUint32(statePtr, expected, LazyStateNotComputed)
}

// Name returns the lazy value name.
func (l *Lazy[T]) Name() string {
	return l.name
}

// Destroy removes the lazy value from the shared memory table.
func (l *Lazy[T]) Destroy() {
	l.memory.Table().Remove(l.name)
}
