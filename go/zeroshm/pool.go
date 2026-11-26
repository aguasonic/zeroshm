package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"sync/atomic"
	"unsafe"
)

// PoolHeaderSize is the size of the pool header in bytes.
const PoolHeaderSize = 24

// PoolExhausted is returned when the pool has no free slots.
var PoolExhausted = errors.New("pool is exhausted")

// PoolInvalidSlot is returned when an invalid slot index is provided.
var PoolInvalidSlot = errors.New("invalid pool slot index")

// PoolNullIndex represents an empty free list.
const PoolNullIndex = uint32(0xFFFFFFFF)

// Pool is a lock-free fixed-size object pool with a free-list.
// Uses tagged pointer CAS (generation + index) to prevent ABA problems.
//
// Binary layout:
//   - free_head: uint64 (atomic, tagged pointer: [generation:32][index:32])
//   - allocated: uint32 (atomic, number of allocated items)
//   - padding_: uint32 (alignment padding)
//   - capacity: uint32 (total slots)
//   - elem_size: uint32 (sizeof(T))
//   - nodes: capacity * (data(T) + next(uint32))
type Pool[T Numeric] struct {
	memory   *Memory
	name     string
	offset   int
	capacity int
	elemSize int
	nodeSize int
}

// PoolNode represents a single slot in the pool.
type PoolNode[T Numeric] struct {
	Data T
	Next uint32
}

// poolPackTagged packs an index and generation into a tagged pointer.
func poolPackTagged(index, generation uint32) uint64 {
	return (uint64(generation) << 32) | uint64(index)
}

// poolUnpackIndex extracts the index from a tagged pointer.
func poolUnpackIndex(tagged uint64) uint32 {
	return uint32(tagged & 0xFFFFFFFF)
}

// poolUnpackGeneration extracts the generation from a tagged pointer.
func poolUnpackGeneration(tagged uint64) uint32 {
	return uint32(tagged >> 32)
}

// NewPool creates a new pool in shared memory.
func NewPool[T Numeric](memory *Memory, name string, capacity int) (*Pool[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	if capacity <= 0 {
		return nil, errors.New("capacity must be positive")
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))
	nodeSize := alignUp(elemSize, 8) + 4 // data (aligned to 8) + next(4)

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("pool '%s' already exists", name)
	}

	// Allocate space
	totalSize := PoolHeaderSize + capacity*nodeSize
	offset, err := memory.Allocate(name, totalSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate: %w", err)
	}

	// Initialize header
	data := memory.Data()
	binary.LittleEndian.PutUint64(data[offset+PoolFreeHeadOffset:offset+PoolFreeHeadOffset+U64Size], poolPackTagged(0, 0)) // free_head = (index=0, gen=0)
	binary.LittleEndian.PutUint32(data[offset+PoolAllocatedOffset:offset+PoolAllocatedOffset+U32Size], 0)                   // allocated = 0
	binary.LittleEndian.PutUint32(data[offset+PoolPaddingOffset:offset+PoolPaddingOffset+U32Size], 0)                  // padding_ = 0
	binary.LittleEndian.PutUint32(data[offset+PoolCapacityOffset:offset+PoolCapacityOffset+U32Size], uint32(capacity))   // capacity
	binary.LittleEndian.PutUint32(data[offset+PoolElemSizeOffset:offset+PoolElemSizeOffset+U32Size], uint32(elemSize))   // elem_size

	// Initialize free list -- all nodes are free
	nodesStart := offset + PoolHeaderSize
	for i := 0; i < capacity; i++ {
		nodeOff := nodesStart + i*nodeSize
		// Zero the data
		for j := 0; j < nodeSize-4; j++ {
			data[nodeOff+j] = 0
		}
		// Set next pointer
		var next uint32 = PoolNullIndex
		if i < capacity-1 {
			next = uint32(i + 1)
		}
		binary.LittleEndian.PutUint32(data[nodeOff+nodeSize-4:nodeOff+nodeSize], next)
	}

	return &Pool[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capacity,
		elemSize: elemSize,
		nodeSize: nodeSize,
	}, nil
}

// OpenPool opens an existing pool in shared memory.
func OpenPool[T Numeric](memory *Memory, name string) (*Pool[T], error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("pool '%s' not found", name)
	}

	var zero T
	elemSize := int(unsafe.Sizeof(zero))
	nodeSize := alignUp(elemSize, 8) + 4

	offset := int(entry.Offset)
	data := memory.Data()

	// Read header
	capacity := binary.LittleEndian.Uint32(data[offset+PoolCapacityOffset : offset+PoolCapacityOffset+U32Size])
	storedElemSize := binary.LittleEndian.Uint32(data[offset+PoolElemSizeOffset : offset+PoolElemSizeOffset+U32Size])

	if int(storedElemSize) != elemSize {
		return nil, fmt.Errorf("type mismatch: expected elem_size %d, got %d", elemSize, storedElemSize)
	}

	// Validate size
	expectedSize := PoolHeaderSize + int(capacity)*nodeSize
	if int(entry.Size) != expectedSize {
		return nil, fmt.Errorf("size mismatch: expected %d, got %d (wrong element type?)",
			expectedSize, entry.Size)
	}

	return &Pool[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: int(capacity),
		elemSize: elemSize,
		nodeSize: nodeSize,
	}, nil
}

