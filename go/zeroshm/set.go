package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"hash"
	"hash/fnv"
	"sync/atomic"
	"unsafe"
)

// SetHeaderSize is the size of the set header in bytes.
const SetHeaderSize = 16

// Set states for entries.
const (
	SetStateEmpty     = uint32(0)
	SetStateOccupied  = uint32(1)
	SetStateDeleted   = uint32(2)
	SetStateInserting = uint32(3)
)

// SetMaxSpins is the bound on waiting for a slot stuck in INSERTING.
const SetMaxSpins = 10000

// SetFull is returned when a set is full and insert cannot proceed.
var SetFull = errors.New("set is full")

// SetNotFound is returned when an element is not found in the set.
var SetNotFound = errors.New("element not found in set")

// Set is a lock-free unique-element container in shared memory.
// Uses 4-state CAS with linear probing for thread-safe insert/erase.
//
// Binary layout:
//   - size: uint32 (atomic, current element count)
//   - capacity: uint32 (total capacity)
//   - elem_size: uint32 (sizeof(T))
//   - reserved: uint32 (pads header to 16 bytes)
//   - entries: capacity * (state(4) + value(T))
type Set[T Numeric] struct {
	memory   *Memory
	name     string
	offset   int
	capacity int
	elemSize int
}

// SetEntry represents a single slot in the set.
type SetEntry[T Numeric] struct {
	State uint32
	Value T
}

// NewSet creates a new set in shared memory.
func NewSet[T Numeric](memory *Memory, name string, capacity int) (*Set[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	if capacity <= 0 {
		return nil, errors.New("capacity must be positive")
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("set '%s' already exists", name)
	}

	// Entry size: 4 bytes state + elemSize, aligned to 8 bytes
	entrySize := alignUp(int(unsafe.Sizeof(uint32(0)))+elemSize, 8)

	// Allocate space
	totalSize := SetHeaderSize + capacity*entrySize
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	// Initialize header
	data := memory.Data()
	binary.LittleEndian.PutUint32(data[offset+SetSizeOffset:offset+SetSizeOffset+U32Size], 0)           // size = 0
	binary.LittleEndian.PutUint32(data[offset+SetCapacityOffset:offset+SetCapacityOffset+U32Size], uint32(capacity)) // capacity
	binary.LittleEndian.PutUint32(data[offset+SetElemSizeOffset:offset+SetElemSizeOffset+U32Size], uint32(elemSize)) // elem_size
	binary.LittleEndian.PutUint32(data[offset+SetReservedOffset:offset+SetReservedOffset+U32Size], 0)        // reserved = 0

	// Initialize all entries as empty
	dataStart := offset + SetHeaderSize
	for i := 0; i < capacity; i++ {
		entryOffset := dataStart + i*entrySize
		binary.LittleEndian.PutUint32(data[entryOffset:entryOffset+4], SetStateEmpty)
		// Zero the value
		for j := 4; j < entrySize; j++ {
			data[entryOffset+j] = 0
		}
	}

	return &Set[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capacity,
		elemSize: elemSize,
	}, nil
}

// OpenSet opens an existing set in shared memory.
func OpenSet[T Numeric](memory *Memory, name string) (*Set[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("set '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))

	offset := int(entry.Offset)
	data := memory.Data()

	// Read header
	capacity := binary.LittleEndian.Uint32(data[offset+SetCapacityOffset : offset+SetCapacityOffset+U32Size])
	storedElemSize := binary.LittleEndian.Uint32(data[offset+SetElemSizeOffset : offset+SetElemSizeOffset+U32Size])

	if int(storedElemSize) != elemSize {
		return nil, fmt.Errorf("type mismatch: expected elem_size %d, got %d", elemSize, storedElemSize)
	}

	// Validate size
	entrySize := alignUp(int(unsafe.Sizeof(uint32(0)))+elemSize, 8)
	expectedSize := SetHeaderSize + int(capacity)*entrySize
	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)",
			expectedSize, entry.Size)
	}

	return &Set[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: int(capacity),
		elemSize: elemSize,
	}, nil
}

// trivialHash computes a hash for a Numeric value.
func trivialHash[T Numeric](val T) uint64 {
	switch v := any(val).(type) {
	case int8:
		return uint64(uint8(v)) * 2654435761
	case int16:
		return uint64(uint16(v)) * 2654435761
	case int32:
		return uint64(uint32(v)) * 2654435761
	case int64:
		return uint64(v) * 2654435761
	case uint8:
		return uint64(v) * 2654435761
	case uint16:
		return uint64(v) * 2654435761
	case uint32:
		return uint64(v) * 2654435761
	case uint64:
		return v * 2654435761
	default:
		// For floats, use byte-level hash (fnv)
		h := fnv.New64()
		writeNumeric(h, val)
		return h.Sum64()
	}
}

