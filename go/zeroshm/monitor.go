package zeroshm


import (
	"fmt"
	"sync/atomic"
	"time"
	"unsafe"
)

// Monitor is a condition variable synchronization primitive across processes.
//
// A Monitor combines a mutex with condition variable semantics, allowing
// threads/processes to wait for conditions to become true. Similar to
// std::condition_variable but works across shared memory.
//
// Binary layout:
//   - Mutex (via Semaphore) - named `<name>_mtx`
//   - Semaphore for condition signaling - named `<name>_cond`
//   - waiting_count: uint32 (atomic, 4 bytes) - named `<name>_count`
//
// Example:
//
//	mem, _ := NewMemory("/sync", 1024*1024, 64)
//	mon, _ := NewMonitor(mem, "producer_consumer")
//
//	// Producer
//	mon.Lock()
//	mon.Wait(func() bool { return count < 100 })
//	// ... produce ...
//	mon.NotifyOne()
//	mon.Unlock()
//
//	// Consumer
//	mon.Lock()
//	mon.Wait(func() bool { return count > 0 })
//	// ... consume ...
//	mon.NotifyOne()
//	mon.Unlock()
type Monitor struct {
	memory        *Memory
	name          string
	mutex         *Mutex
	condSem       *Semaphore
	waitingOffset int
}

// NewMonitor creates a new monitor in shared memory.
func NewMonitor(memory *Memory, name string) (*Monitor, error) {
	if err := validateName(name); err != nil {
		return nil, err
	}

	// Check if already exists
	if entry := memory.Find(name); entry != nil {
		return nil, fmt.Errorf("monitor '%s' already exists", name)
	}

	// Create companion mutex
	mutex, err := NewMutex(memory, name+"_mtx")
	if err != nil {
		return nil, fmt.Errorf("failed to create monitor mutex: %w", err)
	}

	// Create condition semaphore (initially 0)
	condSem, err := NewSemaphore(memory, name+"_cond", 0, 0)
	if err != nil {
		return nil, fmt.Errorf("failed to create condition semaphore: %w", err)
	}

	// Create waiting counter
	waitingOffset, err := memory.Allocate(name+"_count", 4)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate monitor counter: %w", err)
	}

	// Initialize counter to 0
	data := memory.Data()
	*(*uint32)(unsafe.Pointer(&data[waitingOffset])) = 0

	// Add marker entry
	_, err = memory.Allocate(name, 1)
	if err != nil {
		return nil, fmt.Errorf("failed to allocate monitor marker: %w", err)
	}

	return &Monitor{
		memory:        memory,
		name:          name,
		mutex:         mutex,
		condSem:       condSem,
		waitingOffset: waitingOffset,
	}, nil
}

// OpenMonitor opens an existing monitor in shared memory.
func OpenMonitor(memory *Memory, name string) (*Monitor, error) {
	entry := memory.Find(name)
	if entry == nil {
		return nil, fmt.Errorf("monitor '%s' not found", name)
	}

	// Open companion mutex
	mutex, err := OpenMutex(memory, name+"_mtx")
	if err != nil {
		return nil, fmt.Errorf("failed to open monitor mutex: %w", err)
	}

	// Open condition semaphore
	condSem, err := OpenSemaphore(memory, name+"_cond")
	if err != nil {
		return nil, fmt.Errorf("failed to open condition semaphore: %w", err)
	}

	// Find waiting counter
	counterEntry := memory.Find(name + "_count")
	if counterEntry == nil {
		return nil, fmt.Errorf("monitor counter not found for '%s'", name)
	}

	return &Monitor{
		memory:        memory,
		name:          name,
		mutex:         mutex,
		condSem:       condSem,
		waitingOffset: int(counterEntry.Offset),
	}, nil
}

// waitingPtr returns a pointer to the atomic waiting counter.
func (m *Monitor) waitingPtr() *uint32 {
	return (*uint32)(unsafe.Pointer(&m.memory.Data()[m.waitingOffset]))
}

// Lock acquires the monitor's mutex.
func (m *Monitor) Lock() {
	m.mutex.Lock()
}

// Unlock releases the monitor's mutex.
func (m *Monitor) Unlock() {
	m.mutex.Unlock()
}

// TryLock tries to acquire the monitor's mutex without blocking.
// Returns true if the lock was acquired, false if it is held.
func (m *Monitor) TryLock() bool {
	return m.mutex.TryLock()
}

// Wait atomically releases the lock and blocks until notified.
// When woken, reacquires the lock.
// Must hold the lock before calling Wait.
func (m *Monitor) Wait() {
	atomic.AddUint32(m.waitingPtr(), 1)

	// Release lock and wait
	m.mutex.Unlock()
	m.condSem.Acquire()

	// Reacquire lock
	m.mutex.Lock()

	// Decrement AFTER reacquiring lock
	atomic.AddUint32(m.waitingPtr(), ^uint32(0))
}

// WaitWithPredicate waits until the predicate returns true.
// Handles spurious wakeups by looping.
// Must hold the lock before calling.
func (m *Monitor) WaitWithPredicate(pred func() bool) {
	for !pred() {
		m.Wait()
	}
}

// WaitTimeoutWithPredicate waits until the predicate returns true or timeout.
// Returns true if the predicate became true, false if timeout expired.
// Must hold the lock before calling.
func (m *Monitor) WaitTimeoutWithPredicate(timeout time.Duration, pred func() bool) bool {
	deadline := time.Now().Add(timeout)

	for !pred() {
		now := time.Now()
		if now.After(deadline) {
			return false
		}

		remaining := deadline.Sub(now)
		if !m.waitTimeout(remaining) {
			return pred()
		}
	}

	return true
}

// waitTimeout waits with timeout, releasing and reacquiring the lock.
// Returns true if notified, false if timeout expired.
func (m *Monitor) waitTimeout(timeout time.Duration) bool {
	atomic.AddUint32(m.waitingPtr(), 1)

	// Release lock and wait with timeout
	m.mutex.Unlock()

	notified := m.condSem.AcquireTimeout(timeout)

	// Reacquire lock
	m.mutex.Lock()

	// Decrement AFTER reacquiring lock
	atomic.AddUint32(m.waitingPtr(), ^uint32(0))

	return notified
}

// NotifyOne wakes one waiting thread/process.
func (m *Monitor) NotifyOne() {
	if m.condSem.Waiting() > 0 || atomic.LoadUint32(m.waitingPtr()) > 0 {
		_ = m.condSem.Release()
	}
}

// NotifyAll wakes all waiting threads/processes.
func (m *Monitor) NotifyAll() {
	count := atomic.LoadUint32(m.waitingPtr())
	for i := uint32(0); i < count; i++ {
		_ = m.condSem.Release()
	}
}

// Destroy removes the monitor and its resources from shared memory.
func (m *Monitor) Destroy() {
	if m.mutex != nil {
		m.mutex.Destroy()
	}
	if m.condSem != nil {
		m.condSem.Destroy()
	}
	_ = m.memory.Table().Remove(m.name + "_count")
	_ = m.memory.Table().Remove(m.name)
}

// Name returns the monitor name.
func (m *Monitor) Name() string {
	return m.name
}
