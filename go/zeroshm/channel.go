package zeroshm

import (
	"encoding/binary"
	"errors"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// ChannelHeaderSize is the size of the channel header in bytes.
// Layout: capacity(4) + senders(4) + receivers(4) + closed(4) + send_seq(8) + recv_seq(8) = 32
const ChannelHeaderSize = 32

// ChannelClosed is returned when trying to send/recv on a closed channel.
var ChannelClosed = errors.New("channel is closed")

// ChannelEmpty is returned when trying to receive from an empty channel.
var ChannelEmpty = errors.New("channel is empty")

// ChannelFull is returned when trying to send to a full channel with no waiting receiver.
var ChannelFull = errors.New("channel is full")

// Channel is a CSP-style channel for message passing between processes.
// Supports both buffered (Queue-backed, MPMC-safe) and unbuffered (rendezvous) modes.
//
// Binary layout:
//   - Header: capacity(4) + senders(4) + receivers(4) + closed(4) + send_seq(8) + recv_seq(8)
//   - Buffered (capacity > 0): Queue<T> named `<name>_buffer`
//   - Unbuffered (capacity = 0): RendezvousSlot named `<name>_slot` (ready(4) + consumed(4) + data(T))
type Channel[T Numeric] struct {
	memory   *Memory
	name     string
	offset   int // offset of header
	capacity int
	buffer   *Queue[T] // nil for unbuffered
}

// rendezvousReadyOffset is the offset of the ready flag in the rendezvous slot.
const rendezvousReadyOffset = 0

// rendezvousConsumedOffset is the offset of the consumed flag in the rendezvous slot.
const rendezvousConsumedOffset = 4

// rendezvousDataOffset is the offset of the data in the rendezvous slot.
const rendezvousDataOffset = 8

// NewChannel creates a new channel in shared memory.
// capacity = 0 creates an unbuffered (synchronous) channel.
// capacity > 0 creates a buffered channel (rounded up to next power of 2).
func NewChannel[T Numeric](memory *Memory, name string, capacity int) (*Channel[T], error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	// Round capacity up to next power of 2 (for wrap-safety, same as Queue)
	if capacity > 0 {
		capacity = nextPowerOfTwo(capacity)
	}

	// Check if already exists
	if entry := memory.Find(name + "_header"); entry != nil {
		return nil, fmt.Errorf("channel '%s' already exists", name)
	}

	// Allocate header
	offset, err := memory.Allocate(name+"_header", ChannelHeaderSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate header: %w", err)
	}

	data := memory.Data()

	// Initialize header
	binary.LittleEndian.PutUint32(data[offset+ChannelCapacityOffset:offset+ChannelCapacityOffset+U32Size], uint32(capacity)) // capacity
	binary.LittleEndian.PutUint32(data[offset+ChannelSendersOffset:offset+ChannelSendersOffset+U32Size], 0)              // senders
	binary.LittleEndian.PutUint32(data[offset+ChannelReceiversOffset:offset+ChannelReceiversOffset+U32Size], 0)             // receivers
	binary.LittleEndian.PutUint32(data[offset+ChannelClosedOffset:offset+ChannelClosedOffset+U32Size], 0)            // closed = false
	binary.LittleEndian.PutUint64(data[offset+ChannelSendSeqOffset:offset+ChannelSendSeqOffset+U64Size], 0)            // send_seq
	binary.LittleEndian.PutUint64(data[offset+ChannelRecvSeqOffset:offset+ChannelRecvSeqOffset+U64Size], 0)            // recv_seq

	ch := &Channel[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capacity,
	}

	if capacity > 0 {
		// Buffered channel uses queue
		queue, err := NewQueue[T](memory, name+"_buffer", capacity)
		if err != nil {
			memory.Table().Remove(name + "_header")
			return nil, fmt.Errorf("failed to create buffer: %w", err)
		}
		ch.buffer = queue
	} else {
		// Unbuffered uses rendezvous slot
		var zero T
		elemSize := int(unsafe.Sizeof(zero))
		slotSize := alignUp(elemSize, 8) + 8 // ready(4) + consumed(4) + data(aligned to 8)

		slotOffset, err := memory.Allocate(name+"_slot", slotSize)
		if err != nil {
			memory.Table().Remove(name + "_header")
			return nil, fmt.Errorf("failed to allocate slot: %w", err)
		}
		// Initialize slot
		binary.LittleEndian.PutUint32(data[slotOffset+rendezvousReadyOffset:slotOffset+rendezvousReadyOffset+4], 0)     // ready = false
		binary.LittleEndian.PutUint32(data[slotOffset+rendezvousConsumedOffset:slotOffset+rendezvousConsumedOffset+4], 0) // consumed = false
		// Zero the data area
		for i := rendezvousDataOffset; i < slotSize; i++ {
			data[slotOffset+i] = 0
		}
	}

	return ch, nil
}

// OpenChannel opens an existing channel in shared memory.
func OpenChannel[T Numeric](memory *Memory, name string) (*Channel[T], error) {
	entry := memory.Find(name + "_header")
	if entry == nil {
		return nil, fmt.Errorf("channel '%s' not found", name)
	}

	offset := int(entry.Offset)
	data := memory.Data()
	capacity := int(binary.LittleEndian.Uint32(data[offset+ChannelCapacityOffset : offset+ChannelCapacityOffset+U32Size]))

	ch := &Channel[T]{
		memory:   memory,
		name:     name,
		offset:   offset,
		capacity: capacity,
	}

	if capacity > 0 {
		queue, err := OpenQueue[T](memory, name+"_buffer")
		if err != nil {
			return nil, fmt.Errorf("failed to open buffer: %w", err)
		}
		ch.buffer = queue
	} else {
		// Verify slot exists
		if slotEntry := memory.Find(name + "_slot"); slotEntry == nil {
			return nil, fmt.Errorf("channel '%s' slot not found", name)
		}
	}

	return ch, nil
}

// sendHeaderPtr returns a pointer to the senders counter.
func (ch *Channel[T]) sendersPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&ch.memory.Data()[ch.offset+4]))
}

