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

// MapHeaderSize is the size of the map header in bytes.
const MapHeaderSize = 16

// Map states for entries.
const (
	MapStateEmpty     = uint32(0)
	MapStateOccupied  = uint32(1)
	MapStateDeleted   = uint32(2)
	MapStateInserting = uint32(3)
)

// MapMaxSpins is the bound on waiting for a slot stuck in INSERTING.
const MapMaxSpins = 10000

// MapFull is returned when a map is full and insert cannot proceed.
var MapFull = errors.New("map is full")

// MapNotFound is returned when a key is not found in the map.
var MapNotFound = errors.New("key not found in map")

// Map is a lock-free hash-based key-value store in shared memory.
// Uses 4-state CAS with linear probing for thread-safe insert/erase.
//
// Binary layout:
//   - size: uint32 (atomic, current element count)
//   - capacity: uint32 (total capacity)
//   - key_size: uint32 (sizeof(K))
//   - value_size: uint32 (sizeof(V))
//   - entries: capacity * (state(4) + key(K) + value(V), aligned)
type Map[K Numeric, V Numeric] struct {
	memory     *Memory
	name       string
	offset     int
	capacity   int
	keySize    int
	valSize    int
	keyOffset  int
	valOffset  int
	entrySize  int
}

// MapEntry represents a single slot in the map.
type MapEntry[K Numeric, V Numeric] struct {
	State uint32
	Key   K
	Value V
}

// NewMap creates a new map in shared memory.
func NewMap[K Numeric, V Numeric](memory *Memory, name string, capacity int) (*Map[K, V], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	if capacity <= 0 {
		return nil, errors.New("capacity must be positive")
	}

	var zeroK K
	var zeroV V
	keySize := int(unsafe.Sizeof(zeroK))
	valSize := int(unsafe.Sizeof(zeroV))

	// Compute aligned offsets within entry
	keyAlign := int(unsafe.Alignof(zeroK))
	valAlign := int(unsafe.Alignof(zeroV))
	keyOffset := alignUp(4, keyAlign)
	valOffset := alignUp(keyOffset+keySize, valAlign)
	entrySize := alignUp(valOffset+valSize, 8)

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("map '%s' already exists", name)
	}

	// Allocate space
	totalSize := MapHeaderSize + capacity*entrySize
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	// Initialize header
	data := memory.Data()
	binary.LittleEndian.PutUint32(data[offset+MapSizeOffset:offset+MapSizeOffset+U32Size], 0)                    // size = 0
	binary.LittleEndian.PutUint32(data[offset+MapCapacityOffset:offset+MapCapacityOffset+U32Size], uint32(capacity))   // capacity
	binary.LittleEndian.PutUint32(data[offset+MapKeySizeOffset:offset+MapKeySizeOffset+U32Size], uint32(keySize))   // key_size
	binary.LittleEndian.PutUint32(data[offset+MapValueSizeOffset:offset+MapValueSizeOffset+U32Size], uint32(valSize))  // value_size

	// Initialize all entries as empty
	dataStart := offset + MapHeaderSize
	for i := 0; i < capacity; i++ {
		entryOff := dataStart + i*entrySize
		binary.LittleEndian.PutUint32(data[entryOff:entryOff+4], MapStateEmpty)
		// Zero the key and value
		for j := 4; j < entrySize; j++ {
			data[entryOff+j] = 0
		}
	}

	return &Map[K, V]{
		memory:    memory,
		name:      name,
		offset:    offset,
		capacity:  capacity,
		keySize:   keySize,
		valSize:   valSize,
		keyOffset: keyOffset,
		valOffset: valOffset,
		entrySize: entrySize,
	}, nil
}