// writeNumeric writes a numeric value as bytes to a hash function.
func writeNumeric(h hash.Hash, val any) {
	switch v := val.(type) {
	case float32:
		binary.Write(h, binary.LittleEndian, v)
	case float64:
		binary.Write(h, binary.LittleEndian, v)
	default:
		panic("unsupported numeric type")
	}
}

// trivialEqual compares two Numeric values for equality.
func trivialEqual[T Numeric](a, b T) bool {
	switch va := any(a).(type) {
	case float32:
		return va == any(b).(float32)
	case float64:
		return va == any(b).(float64)
	default:
		return a == b
	}
}

// insert inserts an element into the set (lock-free with linear probing).
// Returns nil if the element was inserted or already exists (idempotent).
// Returns SetFull if the set is full.
func (s *Set[T]) insert(value T) error {
	hashVal := trivialHash(value)
	capacity := uint64(s.capacity)

	for {
		var deletedTarget *int = nil
		var emptyTarget *int = nil

		for i := uint64(0); i < capacity && emptyTarget == nil; i++ {
			idx := int((hashVal + i) % capacity)
			entryOffset := s.entryOffset(idx)

			data := s.memory.Data()

			// Load state
			state := binary.LittleEndian.Uint32(data[entryOffset : entryOffset+4])

			if state == SetStateInserting {
				// Mid-write by another goroutine; wait bounded then skip
				spins := 0
				for spins < SetMaxSpins {
					state = binary.LittleEndian.Uint32(data[entryOffset : entryOffset+4])
					if state != SetStateInserting {
						break
					}
					spins++
				}
				// Re-evaluate state after spins
			}

			if state == SetStateOccupied {
				existing := s.readValue(idx)
				if trivialEqual(existing, value) {
					return nil // Already exists, idempotent
				}
				continue // Different value, next slot
			}

			if state == SetStateDeleted {
				if deletedTarget == nil {
					deletedTarget = &idx
				}
				continue // Keep scanning: the value may live further on
			}

			// EMPTY: the probe chain ends here; the value is absent
			emptyTarget = &idx
		}

		var target *int
		if deletedTarget != nil {
			target = deletedTarget
		} else {
			target = emptyTarget
		}
		if target == nil {
			return SetFull // Set is full
		}

		expected := SetStateEmpty
		if deletedTarget != nil {
			expected = SetStateDeleted
		}

		// CAS: claim the slot
		if atomic.CompareAndSwapUint32(s.statePtr(*target), expected, SetStateInserting) {
			// We exclusively own this slot; write the value
			s.writeValue(*target, value)
			// Publish the entry: INSERTING -> OCCUPIED
			atomic.StoreUint32(s.statePtr(*target), SetStateOccupied)
			// Increment size
			s.incrSize()
			return nil
		}
		// CAS failed: another goroutine changed the slot. Rescan from the top.
	}
}

// Contains checks if an element exists in the set.
func (s *Set[T]) Contains(value T) bool {
	hashVal := trivialHash(value)
	capacity := uint64(s.capacity)

	for i := uint64(0); i < capacity; i++ {
		idx := int((hashVal + i) % capacity)
		entryOffset := s.entryOffset(idx)

		data := s.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOffset : entryOffset+4])

		if state == SetStateEmpty {
			return false // Not found
		}

		if state == SetStateOccupied {
			existing := s.readValue(idx)
			if trivialEqual(existing, value) {
				return true
			}
		}
		// Continue searching through DELETED and INSERTING entries
	}

	return false
}

// Insert adds an element to the set.
// Returns nil if inserted or already exists, SetFull if the set is full.
func (s *Set[T]) Insert(value T) error {
	return s.insert(value)
}