// Allocate allocates a slot from the pool (lock-free, ABA-safe).
// Returns the slot index and nil on success, PoolExhausted if the pool is full.
func (p *Pool[T]) Allocate() (int, error) {
	for {
		oldHead := atomic.LoadUint64(p.freeHeadPtr())
		freeIndex := poolUnpackIndex(oldHead)
		generation := poolUnpackGeneration(oldHead)

		if freeIndex == PoolNullIndex {
			return -1, PoolExhausted
		}

		next := p.loadNext(freeIndex)

		// Pack new head with bumped generation to prevent ABA
		newHead := poolPackTagged(next, generation+1)

		// Try to update the free head (tagged CAS prevents ABA)
		if atomic.CompareAndSwapUint64(p.freeHeadPtr(), oldHead, newHead) {
			// Success -- we got the node
			p.incrAllocated()
			return int(freeIndex), nil
		}
	}
}

// Free returns a slot to the pool (lock-free, ABA-safe).
// Returns nil on success, PoolInvalidSlot if the index is invalid.
func (p *Pool[T]) Free(slot int) error {
	if slot < 0 || slot >= p.capacity {
		return PoolInvalidSlot
	}

	nodeIndex := uint32(slot)

	// Add the node back to the free list using tagged CAS
	for {
		oldHead := atomic.LoadUint64(p.freeHeadPtr())
		oldIndex := poolUnpackIndex(oldHead)
		generation := poolUnpackGeneration(oldHead)

		p.storeNext(nodeIndex, oldIndex)

		// Pack new head with bumped generation to prevent ABA
		newHead := poolPackTagged(nodeIndex, generation+1)

		if atomic.CompareAndSwapUint64(p.freeHeadPtr(), oldHead, newHead) {
			break
		}
	}

	p.decrAllocated()
	return nil
}

// Get reads the value at the given slot index.
func (p *Pool[T]) Get(slot int) (T, error) {
	if slot < 0 || slot >= p.capacity {
		var zero T
		return zero, PoolInvalidSlot
	}
	return p.readValue(slot), nil
}

// Set writes a value to the given slot index.
func (p *Pool[T]) Set(slot int, value T) error {
	if slot < 0 || slot >= p.capacity {
		return PoolInvalidSlot
	}
	p.writeValue(slot, value)
	return nil
}

// Size returns the number of allocated slots.
func (p *Pool[T]) Size() int {
	return int(atomic.LoadUint32(p.allocatedPtr()))
}

// Capacity returns the total capacity of the pool.
func (p *Pool[T]) Capacity() int {
	return p.capacity
}

// Available returns the number of free slots.
func (p *Pool[T]) Available() int {
	return p.capacity - p.Size()
}

// Empty returns true if no slots are allocated.
func (p *Pool[T]) Empty() bool {
	return p.Size() == 0
}

// Full returns true if all slots are allocated.
func (p *Pool[T]) Full() bool {
	return p.Size() == p.capacity
}

// Name returns the pool name.
func (p *Pool[T]) Name() string {
	return p.name
}

// Destroy removes the pool from the shared memory table.
func (p *Pool[T]) Destroy() {
	p.memory.Table().Remove(p.name)
}

// freeHeadPtr returns a pointer to the atomic free_head in the header.
func (p *Pool[T]) freeHeadPtr() *uint64 {
	return (*uint64)(unsafe.Pointer(&p.memory.Data()[p.offset]))
}

// allocatedPtr returns a pointer to the atomic allocated counter in the header.
func (p *Pool[T]) allocatedPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&p.memory.Data()[p.offset+8]))
}

// nodeOffset returns the byte offset of the given node index.
func (p *Pool[T]) nodeOffset(idx int) int {
	return p.offset + PoolHeaderSize + idx*p.nodeSize
}

// readValue reads the data at the given slot index.
func (p *Pool[T]) readValue(idx int) T {
	nodeOff := p.nodeOffset(idx)
	data := p.memory.Data()
	return *(*T)(unsafe.Pointer(&data[nodeOff]))
}

// writeValue writes data at the given slot index.
func (p *Pool[T]) writeValue(idx int, value T) {
	nodeOff := p.nodeOffset(idx)
	data := p.memory.Data()
	*(*T)(unsafe.Pointer(&data[nodeOff])) = value
}

// loadNext loads the next pointer from the given node.
func (p *Pool[T]) loadNext(idx uint32) uint32 {
	nodeOff := p.nodeOffset(int(idx))
	data := p.memory.Data()
	return binary.LittleEndian.Uint32(data[nodeOff+p.nodeSize-4 : nodeOff+p.nodeSize])
}

// storeNext stores the next pointer in the given node.
func (p *Pool[T]) storeNext(idx uint32, next uint32) {
	nodeOff := p.nodeOffset(int(idx))
	data := p.memory.Data()
	binary.LittleEndian.PutUint32(data[nodeOff+p.nodeSize-4:nodeOff+p.nodeSize], next)
}

// incrAllocated atomically increments the allocated counter.
func (p *Pool[T]) incrAllocated() {
	atomic.AddUint32(p.allocatedPtr(), 1)
}

// decrAllocated atomically decrements the allocated counter.
func (p *Pool[T]) decrAllocated() {
	atomic.AddUint32(p.allocatedPtr(), ^uint32(0))
}
