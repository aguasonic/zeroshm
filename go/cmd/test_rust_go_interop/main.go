// ZeroSHM Cross-Language Interop Tests (Part 1)
// Go side of Rust <-> Go interop verification.
//
// Usage:
//   go run . <command> <shm_name> [args...]
//
// Commands:
//   array_create <shm> <name> <capacity> <values...>  - Create array, write values
//   array_read <shm> <name>                            - Read array, print values
//   array_write <shm> <name> <values...>               - Write values to existing array
//   queue_create <shm> <name> <capacity>               - Create queue
//   queue_push <shm> <name> <values...>                - Push values to queue
//   queue_pop <shm> <name> <count>                     - Pop N values from queue
//   mutex_create <shm> <name>                          - Create mutex
//   mutex_lock <shm> <name>                            - Lock mutex
//   mutex_unlock <shm> <name>                          - Unlock mutex
//   barrier_create <shm> <name> <participants>         - Create barrier
//   barrier_wait <shm> <name>                          - Wait at barrier
//   latest_create <shm> <name> <initial_value>         - Create Latest with initial value
//   latest_get <shm> <name>                            - Read current value and version
//   latest_set <shm> <name> <value>                    - Set new value
//   latest_wait <shm> <name> <last_version> <timeout_ms> - Wait for change
//   cleanup <shm>                                      - Unlink shared memory

package main

import (
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/aguasonic/zeroshm/zeroshm"
)

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintln(os.Stderr, "Usage: test_rust_go_interop <command> <shm_name> [args...]")
		os.Exit(1)
	}

	command := os.Args[1]
	shmName := os.Args[2]
	args := os.Args[3:]

	var err error
	switch command {
	case "array_create":
		err = cmdArrayCreate(shmName, args)
	case "array_read":
		err = cmdArrayRead(shmName, args)
	case "array_write":
		err = cmdArrayWrite(shmName, args)
	case "queue_create":
		err = cmdQueueCreate(shmName, args)
	case "queue_push":
		err = cmdQueuePush(shmName, args)
	case "queue_pop":
		err = cmdQueuePop(shmName, args)
	case "mutex_create":
		err = cmdMutexCreate(shmName, args)
	case "mutex_lock":
		err = cmdMutexLock(shmName, args)
	case "mutex_unlock":
		err = cmdMutexUnlock(shmName, args)
	case "barrier_create":
		err = cmdBarrierCreate(shmName, args)
	case "barrier_wait":
		err = cmdBarrierWait(shmName, args)
	case "latest_create":
		err = cmdLatestCreate(shmName, args)
	case "latest_get":
		err = cmdLatestGet(shmName, args)
	case "latest_set":
		err = cmdLatestSet(shmName, args)
	case "latest_wait":
		err = cmdLatestWait(shmName, args)
	case "cleanup":
		err = cmdCleanup(shmName)
	default:
		fmt.Fprintf(os.Stderr, "Unknown command: %s\n", command)
		os.Exit(1)
	}

	if err != nil {
		fmt.Fprintf(os.Stderr, "ERROR: %v\n", err)
		os.Exit(1)
	}
}

func ensureName(name string) string {
	if len(name) > 0 && name[0] == '/' {
		return name
	}
	return "/" + name
}

func cmdArrayCreate(shmName string, args []string) error {
	if len(args) < 3 {
		return fmt.Errorf("Usage: array_create <shm> <name> <capacity> <values...>")
	}

	name := ensureName(args[0])
	capacity, err := strconv.Atoi(args[1])
	if err != nil {
		return fmt.Errorf("Invalid capacity: %v", err)
	}
	var values []float64
	for _, v := range args[2:] {
		f, err := strconv.ParseFloat(v, 64)
		if err != nil {
			return fmt.Errorf("Invalid value '%s': %v", v, err)
		}
		values = append(values, f)
	}

	shm := ensureName(shmName)
	zeroshm.UnlinkName(shm)

	memory, err := zeroshm.NewMemory(shm, 1024*1024, 64)
	if err != nil {
		return fmt.Errorf("Failed to create memory: %v", err)
	}
	defer memory.Close()

	array, err := zeroshm.NewArray[float64](memory, name, capacity)
	if err != nil {
		return fmt.Errorf("Failed to create array: %v", err)
	}

	for i, v := range values {
		array.Set(i, v)
	}

	fmt.Printf("Go: Created array '%s' with %d values\n", name, len(values))
	for i, v := range values {
		fmt.Printf("Go: array[%d] = %v\n", i, v)
	}

	return nil
}

