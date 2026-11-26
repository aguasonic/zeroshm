package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"sync/atomic"
	"unsafe"
)

// RingHeaderSize is the size of the ring header in bytes.
const RingHeaderSize = 24

// RingFull is returned when the ring buffer is full.
var RingFull = errors.New("ring buffer is full")

// RingEmpty is returned when the ring buffer is empty.
var RingEmpty = errors.New("ring buffer is empty")

// Ring is a lock-free circular buffer for streaming (MPSC -- multiple-producers /
// single-consumer). Push() uses CAS on the write position so multiple producers
// can safely write concurrently. Pop() is single-consumer only and uses plain
// load/store on the read position. Concurrent consumers will race and corrupt data.
//
// Binary layout:
//   - write_pos: uint64 (atomic, total bytes written)
//   - read_pos: uint64 (atomic, total bytes read)
//   - capacity: uint32 (ring buffer capacity in bytes)
//   - elem_size: uint32 (sizeof(T))
//   - buffer: capacity bytes
type Ring[T Numeric] struct {
	memory   *Memory
	name     string
	offset   int
	capacity int // in bytes
	elemSize int
}

// NewRing creates a new ring buffer in shared memory.
// capacity is the number of elements (rounded down to a multiple of elem_size in bytes).
func NewRing[T Numeric](memory *Memory, name string, capacity int) (*Ring[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	if capacity <= 0 {
		return nil, errors.New("capacity must be positive")
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	// Convert element count to bytes
	capBytes := capacity * elemSize

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("ring '%s' already exists", name)
	}

	// Allocate space
	totalSize := RingHeaderSize + capBytes
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	// Initialize header
	data := memory.Data()
	binary.LittleEndian.PutUint64(data[offset+RingWritePosOffset:offset+RingWritePosOffset+U64Size], 0)                    // write_pos = 0
	binary.LittleEndian.PutUint64(data[offset+RingReadPosOffset:offset+RingReadPosOffset+U64Size], 0)                 // read_pos = 0
	binary.LittleEndian.PutUint32(data[offset+RingCapacityOffset:offset+RingCapacityOffset+U32Size], uint32(capBytes)) // capacity (bytes)
	binary.LittleEndian.PutUint32(data[offset+RingElemSizeOffset:offset+RingElemSizeOffset+U32Size], uint32(elemSize)) // elem_size

	// Zero-initialize buffer
	bufStart := offset + RingHeaderSize
	for i := bufStart; i < bufStart+capBytes; i++ {
		data[i] = 0
	}

	return &Ring[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capBytes,
		elemSize: elemSize,
	}, nil
}

// OpenRing opens an existing ring buffer in shared memory.
func OpenRing[T Numeric](memory *Memory, name string) (*Ring[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("ring '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	offset := int(entry.Offset)
	data := memory.Data()

	// Read header
	capacity := binary.LittleEndian.Uint32(data[offset+RingCapacityOffset : offset+RingCapacityOffset+U32Size])
	storedElemSize := binary.LittleEndian.Uint32(data[offset+RingElemSizeOffset : offset+RingElemSizeOffset+U32Size])

	if int(storedElemSize) != elemSize {
		return nil, fmt.Errorf("type mismatch: expected elem_size %d, got %d", elemSize, storedElemSize)
	}

	// Validate size
	expectedSize := RingHeaderSize + int(capacity)
	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)",
			expectedSize, entry.Size)
	}

	return &Ring[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: int(capacity),
		elemSize: elemSize,
	}, nil
}

// Push writes an element to the ring buffer (MPSC -- lock-free, multiple producers safe).
// Uses CAS on the write position so concurrent producers don't overwrite each other.
// Returns nil if written, RingFull if the buffer is full.
func (r *Ring[T]) Push(value T) error {
	bufOffset := r.offset + RingHeaderSize
	data := r.memory.Data()

	for {
		writePos := atomic.LoadUint64(r.writePosPtr())
		readPos := atomic.LoadUint64(r.readPosPtr())

		// Check if there's space
		if writePos-readPos+uint64(r.elemSize) > uint64(r.capacity) {
			return RingFull
		}

		// Write the data to the claimed slot
		offset := int(writePos % uint64(r.capacity))
		if offset+r.elemSize <= r.capacity {
			// Normal case -- contiguous write
			*(*T)(unsafe.Pointer(&data[bufOffset+offset])) = value
		} else {
			// Wrap around case -- split write
			firstPart := r.capacity - offset
			remaining := r.elemSize - firstPart
			src := unsafe.Slice((*byte)(unsafe.Pointer(&value)), r.elemSize)
			copy(data[bufOffset+offset:bufOffset+r.capacity], src[:firstPart])
			copy(data[bufOffset:bufOffset+remaining], src[firstPart:])
		}

		// Try to claim the slot with CAS; retry if another producer won
		if atomic.CompareAndSwapUint64(r.writePosPtr(), writePos, writePos+uint64(r.elemSize)) {
			return nil
		}
	}
}