// OpenMap opens an existing map in shared memory.
func OpenMap[K Numeric, V Numeric](memory *Memory, name string) (*Map[K, V], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("map '%s' not found", name)
	}

	var zeroK K
	var zeroV V
	keySize := int(unsafe.Sizeof(zeroK))
	valSize := int(unsafe.Sizeof(zeroV))

	offset := int(entry.Offset)
	data := memory.Data()

	// Read header
	capacity := binary.LittleEndian.Uint32(data[offset+MapCapacityOffset : offset+MapCapacityOffset+U32Size])
	storedKeySize := binary.LittleEndian.Uint32(data[offset+MapKeySizeOffset : offset+MapKeySizeOffset+U32Size])
	storedValSize := binary.LittleEndian.Uint32(data[offset+MapValueSizeOffset : offset+MapValueSizeOffset+U32Size])

	if int(storedKeySize) != keySize || int(storedValSize) != valSize {
		return nil, fmt.Errorf("type mismatch: expected key_size %d/value_size %d, got %d/%d",
			keySize, valSize, storedKeySize, storedValSize)
	}

	// Compute aligned offsets within entry
	keyAlign := int(unsafe.Alignof(zeroK))
	valAlign := int(unsafe.Alignof(zeroV))
	keyOffset := alignUp(4, keyAlign)
	valOffset := alignUp(keyOffset+keySize, valAlign)
	entrySize := alignUp(valOffset+valSize, 8)

	// Validate size
	expectedSize := MapHeaderSize + int(capacity)*entrySize
	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)",
			expectedSize, entry.Size)
	}

	return &Map[K, V]{
		memory:    memory,
		name:      name,
		offset:    offset,
		capacity:  int(capacity),
		keySize:   keySize,
		valSize:   valSize,
		keyOffset: keyOffset,
		valOffset: valOffset,
		entrySize: entrySize,
	}, nil
}

// mapHash computes a hash for a Numeric key.
func mapHash[K Numeric](key K) uint64 {
	switch v := any(key).(type) {
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
		mapWriteNumeric(h, key)
		return h.Sum64()
	}
}

// mapWriteNumeric writes a numeric value as bytes to a hash function.
func mapWriteNumeric(h hash.Hash, val any) {
	switch v := val.(type) {
	case float32:
		binary.Write(h, binary.LittleEndian, v)
	case float64:
		binary.Write(h, binary.LittleEndian, v)
	default:
		panic("unsupported numeric type")
	}
}

// mapKeyEqual compares two Numeric keys for equality.
func mapKeyEqual[K Numeric](a, b K) bool {
	switch va := any(a).(type) {
	case float32:
		return va == any(b).(float32)
	case float64:
		return va == any(b).(float64)
	default:
		return a == b
	}
}

// insert inserts or updates a key-value pair (lock-free with linear probing).
// Returns nil if inserted or updated, MapFull if the map is full.
func (m *Map[K, V]) insert(key K, value V) error {
	hashVal := mapHash(key)
	capacity := uint64(m.capacity)

	for {
		var deletedTarget *int = nil
		var emptyTarget *int = nil

		for i := uint64(0); i < capacity && emptyTarget == nil; i++ {
			idx := int((hashVal + i) % capacity)
			entryOff := m.entryOffset(idx)

			data := m.memory.Data()

			// Load state
			state := binary.LittleEndian.Uint32(data[entryOff : entryOff+4])

			if state == MapStateInserting {
				// Mid-write by another goroutine; wait bounded then skip
				spins := 0
				for spins < MapMaxSpins {
					state = binary.LittleEndian.Uint32(data[entryOff : entryOff+4])
					if state != MapStateInserting {
						break
					}
					spins++
				}
				// Re-evaluate state after spins
			}

			if state == MapStateOccupied {
				existingKey := m.readKey(idx)
				if mapKeyEqual(existingKey, key) {
					// Update: CAS OCCUPIED -> INSERTING for exclusive access
					if atomic.CompareAndSwapUint32(m.statePtr(idx), MapStateOccupied, MapStateInserting) {
						m.writeValue(idx, value)
						m.statePtr_store(idx, MapStateOccupied)
						return nil
					}
					// CAS failed: erased or another updater won; re-examine
					continue
				}
				continue // Different key, next slot
			}

			if state == MapStateDeleted {
				if deletedTarget == nil {
					deletedTarget = &idx
				}
				continue // Keep scanning: the key may live further on
			}

			// EMPTY: the probe chain ends here; the key is absent
			emptyTarget = &idx
		}

		var target *int
		if deletedTarget != nil {
			target = deletedTarget
		} else {
			target = emptyTarget
		}
		if target == nil {
			return MapFull // Map is full
		}

		expected := MapStateEmpty
		if deletedTarget != nil {
			expected = MapStateDeleted
		}

		// CAS: claim the slot
		if atomic.CompareAndSwapUint32(m.statePtr(*target), expected, MapStateInserting) {
			// We exclusively own this slot; write key and value
			m.writeKey(*target, key)
			m.writeValue(*target, value)
			// Publish the entry: INSERTING -> OCCUPIED
			m.statePtr_store(*target, MapStateOccupied)
			// Increment size
			m.incrSize()
			return nil
		}
		// CAS failed: another goroutine changed the slot. Rescan from the top.
	}
}