// Erase removes an element from the set.
// Returns nil if removed, SetNotFound if the element doesn't exist.
func (s *Set[T]) Erase(value T) error {
	hashVal := trivialHash(value)
	capacity := uint64(s.capacity)

	for i := uint64(0); i < capacity; i++ {
		idx := int((hashVal + i) % capacity)
		entryOffset := s.entryOffset(idx)

		data := s.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOffset : entryOffset+4])

		if state == SetStateEmpty {
			return SetNotFound // Not found
		}

		if state == SetStateOccupied {
			existing := s.readValue(idx)
			if trivialEqual(existing, value) {
				// CAS from OCCUPIED to DELETED
				if atomic.CompareAndSwapUint32(s.statePtr(idx), SetStateOccupied, SetStateDeleted) {
					s.decrSize()
					return nil
				}
				// CAS failed: check if it's now DELETED
				state = atomic.LoadUint32(s.statePtr(idx))
				if state == SetStateDeleted {
					return SetNotFound
				}
				// Otherwise (INSERTING), continue probing
			}
		}
		// Continue searching through DELETED and INSERTING entries
	}

	return SetNotFound
}

// Size returns the current number of elements in the set.
func (s *Set[T]) Size() int {
	return int(atomic.LoadUint32(s.sizePtr()))
}

// Capacity returns the total capacity of the set.
func (s *Set[T]) Capacity() int {
	return s.capacity
}

// Empty returns true if the set has no elements.
func (s *Set[T]) Empty() bool {
	return s.Size() == 0
}

// Clear removes all elements from the set (not thread-safe with concurrent operations).
func (s *Set[T]) Clear() {
	data := s.memory.Data()
	for i := 0; i < s.capacity; i++ {
		entryOffset := s.entryOffset(i)
		binary.LittleEndian.PutUint32(data[entryOffset:entryOffset+4], SetStateEmpty)
		// Zero the value
		for j := 4; j < s.entrySize(); j++ {
			data[entryOffset+j] = 0
		}
	}
	binary.LittleEndian.PutUint32(data[s.offset:s.offset+4], 0)
}

// Entries returns all entries in the set (including empty/tombstone slots).
func (s *Set[T]) Entries() []SetEntry[T] {
	result := make([]SetEntry[T], s.capacity)
	for i := 0; i < s.capacity; i++ {
		entryOffset := s.entryOffset(i)
		data := s.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOffset : entryOffset+4])
		result[i].State = state
		result[i].Value = s.readValue(i)
	}
	return result
}

// Range iterates over occupied entries and calls fn for each.
// If fn returns false, iteration stops.
func (s *Set[T]) Range(fn func(value T) bool) {
	for i := 0; i < s.capacity; i++ {
		entryOffset := s.entryOffset(i)
		data := s.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOffset : entryOffset+4])
		if state == SetStateOccupied {
			value := s.readValue(i)
			if !fn(value) {
				break
			}
		}
	}
}

// Name returns the set name.
func (s *Set[T]) Name() string {
	return s.name
}

// Destroy removes the set from the shared memory table.
func (s *Set[T]) Destroy() {
	s.memory.Table().Remove(s.name)
}

// entryOffset returns the byte offset of the given entry index.
func (s *Set[T]) entryOffset(idx int) int {
	return s.offset + SetHeaderSize + idx*s.entrySize()
}

// entrySize returns the size of a single entry (state + value, aligned).
func (s *Set[T]) entrySize() int {
	return alignUp(4+s.elemSize, 8)
}

// readValue reads the value at the given entry index.
func (s *Set[T]) readValue(idx int) T {
	entryOffset := s.entryOffset(idx)
	data := s.memory.Data()
	return *(*T)(unsafe.Pointer(&data[entryOffset+4]))
}

// writeValue writes a value at the given entry index.
func (s *Set[T]) writeValue(idx int, value T) {
	entryOffset := s.entryOffset(idx)
	data := s.memory.Data()
	*(*T)(unsafe.Pointer(&data[entryOffset+4])) = value
}

// incrSize atomically increments the set size.
func (s *Set[T]) incrSize() {
	atomic.AddUint32(s.sizePtr(), 1)
}

// decrSize atomically decrements the set size.
func (s *Set[T]) decrSize() {
	atomic.AddUint32(s.sizePtr(), ^uint32(0))
}

// sizePtr returns a pointer to the atomic size in the header.
func (s *Set[T]) sizePtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&s.memory.Data()[s.offset]))
}

// statePtr returns a pointer to the atomic state of the given entry.
func (s *Set[T]) statePtr(idx int) *uint32 {
	return (*uint32)(unsafe.Pointer(&s.memory.Data()[s.entryOffset(idx)]))
}

// alignUp rounds n up to the next multiple of a (a must be a power of two).
func alignUp(n, a int) int {
	return (n + a - 1) & ^(a - 1)
}
