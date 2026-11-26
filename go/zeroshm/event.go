package zeroshm

import (
	"encoding/binary"
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// EventMode represents the mode of an Event.
type EventMode uint32

const (
	// EventModeAutoReset wakes one waiter and auto-resets.
	EventModeAutoReset EventMode = 0
	// EventModeManualReset wakes all waiters and stays signaled.
	EventModeManualReset EventMode = 1
)

// EventStateSize is the size of the event state in bytes.
// Layout: signaled(4) + mode(4) + waiting(4) = 12 bytes
const EventStateSize = 12

// Event is an event synchronization primitive for shared memory.
//
// Provides manual-reset and auto-reset event semantics similar to
// Win32 Events or POSIX condition variables.
//
// Binary layout:
//   - signaled: uint32 (atomic, offset 0) - 0 = not signaled, 1 = signaled
//   - mode: uint32 (offset 4) - AutoReset or ManualReset
//   - waiting: uint32 (atomic, offset 8) - number of waiting processes
//
// Example:
//
//	mem, _ := NewMemory("/sync", 1024*1024, 64)
//	event, _ := NewEvent(mem, "ready", EventModeManualReset)
//
//	// Process A - waits for ready signal
//	event.Wait()
//
//	// Process B - signals when ready
//	event.Signal()
type Event struct {
	memory *Memory
	name   string
	offset int
	sem    *Semaphore
}

// NewEvent creates a new event in shared memory.
func NewEvent(memory *Memory, name string, mode EventMode) (*Event, error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("event '%s' already exists", name)
	}

	// Allocate state
	offset, err := memory.Allocate(name, EventStateSize)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate event: %w", err)
	}

	// Initialize state
	data := memory.Data()
	binary.LittleEndian.PutUint32(data[offset+EventSignaledOffset:], 0)                     // signaled = 0
	binary.LittleEndian.PutUint32(data[offset+EventModeOffset:], uint32(mode))        // mode
	binary.LittleEndian.PutUint32(data[offset+EventWaitingOffset:], 0)                   // waiting = 0

	// Create companion semaphore (initially 0)
	sem, err := NewSemaphore(memory, name+"_sem", 0, 0)
	if err != nil {
		return nil, fmt.Errorf("failed to create event semaphore: %w", err)
	}

	return &Event{
		memory: memory,
		name:   name,
		offset: offset,
		sem:    sem,
	}, nil
}

// OpenEvent opens an existing event in shared memory.
func OpenEvent(memory *Memory, name string) (*Event, error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("event '%s' not found", name)
	}

	if int(entry.Size) != EventStateSize {
		return nil, fmt.Errorf("invalid event size: %d, expected %d", entry.Size, EventStateSize)
	}

	// Open companion semaphore
	sem, err := OpenSemaphore(memory, name+"_sem")
	if err != nil {
		return nil, fmt.Errorf("failed to open event semaphore: %w", err)
	}

	return &Event{
		memory: memory,
		name:   name,
		offset: int(entry.Offset),
		sem:    sem,
	}, nil
}

// signaledPtr returns a pointer to the atomic signaled flag.
func (e *Event) signaledPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&e.memory.Data()[e.offset]))
}

// waitingPtr returns a pointer to the atomic waiting counter.
func (e *Event) waitingPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&e.memory.Data()[e.offset+8]))
}

// mode returns the event mode.
func (e *Event) mode() EventMode {
	data := e.memory.Data()
	return EventMode(binary.LittleEndian.Uint32(data[e.offset+4:]))
}

// isManualReset returns true if the event is in ManualReset mode.
func (e *Event) isManualReset() bool {
	return e.mode() == EventModeManualReset
}

// Signal the event.
// AutoReset: Wakes one waiting thread.
// ManualReset: Wakes all waiting threads, stays signaled.
func (e *Event) Signal() {
	atomic.StoreUint32(e.signaledPtr(), 1)

	if !e.isManualReset() {
		// AutoReset: release semaphore to wake one waiter
		_ = e.sem.Release()
	}
}

// Wait blocks until the event is signaled.
// AutoReset: consuming the signal is automatic.
// ManualReset: the event stays signaled until Reset() is called.
func (e *Event) Wait() {
	if e.isManualReset() {
		atomic.AddUint32(e.waitingPtr(), 1)
		defer atomic.AddUint32(e.waitingPtr(), ^uint32(0))

		// Spin-wait for signaled
		for atomic.LoadUint32(e.signaledPtr()) == 0 {
			time.Sleep(time.Microsecond)
		}
	} else {
		// AutoReset: acquire semaphore
		e.sem.Acquire()
		atomic.StoreUint32(e.signaledPtr(), 0)
	}
}

// WaitTimeout waits for the event with a timeout.
// Returns true if the event was signaled, false if the timeout expired.
func (e *Event) WaitTimeout(timeout time.Duration) bool {
	if e.isManualReset() {
		atomic.AddUint32(e.waitingPtr(), 1)
		defer atomic.AddUint32(e.waitingPtr(), ^uint32(0))

		deadline := time.Now().Add(timeout)
		for time.Now().Before(deadline) {
			if atomic.LoadUint32(e.signaledPtr()) != 0 {
				return true
			}
			time.Sleep(time.Microsecond)
		}
		return false
	} else {
		// AutoReset: acquire with timeout
		if e.sem.AcquireTimeout(timeout) {
			atomic.StoreUint32(e.signaledPtr(), 0)
			return true
		}
		return false
	}
}

// Reset the event to non-signaled state.
// Only meaningful for ManualReset events.
func (e *Event) Reset() {
	atomic.StoreUint32(e.signaledPtr(), 0)
}

// Pulse signals the event then resets after all waiters wake.
// Only meaningful for ManualReset events.
func (e *Event) Pulse() {
	e.Signal()

	// Wait for all waiters to finish
	for atomic.LoadUint32(e.waitingPtr()) > 0 {
		time.Sleep(time.Microsecond)
	}

	e.Reset()
}

// IsSignaled returns true if the event is currently signaled.
func (e *Event) IsSignaled() bool {
	return atomic.LoadUint32(e.signaledPtr()) == 1
}

// Destroy removes the event and its resources from shared memory.
func (e *Event) Destroy() {
	if e.sem != nil {
		e.sem.Destroy()
	}
	_ = e.memory.Table().Remove(e.name)
}

// Name returns the event name.
func (e *Event) Name() string {
	return e.name
}