// Set inserts or updates a key-value pair in the map.
// Returns nil if inserted or updated, MapFull if the map is full.
func (m *Map[K, V]) Set(key K, value V) error {
	return m.insert(key, value)
}

// Get retrieves the value for a key.
// Returns the value and nil if found, MapNotFound if the key doesn't exist.
func (m *Map[K, V]) Get(key K) (V, error) {
	var zero V
	hashVal := mapHash(key)
	capacity := uint64(m.capacity)

	for i := uint64(0); i < capacity; i++ {
		idx := int((hashVal + i) % capacity)
		entryOff := m.entryOffset(idx)

		data := m.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOff : entryOff+4])

		if state == MapStateEmpty {
			return zero, MapNotFound // Not found
		}

		if state == MapStateInserting {
			// Mid-write; wait bounded then skip
			spins := 0
			for spins < MapMaxSpins {
				state = binary.LittleEndian.Uint32(data[entryOff : entryOff+4])
				if state != MapStateInserting {
					break
				}
				spins++
			}
			if state == MapStateInserting {
				continue // Still inserting, skip
			}
		}

		if state == MapStateOccupied {
			existingKey := m.readKey(idx)
			if mapKeyEqual(existingKey, key) {
				return m.readValue(idx), nil
			}
		}
		// Continue searching through DELETED entries
	}

	return zero, MapNotFound
}

// Delete removes a key from the map.
// Returns nil if removed, MapNotFound if the key doesn't exist.
func (m *Map[K, V]) Delete(key K) error {
	hashVal := mapHash(key)
	capacity := uint64(m.capacity)

	for i := uint64(0); i < capacity; i++ {
		idx := int((hashVal + i) % capacity)
		entryOff := m.entryOffset(idx)

		data := m.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOff : entryOff+4])

		if state == MapStateEmpty {
			return MapNotFound // Not found
		}

		if state == MapStateInserting {
			// Mid-write; wait bounded then skip
			spins := 0
			for spins < MapMaxSpins {
				state = binary.LittleEndian.Uint32(data[entryOff : entryOff+4])
				if state != MapStateInserting {
					break
				}
				spins++
			}
			if state == MapStateInserting {
				continue // Still inserting, skip
			}
		}

		if state == MapStateOccupied {
			existingKey := m.readKey(idx)
			if mapKeyEqual(existingKey, key) {
				// CAS from OCCUPIED to DELETED
				if atomic.CompareAndSwapUint32(m.statePtr(idx), MapStateOccupied, MapStateDeleted) {
					m.decrSize()
					return nil
				}
				// CAS failed: check if it's now DELETED
				state = atomic.LoadUint32(m.statePtr(idx))
				if state == MapStateDeleted {
					return MapNotFound
				}
				// Otherwise (INSERTING), continue probing
			}
		}
		// Continue searching through DELETED entries
	}

	return MapNotFound
}

// Contains checks if a key exists in the map.
func (m *Map[K, V]) Contains(key K) bool {
	_, err := m.Get(key)
	return err == nil
}

// Size returns the current number of elements in the map.
func (m *Map[K, V]) Size() int {
	return int(atomic.LoadUint32(m.sizePtr()))
}

// Capacity returns the total capacity of the map.
func (m *Map[K, V]) Capacity() int {
	return m.capacity
}

// Empty returns true if the map has no elements.
func (m *Map[K, V]) Empty() bool {
	return m.Size() == 0
}