// Pop reads an element from the ring buffer (single-consumer -- lock-free).
// Returns the element and nil if read, RingEmpty if the buffer is empty.
func (r *Ring[T]) Pop() (T, error) {
	var zero T
	readPos := atomic.LoadUint64(r.readPosPtr())
	writePos := atomic.LoadUint64(r.writePosPtr())

	// Check if there's data
	if readPos+uint64(r.elemSize) > writePos {
		return zero, RingEmpty
	}

	// Read the data
	bufOffset := r.offset + RingHeaderSize
	offset := int(readPos % uint64(r.capacity))

	data := r.memory.Data()
	if offset+r.elemSize <= r.capacity {
		// Normal case -- contiguous read
		zero = *(*T)(unsafe.Pointer(&data[bufOffset+offset]))
	} else {
		// Wrap around case -- split read
		firstPart := r.capacity - offset
		remaining := r.elemSize - firstPart
		dst := unsafe.Slice((*byte)(unsafe.Pointer(&zero)), r.elemSize)
		copy(dst[:firstPart], data[bufOffset+offset:bufOffset+r.capacity])
		copy(dst[firstPart:], data[bufOffset:bufOffset+remaining])
	}

	// Update read position
	atomic.StoreUint64(r.readPosPtr(), readPos+uint64(r.elemSize))
	return zero, nil
}

// Peek reads the next element without removing it from the ring buffer.
// Returns the element and nil if available, RingEmpty if the buffer is empty.
func (r *Ring[T]) Peek() (T, error) {
	var zero T
	readPos := atomic.LoadUint64(r.readPosPtr())
	writePos := atomic.LoadUint64(r.writePosPtr())

	// Check if there's data
	if readPos+uint64(r.elemSize) > writePos {
		return zero, RingEmpty
	}

	// Read the data (without updating read_pos)
	bufOffset := r.offset + RingHeaderSize
	offset := int(readPos % uint64(r.capacity))

	data := r.memory.Data()
	if offset+r.elemSize <= r.capacity {
		// Normal case -- contiguous read
		zero = *(*T)(unsafe.Pointer(&data[bufOffset+offset]))
	} else {
		// Wrap around case -- split read
		firstPart := r.capacity - offset
		remaining := r.elemSize - firstPart
		dst := unsafe.Slice((*byte)(unsafe.Pointer(&zero)), r.elemSize)
		copy(dst[:firstPart], data[bufOffset+offset:bufOffset+r.capacity])
		copy(dst[firstPart:], data[bufOffset:bufOffset+remaining])
	}

	return zero, nil
}

// Size returns the number of elements available to read.
func (r *Ring[T]) Size() int {
	readPos := atomic.LoadUint64(r.readPosPtr())
	writePos := atomic.LoadUint64(r.writePosPtr())
	return int((writePos - readPos) / uint64(r.elemSize))
}

// Capacity returns the capacity in elements.
func (r *Ring[T]) Capacity() int {
	return r.capacity / r.elemSize
}

// Empty returns true if the ring buffer has no elements.
func (r *Ring[T]) Empty() bool {
	readPos := atomic.LoadUint64(r.readPosPtr())
	writePos := atomic.LoadUint64(r.writePosPtr())
	return readPos == writePos
}

// Full returns true if the ring buffer is full.
func (r *Ring[T]) Full() bool {
	readPos := atomic.LoadUint64(r.readPosPtr())
	writePos := atomic.LoadUint64(r.writePosPtr())
	return (writePos - readPos) >= uint64(r.capacity)
}

// FreeSpace returns the number of elements that can be written.
func (r *Ring[T]) FreeSpace() int {
	readPos := atomic.LoadUint64(r.readPosPtr())
	writePos := atomic.LoadUint64(r.writePosPtr())
	return int((uint64(r.capacity) - (writePos - readPos)) / uint64(r.elemSize))
}

// Reset clears the ring buffer (not thread-safe with concurrent operations).
func (r *Ring[T]) Reset() {
	atomic.StoreUint64(r.writePosPtr(), 0)
	atomic.StoreUint64(r.readPosPtr(), 0)
}

// Name returns the ring buffer name.
func (r *Ring[T]) Name() string {
	return r.name
}

// Destroy removes the ring buffer from the shared memory table.
func (r *Ring[T]) Destroy() {
	r.memory.Table().Remove(r.name)
}

// writePosPtr returns a pointer to the atomic write_pos in the header.
func (r *Ring[T]) writePosPtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&r.memory.Data()[r.offset]))
}

// readPosPtr returns a pointer to the atomic read_pos in the header.
func (r *Ring[T]) readPosPtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&r.memory.Data()[r.offset+RingReadPosOffset]))
}
