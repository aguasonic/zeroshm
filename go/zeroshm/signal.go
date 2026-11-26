package zeroshm

import (
	"encoding/binary"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// SignalStateSize is the size of the signal state header in bytes (version only).
// The value is stored after the version, so total size is 8 + sizeof(T).
const SignalStateSize = 8

// Signal is a reactive signal for fine-grained reactivity across processes.
//
// A Signal[T] stores a value that can be observed for changes. When the value
// changes, the version number increments, allowing other processes to detect
// and react to changes.
//
// Binary layout:
//   - version: uint64 (8 bytes, atomic) - increments on each change
//   - value: T (sizeof(T) bytes)
//
// Example:
//
//	mem, _ := NewMemory("/data", 1024*1024, 64)
//	sig, _ := NewSignal(mem, "counter", int32(0))
//
//	// Writer
//	sig.Set(42)
//
//	// Reader - reactive
//	lastVersion := sig.Version()
//	sig.WaitForChange(lastVersion, time.Second)
//	fmt.Println("New value:", sig.Get())
type Signal[T Numeric] struct {
	memory *Memory
	name   string
	offset int
	mutex  *Mutex
}

// NewSignal creates a new signal in shared memory.
// initial_value is the starting value (only used when creating).
func NewSignal[T Numeric](memory *Memory, name string, initialValue T) (*Signal[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("signal '%s' already exists", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))
	totalSize := SignalStateSize + elemSize

	// Allocate state
	stateName := name + "_state"
	stateOffset, err := memory.Allocate(stateName, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate signal state: %w", err)
	}

	// Initialize state
	data := memory.Data()
	binary.LittleEndian.PutUint64(data[stateOffset:], 0) // version = 0
	copy(data[stateOffset+SignalStateSize:], unsafe.Slice((*byte)(unsafe.Pointer(&initialValue)), elemSize))

	// Create companion mutex
	mutex, err := NewMutex(memory, name+"_mtx")
	if err != nil {
		return nil, fmt.Errorf("failed to create signal mutex: %w", err)
	}

	// Add marker entry
	_, err = memory.Allocate(name, 1)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate signal marker: %w", err)
	}

	return &Signal[T]{
		memory: memory,
		name:   name,
		offset: stateOffset,
		mutex:  mutex,
	}, nil
}

// OpenSignal opens an existing signal in shared memory.
func OpenSignal[T Numeric](memory *Memory, name string) (*Signal[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("signal '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))
	expectedSize := SignalStateSize + elemSize

	// Find state
	stateEntry := memory.Find(name + "_state")
	if stateEntry == nil {
		return nil, fmt.Errorf("signal state not found for '%s'", name)
	}

	if int(stateEntry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)",
			expectedSize, stateEntry.Size)
	}

	// Open companion mutex
	mutex, err := OpenMutex(memory, name+"_mtx")
	if err != nil {
		return nil, fmt.Errorf("failed to open signal mutex: %w", err)
	}

	return &Signal[T]{
		memory: memory,
		name:   name,
		offset: int(stateEntry.Offset),
		mutex:  mutex,
	}, nil
}

// versionPtr returns a pointer to the atomic version counter.
func (s *Signal[T]) versionPtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&s.memory.Data()[s.offset]))
}

// valuePtr returns a pointer to the value.
func (s *Signal[T]) valuePtr() *T {
	return (*T)(unsafe.Pointer(&s.memory.Data()[s.offset+SignalStateSize]))
}

// Get returns the current value.
func (s *Signal[T]) Get() T {
	s.mutex.Lock()
	value := *s.valuePtr()
	s.mutex.Unlock()
	return value
}

// Set sets a new value and increments the version.
// Triggers all registered callbacks in this process.
func (s *Signal[T]) Set(newValue T) {
	s.mutex.Lock()
	*s.valuePtr() = newValue
	atomic.AddUint64(s.versionPtr(), 1)
	s.mutex.Unlock()
}

// Update atomically applies a function to the current value.
func (s *Signal[T]) Update(fn func(T) T) {
	s.mutex.Lock()
	newValue := fn(*s.valuePtr())
	*s.valuePtr() = newValue
	atomic.AddUint64(s.versionPtr(), 1)
	s.mutex.Unlock()
}

// Version returns the current version number.
// Version increments on each Set/Update.
func (s *Signal[T]) Version() uint64 {
	return atomic.LoadUint64(s.versionPtr())
}

// HasChanged returns true if the version has changed since oldVersion.
func (s *Signal[T]) HasChanged(oldVersion uint64) bool {
	return s.Version() != oldVersion
}

// WaitForChange blocks until the signal's version changes from oldVersion.
// Returns true if the version changed, false if the timeout expired.
func (s *Signal[T]) WaitForChange(oldVersion uint64, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		if s.HasChanged(oldVersion) {
			return true
		}
		time.Sleep(100 * time.Microsecond)
	}

	return false
}

// Destroy removes the signal and its resources from shared memory.
func (s *Signal[T]) Destroy() {
	if s.mutex != nil {
		s.mutex.Destroy()
	}
	_ = s.memory.Table().Remove(s.name + "_state")
	_ = s.memory.Table().Remove(s.name)
}

// Name returns the signal name.
func (s *Signal[T]) Name() string {
	return s.name
}