// Clear removes all elements from the map (not thread-safe with concurrent operations).
func (m *Map[K, V]) Clear() {
	data := m.memory.Data()
	for i := 0; i < m.capacity; i++ {
		entryOff := m.entryOffset(i)
		binary.LittleEndian.PutUint32(data[entryOff:entryOff+4], MapStateEmpty)
		// Zero the key and value
		for j := 4; j < m.entrySize; j++ {
			data[entryOff+j] = 0
		}
	}
	binary.LittleEndian.PutUint32(data[m.offset:m.offset+4], 0)
}

// Keys returns all keys currently in the map.
func (m *Map[K, V]) Keys() []K {
	var result []K
	m.Range(func(key K, value V) bool {
		result = append(result, key)
		return true
	})
	return result
}

// Range iterates over occupied entries and calls fn for each.
// If fn returns false, iteration stops.
func (m *Map[K, V]) Range(fn func(key K, value V) bool) {
	for i := 0; i < m.capacity; i++ {
		entryOff := m.entryOffset(i)
		data := m.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOff : entryOff+4])
		if state == MapStateOccupied {
			key := m.readKey(i)
			value := m.readValue(i)
			if !fn(key, value) {
				break
			}
		}
	}
}

// Entries returns all entries in the map (including empty/tombstone slots).
func (m *Map[K, V]) Entries() []MapEntry[K, V] {
	result := make([]MapEntry[K, V], m.capacity)
	for i := 0; i < m.capacity; i++ {
		entryOff := m.entryOffset(i)
		data := m.memory.Data()
		state := binary.LittleEndian.Uint32(data[entryOff : entryOff+4])
		result[i].State = state
		result[i].Key = m.readKey(i)
		result[i].Value = m.readValue(i)
	}
	return result
}

// Name returns the map name.
func (m *Map[K, V]) Name() string {
	return m.name
}

// Destroy removes the map from the shared memory table.
func (m *Map[K, V]) Destroy() {
	m.memory.Table().Remove(m.name)
}

// entryOffset returns the byte offset of the given entry index.
func (m *Map[K, V]) entryOffset(idx int) int {
	return m.offset + MapHeaderSize + idx*m.entrySize
}

// readKey reads the key at the given entry index.
func (m *Map[K, V]) readKey(idx int) K {
	entryOff := m.entryOffset(idx)
	data := m.memory.Data()
	return *(*K)(unsafe.Pointer(&data[entryOff+m.keyOffset]))
}

// writeKey writes a key at the given entry index.
func (m *Map[K, V]) writeKey(idx int, key K) {
	entryOff := m.entryOffset(idx)
	data := m.memory.Data()
	*(*K)(unsafe.Pointer(&data[entryOff+m.keyOffset])) = key
}

// readValue reads the value at the given entry index.
func (m *Map[K, V]) readValue(idx int) V {
	entryOff := m.entryOffset(idx)
	data := m.memory.Data()
	return *(*V)(unsafe.Pointer(&data[entryOff+m.valOffset]))
}

// writeValue writes a value at the given entry index.
func (m *Map[K, V]) writeValue(idx int, value V) {
	entryOff := m.entryOffset(idx)
	data := m.memory.Data()
	*(*V)(unsafe.Pointer(&data[entryOff+m.valOffset])) = value
}

// incrSize atomically increments the map size.
func (m *Map[K, V]) incrSize() {
	atomic.AddUint32(m.sizePtr(), 1)
}

// decrSize atomically decrements the map size.
func (m *Map[K, V]) decrSize() {
	atomic.AddUint32(m.sizePtr(), ^uint32(0))
}

// sizePtr returns a pointer to the atomic size in the header.
func (m *Map[K, V]) sizePtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&m.memory.Data()[m.offset]))
}

// statePtr returns a pointer to the atomic state of the given entry.
func (m *Map[K, V]) statePtr(idx int) *uint32 {
	return (*uint32)(unsafe.Pointer(&m.memory.Data()[m.entryOffset(idx)]))
}

// statePtr_store stores a state value to the given entry (convenience helper).
// Used after successful CAS where we have exclusive access to the slot.
func (m *Map[K, V]) statePtr_store(idx int, state uint32) {
	atomic.StoreUint32(m.statePtr(idx), state)
}