func cmdArrayRead(shmName string, args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("Usage: array_read <shm> <name>")
	}

	name := ensureName(args[0])
	shm := ensureName(shmName)

	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	array, err := zeroshm.OpenArray[float64](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open array: %v", err)
	}

	fmt.Printf("Go: Opened array '%s', capacity=%d\n", name, array.Capacity())
	for i := 0; i < array.Capacity(); i++ {
		v := array.Get(i)
		fmt.Printf("Go: array[%d] = %v\n", i, v)
	}

	return nil
}

func cmdArrayWrite(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: array_write <shm> <name> <values...>")
	}

	name := ensureName(args[0])
	var values []float64
	for _, v := range args[1:] {
		f, err := strconv.ParseFloat(v, 64)
		if err != nil {
			return fmt.Errorf("Invalid value '%s': %v", v, err)
		}
		values = append(values, f)
	}

	shm := ensureName(shmName)
	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	array, err := zeroshm.OpenArray[float64](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open array: %v", err)
	}

	for i, v := range values {
		array.Set(i, v)
	}

	fmt.Printf("Go: Wrote %d values to array '%s'\n", len(values), name)
	return nil
}

func cmdQueueCreate(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: queue_create <shm> <name> <capacity>")
	}

	name := ensureName(args[0])
	capacity, err := strconv.Atoi(args[1])
	if err != nil {
		return fmt.Errorf("Invalid capacity: %v", err)
	}

	shm := ensureName(shmName)
	zeroshm.UnlinkName(shm)

	memory, err := zeroshm.NewMemory(shm, 1024*1024, 64)
	if err != nil {
		return fmt.Errorf("Failed to create memory: %v", err)
	}
	defer memory.Close()

	queue, err := zeroshm.NewQueue[uint32](memory, name, capacity)
	if err != nil {
		return fmt.Errorf("Failed to create queue: %v", err)
	}

	fmt.Printf("Go: Created queue '%s', capacity=%d\n", name, queue.Capacity())
	return nil
}

func cmdQueuePush(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: queue_push <shm> <name> <values...>")
	}

	name := ensureName(args[0])
	var values []uint32
	for _, v := range args[1:] {
		u, err := strconv.ParseUint(v, 10, 32)
		if err != nil {
			return fmt.Errorf("Invalid value '%s': %v", v, err)
		}
		values = append(values, uint32(u))
	}

	shm := ensureName(shmName)
	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	queue, err := zeroshm.OpenQueue[uint32](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open queue: %v", err)
	}

	for _, v := range values {
		if !queue.Push(v) {
			return fmt.Errorf("Failed to push %d (queue full)", v)
		}
	}

	fmt.Printf("Go: Pushed %d values to queue '%s'\n", len(values), name)
	return nil
}

func cmdQueuePop(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: queue_pop <shm> <name> <count>")
	}

	name := ensureName(args[0])
	count, err := strconv.Atoi(args[1])
	if err != nil {
		return fmt.Errorf("Invalid count: %v", err)
	}

	shm := ensureName(shmName)
	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	queue, err := zeroshm.OpenQueue[uint32](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open queue: %v", err)
	}

	for i := 0; i < count; i++ {
		v, ok := queue.Pop()
		if !ok {
			fmt.Printf("Go: Queue empty\n")
			return fmt.Errorf("Failed to pop all values")
		}
		fmt.Printf("Go: Popped %d from queue '%s'\n", v, name)
	}

	return nil
}

func cmdMutexCreate(shmName string, args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("Usage: mutex_create <shm> <name>")
	}

	name := ensureName(args[0])
	shm := ensureName(shmName)
	zeroshm.UnlinkName(shm)

	memory, err := zeroshm.NewMemory(shm, 1024*1024, 64)
	if err != nil {
		return fmt.Errorf("Failed to create memory: %v", err)
	}
	defer memory.Close()

	mutex, err := zeroshm.NewMutex(memory, name)
	if err != nil {
		return fmt.Errorf("Failed to create mutex: %v", err)
	}
	_ = mutex

	fmt.Printf("Go: Created mutex '%s'\n", name)
	return nil
}

func cmdMutexLock(shmName string, args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("Usage: mutex_lock <shm> <name>")
	}

	name := ensureName(args[0])
	shm := ensureName(shmName)

	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	mutex, err := zeroshm.OpenMutex(memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open mutex: %v", err)
	}

	mutex.Lock()
	fmt.Printf("Go: Locked mutex '%s'\n", name)

	// Hold the lock for a moment so another process can detect it's locked
	time.Sleep(100 * time.Millisecond)

	mutex.Unlock()
	return nil
}

func cmdMutexUnlock(shmName string, args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("Usage: mutex_unlock <shm> <name>")
	}

	name := ensureName(args[0])
	shm := ensureName(shmName)

	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	mutex, err := zeroshm.OpenMutex(memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open mutex: %v", err)
	}

	// Try to lock and unlock (verifies cross-language unlock worked)
	mutex.Lock()
	fmt.Printf("Go: Acquired mutex '%s' (was unlocked by other language)\n", name)
	mutex.Unlock()
	return nil
}