// recvHeaderPtr returns a pointer to the receivers counter.
func (ch *Channel[T]) receiversPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&ch.memory.Data()[ch.offset+8]))
}

// closedPtr returns a pointer to the closed flag.
func (ch *Channel[T]) closedPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&ch.memory.Data()[ch.offset+12]))
}

// slotDataPtr returns a pointer to the data area in the rendezvous slot.
func (ch *Channel[T]) slotDataPtr() *T {
	entry := ch.memory.Find(ch.name + "_slot")
	if entry == nil {
		return nil
	}
	slotOffset := int(entry.Offset)
	return (*T)(unsafe.Pointer(&ch.memory.Data()[slotOffset+rendezvousDataOffset]))
}

// slotReadyPtr returns a pointer to the ready flag in the rendezvous slot.
func (ch *Channel[T]) slotReadyPtr() *uint32 {
	entry := ch.memory.Find(ch.name + "_slot")
	if entry == nil {
		return nil
	}
	slotOffset := int(entry.Offset)
	return (*uint32)(unsafe.Pointer(&ch.memory.Data()[slotOffset+rendezvousReadyOffset]))
}

// slotConsumedPtr returns a pointer to the consumed flag in the rendezvous slot.
func (ch *Channel[T]) slotConsumedPtr() *uint32 {
	entry := ch.memory.Find(ch.name + "_slot")
	if entry == nil {
		return nil
	}
	slotOffset := int(entry.Offset)
	return (*uint32)(unsafe.Pointer(&ch.memory.Data()[slotOffset+rendezvousConsumedOffset]))
}

// isClosed returns true if the channel is closed.
func (ch *Channel[T]) isClosed() bool {
	return atomic.LoadUint32(ch.closedPtr()) != 0
}

// rendezvousSend performs a synchronous rendezvous send (unbuffered).
// Blocks until a receiver consumes the value, or returns false if channel is closed.
func (ch *Channel[T]) rendezvousSend(value T) bool {
	// Indicate we're a waiting sender
	atomic.AddUint32(ch.sendersPtr(), 1)

	readyPtr := ch.slotReadyPtr()
	consumedPtr := ch.slotConsumedPtr()
	dataPtr := ch.slotDataPtr()

	// Wait for slot to be free
	for atomic.LoadUint32(readyPtr) != 0 {
		if ch.isClosed() {
			atomic.AddUint32(ch.sendersPtr(), ^uint32(0))
			return false
		}
		time.Sleep(time.Microsecond)
	}

	// Place data
	*dataPtr = value
	atomic.StoreUint32(consumedPtr, 0)
	atomic.StoreUint32(readyPtr, 1)

	// Wait for receiver to consume
	for atomic.LoadUint32(consumedPtr) == 0 {
		if ch.isClosed() {
			atomic.AddUint32(ch.sendersPtr(), ^uint32(0))
			return false
		}
		time.Sleep(time.Microsecond)
	}

	atomic.AddUint32(ch.sendersPtr(), ^uint32(0))
	return true
}

// rendezvousRecv performs a synchronous rendezvous receive (unbuffered).
// Blocks until a sender provides a value, or returns zero and false if channel is closed.
func (ch *Channel[T]) rendezvousRecv() (T, bool) {
	var zero T

	// Indicate we're a waiting receiver
	atomic.AddUint32(ch.receiversPtr(), 1)

	readyPtr := ch.slotReadyPtr()
	consumedPtr := ch.slotConsumedPtr()
	dataPtr := ch.slotDataPtr()

	// Wait for data
	for atomic.LoadUint32(readyPtr) == 0 {
		if ch.isClosed() {
			atomic.AddUint32(ch.receiversPtr(), ^uint32(0))
			return zero, false
		}
		time.Sleep(time.Microsecond)
	}

	// Get data
	value := *dataPtr

	// Mark as consumed
	atomic.StoreUint32(consumedPtr, 1)
	atomic.StoreUint32(readyPtr, 0)

	atomic.AddUint32(ch.receiversPtr(), ^uint32(0))
	return value, true
}

