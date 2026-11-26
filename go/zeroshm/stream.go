package zeroshm

import (
	"encoding/binary"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// StreamHeaderSize is the size of the stream header in bytes.
// Layout: sequence(8) + subscribers(4) + closed(4) + buffer_capacity(4) + padding(4) + transform_name[32] = 56
const StreamHeaderSize = 56

// Stream is a reactive stream for event-driven data processing in shared memory.
// Built on Ring[T] for backpressure handling. Supports multiple producers and consumers.
//
// Binary layout:
//   - Header: sequence(8) + subscribers(4) + closed(4) + buffer_capacity(4) + padding(4) + transform_name[32]
//   - Buffer: Ring[T] named `<name>_buffer`
type Stream[T Numeric] struct {
	memory   *Memory
	name     string
	offset   int // offset of header
	buffer   *Ring[T]
	capacity int
}

// NewStream creates a new stream in shared memory.
// capacity is the number of elements the ring buffer can hold.
func NewStream[T Numeric](memory *Memory, name string, capacity int) (*Stream[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	if capacity <= 0 {
		return nil, fmt.Errorf("capacity must be positive")
	}

	// Check if already exists
	if entry := memory.Find(name + "_header"); entry != nil {
		return nil, fmt.Errorf("stream '%s' already exists", name)
	}

	// Allocate header
	offset, err := memory.Allocate(name+"_header", StreamHeaderSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate header: %w", err)
	}

	data := memory.Data()

	// Initialize header
	binary.LittleEndian.PutUint64(data[offset+StreamSequenceOffset:offset+StreamSequenceOffset+U64Size], 0)     // sequence
	binary.LittleEndian.PutUint32(data[offset+StreamSubscribersOffset:offset+StreamSubscribersOffset+U32Size], 0)  // subscribers
	binary.LittleEndian.PutUint32(data[offset+StreamClosedOffset:offset+StreamClosedOffset+U32Size], 0) // closed = false
	binary.LittleEndian.PutUint32(data[offset+StreamBufferCapacityOffset:offset+StreamBufferCapacityOffset+U32Size], uint32(capacity)) // buffer_capacity
	binary.LittleEndian.PutUint32(data[offset+StreamPaddingOffset:offset+StreamPaddingOffset+U32Size], 0) // padding
	// Zero transform_name (offset 24, size 32)
	for i := 24; i < 56; i++ {
		data[offset+i] = 0
	}

	s := &Stream[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capacity,
	}

	// Create ring buffer
	ring, err := NewRing[T](memory, name+"_buffer", capacity)
	if err != nil {
		memory.Table().Remove(name + "_header")
		return nil, fmt.Errorf("failed to create buffer: %w", err)
	}
	s.buffer = ring

	return s, nil
}

// OpenStream opens an existing stream in shared memory.
func OpenStream[T Numeric](memory *Memory, name string) (*Stream[T], error) {
	entry := memory.Find(name + "_header")
	if entry == nil {
		return nil, fmt.Errorf("stream '%s' not found", name)
	}

	offset := int(entry.Offset)
	data := memory.Data()
	capacity := int(binary.LittleEndian.Uint32(data[offset+StreamBufferCapacityOffset : offset+StreamBufferCapacityOffset+U32Size]))

	s := &Stream[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capacity,
	}

	ring, err := OpenRing[T](memory, name+"_buffer")
	if err != nil {
		return nil, fmt.Errorf("failed to open buffer: %w", err)
	}
	s.buffer = ring

	return s, nil
}

// sequencePtr returns a pointer to the atomic sequence number.
func (s *Stream[T]) sequencePtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&s.memory.Data()[s.offset]))
}

// subscribersPtr returns a pointer to the subscribers counter.
func (s *Stream[T]) subscribersPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&s.memory.Data()[s.offset+StreamSubscribersOffset]))
}

// closedPtr returns a pointer to the closed flag.
func (s *Stream[T]) closedPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&s.memory.Data()[s.offset+StreamClosedOffset]))
}

// isClosed returns true if the stream is closed.
func (s *Stream[T]) isClosed() bool {
	return atomic.LoadUint32(s.closedPtr()) != 0
}

// Emit writes a value to the stream. Returns true on success, false if closed or buffer full.
func (s *Stream[T]) Emit(value T) bool {
	if s.isClosed() {
		return false
	}

	if err := s.buffer.Push(value); err != nil {
		return false // Buffer full
	}

	// Increment sequence number
	atomic.AddUint64(s.sequencePtr(), 1)

	return true
}

// Read reads the next value from the stream (non-blocking).
// Returns the value and true on success, or zero and false if empty or closed.
func (s *Stream[T]) Read() (T, bool) {
	value, err := s.buffer.Pop()
	if err != nil {
		return value, false
	}
	return value, true
}

// ReadTimeout reads the next value with a timeout.
// Returns the value and true on success, or zero and false on timeout or closed (after drain).
func (s *Stream[T]) ReadTimeout(timeout time.Duration) (T, bool) {
	var zero T
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		value, err := s.buffer.Pop()
		if err == nil {
			return value, true
		}
		if s.isClosed() {
			return zero, false
		}
		time.Sleep(time.Microsecond)
	}

	return zero, false
}

// Close closes the stream. Subscribers will drain remaining items then stop.
func (s *Stream[T]) Close() {
	atomic.StoreUint32(s.closedPtr(), 1)
}

// IsClosed returns true if the stream is closed.
func (s *Stream[T]) IsClosed() bool {
	return s.isClosed()
}

// Capacity returns the stream buffer capacity.
func (s *Stream[T]) Capacity() int {
	return s.capacity
}

// Size returns the current number of items in the stream buffer.
func (s *Stream[T]) Size() int {
	return s.buffer.Size()
}

// Version returns the current sequence number (total items ever emitted).
func (s *Stream[T]) Version() uint64 {
	return atomic.LoadUint64(s.sequencePtr())
}

// Subscribers returns the current number of subscribers.
func (s *Stream[T]) Subscribers() int {
	return int(atomic.LoadUint32(s.subscribersPtr()))
}

// Subscribe increments the subscriber count. Returns a function to decrement it.
// The caller is responsible for implementing the subscription logic (e.g., a goroutine that reads).
func (s *Stream[T]) Subscribe() func() {
	atomic.AddUint32(s.subscribersPtr(), 1)
	return func() {
		atomic.AddUint32(s.subscribersPtr(), ^uint32(0))
	}
}

// Name returns the stream name.
func (s *Stream[T]) Name() string {
	return s.name
}

// Destroy removes the stream from the shared memory table.
func (s *Stream[T]) Destroy() {
	s.buffer.Destroy()
	s.memory.Table().Remove(s.name + "_header")
}