func cmdBarrierCreate(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: barrier_create <shm> <name> <participants>")
	}

	name := ensureName(args[0])
	participants, err := strconv.ParseInt(args[1], 10, 32)
	if err != nil {
		return fmt.Errorf("Invalid participants: %v", err)
	}

	shm := ensureName(shmName)
	zeroshm.UnlinkName(shm)

	memory, err := zeroshm.NewMemory(shm, 1024*1024, 64)
	if err != nil {
		return fmt.Errorf("Failed to create memory: %v", err)
	}
	defer memory.Close()

	barrier, err := zeroshm.NewBarrier(memory, name, int32(participants))
	if err != nil {
		return fmt.Errorf("Failed to create barrier: %v", err)
	}
	_ = barrier

	fmt.Printf("Go: Created barrier '%s' with %d participants\n", name, participants)
	return nil
}

func cmdBarrierWait(shmName string, args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("Usage: barrier_wait <shm> <name>")
	}

	name := ensureName(args[0])
	shm := ensureName(shmName)

	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	barrier, err := zeroshm.OpenBarrier(memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open barrier: %v", err)
	}

	fmt.Printf("Go: Waiting at barrier '%s'...\n", name)
	barrier.Wait()
	fmt.Printf("Go: Barrier '%s' released\n", name)

	return nil
}

func cmdLatestCreate(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: latest_create <shm> <name> <initial_value>")
	}

	name := ensureName(args[0])
	initialValue, err := strconv.ParseUint(args[1], 10, 64)
	if err != nil {
		return fmt.Errorf("Invalid value: %v", err)
	}

	shm := ensureName(shmName)
	zeroshm.UnlinkName(shm)

	memory, err := zeroshm.NewMemory(shm, 1024*1024, 64)
	if err != nil {
		return fmt.Errorf("Failed to create memory: %v", err)
	}
	defer memory.Close()

	latest, err := zeroshm.NewLatest[uint64](memory, name, uint64(initialValue))
	if err != nil {
		return fmt.Errorf("Failed to create Latest: %v", err)
	}
	_ = latest

	fmt.Printf("Go: Created Latest '%s' with value %d\n", name, initialValue)
	return nil
}

func cmdLatestGet(shmName string, args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("Usage: latest_get <shm> <name>")
	}

	name := ensureName(args[0])
	shm := ensureName(shmName)

	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	latest, err := zeroshm.OpenLatest[uint64](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open Latest: %v", err)
	}

	value, version := latest.Get()
	fmt.Printf("Go: Latest '%s' value = %d, version = %d\n", name, value, version)
	return nil
}

func cmdLatestSet(shmName string, args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("Usage: latest_set <shm> <name> <value>")
	}

	name := ensureName(args[0])
	value, err := strconv.ParseUint(args[1], 10, 64)
	if err != nil {
		return fmt.Errorf("Invalid value: %v", err)
	}

	shm := ensureName(shmName)
	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	latest, err := zeroshm.OpenLatest[uint64](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open Latest: %v", err)
	}

	latest.Set(uint64(value))
	fmt.Printf("Go: Set Latest '%s' to %d\n", name, value)
	return nil
}

func cmdLatestWait(shmName string, args []string) error {
	if len(args) < 3 {
		return fmt.Errorf("Usage: latest_wait <shm> <name> <last_version> <timeout_ms>")
	}

	name := ensureName(args[0])
	lastVersion, err := strconv.ParseUint(args[1], 10, 64)
	if err != nil {
		return fmt.Errorf("Invalid version: %v", err)
	}
	timeoutMs, err := strconv.ParseInt(args[2], 10, 64)
	if err != nil {
		return fmt.Errorf("Invalid timeout: %v", err)
	}

	shm := ensureName(shmName)
	memory, err := zeroshm.OpenMemory(shm, 64)
	if err != nil {
		return fmt.Errorf("Failed to open memory: %v", err)
	}
	defer memory.Close()

	latest, err := zeroshm.OpenLatest[uint64](memory, name)
	if err != nil {
		return fmt.Errorf("Failed to open Latest: %v", err)
	}

	timeout := time.Duration(timeoutMs) * time.Millisecond
	value, version, changed := latest.WaitForChange(uint64(lastVersion), timeout)

	if changed {
		fmt.Printf("Go: Latest '%s' changed: value = %d, version = %d\n", name, value, version)
	} else {
		fmt.Printf("Go: Latest '%s' timeout: value = %d, version = %d\n", name, value, version)
	}
	return nil
}

func cmdCleanup(shmName string) error {
	shm := ensureName(shmName)
	zeroshm.UnlinkName(shm)
	fmt.Printf("Go: Cleaned up %s\n", shm)
	return nil
}