// Send sends a value, blocking until received or buffered.
// Returns true on success, false if the channel is closed.
func (ch *Channel[T]) Send(value T) bool {
	if ch.isClosed() {
		return false
	}

	if ch.capacity > 0 {
		// Buffered send
		return ch.buffer.Push(value)
	}
	// Unbuffered: synchronous rendezvous
	return ch.rendezvousSend(value)
}

// TrySend tries to send without blocking.
// Returns nil on success, ChannelClosed if closed, ChannelFull if buffer full or no receiver.
func (ch *Channel[T]) TrySend(value T) error {
	if ch.isClosed() {
		return ChannelClosed
	}

	if ch.capacity > 0 {
		if ch.buffer.Push(value) {
			return nil
		}
		return ChannelFull
	}

	// For unbuffered, check if receiver is waiting
	if atomic.LoadUint32(ch.receiversPtr()) > 0 {
		if ch.rendezvousSend(value) {
			return nil
		}
	}
	return ChannelFull
}

// SendTimeout sends with a timeout.
// Returns true on success, false on timeout or closed channel.
func (ch *Channel[T]) SendTimeout(value T, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		if err := ch.TrySend(value); err == nil {
			return true
		}
		if ch.isClosed() {
			return false
		}
		time.Sleep(time.Microsecond)
	}

	return false
}

// Receive receives a value.
// For buffered channels: non-blocking, returns zero and false when empty.
// For unbuffered channels: blocks until a sender arrives.
// Returns the value and true on success, or zero and false if empty/closed.
func (ch *Channel[T]) Receive() (T, bool) {
	if ch.capacity > 0 {
		// Buffered receive (non-blocking)
		return ch.buffer.Pop()
	}
	// Unbuffered: synchronous rendezvous
	return ch.rendezvousRecv()
}

// TryReceive tries to receive without blocking.
// Returns the value and nil on success, ChannelClosed if closed and empty, ChannelEmpty if no data.
func (ch *Channel[T]) TryReceive() (T, error) {
	var zero T

	if ch.capacity > 0 {
		if value, ok := ch.buffer.Pop(); ok {
			return value, nil
		}
		if ch.isClosed() {
			return zero, ChannelClosed
		}
		return zero, ChannelEmpty
	}

	// For unbuffered, check if sender is waiting
	readyPtr := ch.slotReadyPtr()
	if readyPtr != nil && atomic.LoadUint32(readyPtr) != 0 {
		if value, ok := ch.rendezvousRecv(); ok {
			return value, nil
		}
		return zero, ChannelClosed
	}
	if ch.isClosed() {
		return zero, ChannelClosed
	}
	return zero, ChannelEmpty
}

// ReceiveTimeout receives with a timeout.
// Returns the value and true on success, or zero and false on timeout or closed.
func (ch *Channel[T]) ReceiveTimeout(timeout time.Duration) (T, bool) {
	var zero T
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		value, err := ch.TryReceive()
		if err == nil {
			return value, true
		}
		if ch.isClosed() && !ch.hasData() {
			return zero, false
		}
		time.Sleep(time.Microsecond)
	}

	return zero, false
}

// hasData returns true if there is data available to receive.
func (ch *Channel[T]) hasData() bool {
	if ch.capacity > 0 && ch.buffer != nil {
		return !ch.buffer.Empty()
	}
	readyPtr := ch.slotReadyPtr()
	if readyPtr == nil {
		return false
	}
	return atomic.LoadUint32(readyPtr) != 0
}

// Close closes the channel.
// Senders will be unblocked with failure, receivers will drain remaining data then fail.
func (ch *Channel[T]) Close() {
	atomic.StoreUint32(ch.closedPtr(), 1)
}

// IsClosed returns true if the channel is closed.
func (ch *Channel[T]) IsClosed() bool {
	return ch.isClosed()
}

// Capacity returns the channel capacity (0 for unbuffered).
func (ch *Channel[T]) Capacity() int {
	return ch.capacity
}

// IsBuffered returns true if the channel is buffered.
func (ch *Channel[T]) IsBuffered() bool {
	return ch.capacity > 0
}

// Name returns the channel name.
func (ch *Channel[T]) Name() string {
	return ch.name
}

// Destroy removes the channel from the shared memory table.
func (ch *Channel[T]) Destroy() {
	if ch.buffer != nil {
		ch.memory.Table().Remove(ch.name + "_buffer")
	} else {
		ch.memory.Table().Remove(ch.name + "_slot")
	}
	ch.memory.Table().Remove(ch.name + "_header")
}
