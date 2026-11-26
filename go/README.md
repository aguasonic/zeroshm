# ZeroSHM Go Implementation

Go implementation of ZeroSHM - high-performance cross-language shared memory system.

## Requirements

- Go 1.21 or later (for generics support)
- Linux (uses `/dev/shm` for POSIX shared memory)

## Platform support

The shared-memory backend is **Linux only** (names are mapped under `/dev/shm`).
The package compiles on every `GOOS`, but on non-Linux targets `NewMemory` and
`OpenMemory` return a clear error instead of failing the build:

```
zeroshm: shared memory requires Linux (POSIX shared memory via /dev/shm);
GOOS "windows" is not supported (contributions welcome)
```

Adding a backend for another OS means implementing the small set of methods in
`memory_linux.go` (`create`, `open`, `Close`, `Unlink`, `UnlinkName`) in a
build-tagged file such as `memory_windows.go`; that file already documents the
Win32 file-mapping approach. Note that *cross-language* interop additionally
requires the C++/Rust implementations to use the same OS shared-memory
scheme, so a Go-only backend would be Go-to-Go on that platform.

## Installation

```bash
go get github.com/aguasonic/zeroshm
```

## Quick Start

### Creating Shared Memory

```go
package main

import (
    "fmt"
    "github.com/aguasonic/zeroshm/zeroshm"
)

func main() {
    // Create new shared memory (10MB, 64 entries)
    mem, err := zeroshm.NewMemory("/mydata", 10*1024*1024, 64)
    if err != nil {
        panic(err)
    }
    defer mem.Close()

    // Create an array of floats
    temps, _ := zeroshm.NewArray[float32](mem, "temperatures", 1000)
    temps.Set(0, 23.5)
    temps.Set(1, 24.0)

    fmt.Println("Created temperatures array")
}
```

### Opening Existing Shared Memory

```go
// Open existing shared memory created by another process
mem, err := zeroshm.OpenMemory("/mydata", 10*1024*1024)
if err != nil {
    panic(err)
}
defer mem.Close()

// Open existing array
temps, _ := zeroshm.OpenArray[float32](mem, "temperatures")
fmt.Printf("Temperature: %.1f\n", temps.Get(0))
```

## Data Structures

### Array[T]
Fixed-size contiguous storage with type safety via generics.

```go
arr, _ := zeroshm.NewArray[float64](mem, "data", 100)
arr.Set(0, 3.14159)
val := arr.Get(0)
```

### Queue[T]
Lock-free MPMC circular buffer.

```go
q, _ := zeroshm.NewQueue[int32](mem, "events", 1000)
q.Push(42)
if val, ok := q.Pop(); ok {
    fmt.Println(val)
}
```

### Stack[T]
Lock-free LIFO stack.

```go
s, _ := zeroshm.NewStack[int64](mem, "history", 100)
s.Push(100)
if val, ok := s.Pop(); ok {
    fmt.Println(val)
}
```

### Semaphore
Cross-process counting semaphore.

```go
sem, _ := zeroshm.NewSemaphore(mem, "permits", 5, 10)
sem.Acquire()  // Blocks if count is 0
sem.Release()
```

### Mutex
Binary semaphore for mutual exclusion.

```go
mtx, _ := zeroshm.NewMutex(mem, "lock")
mtx.Lock()
// Critical section
mtx.Unlock()
```

### Barrier
Multi-process synchronization point.

```go
barrier, _ := zeroshm.NewBarrier(mem, "sync_point", 4)
barrier.Wait()  // Blocks until 4 processes arrive
```

### Latch
One-shot countdown synchronization.

```go
latch, _ := zeroshm.NewLatch(mem, "ready", 3)
latch.CountDownOne()  // Decrement count
latch.Wait()          // Block until count reaches 0
```

### Once
One-time initialization primitive.

```go
once, _ := zeroshm.NewOnce(mem, "init")
once.Do(func() {
    // Executes exactly once across all processes
    initializeResources()
})
```

### Latest[T]
Single-producer, multi-reader value with change detection. Supports true OS-level blocking reads via pthread condition variables -- readers sleep (zero CPU) until the writer signals.

```go
// Producer
latest, _ := zeroshm.NewLatest[uint64](mem, "counter", 0)
latest.Set(42)

// Consumer
latest, _ := zeroshm.OpenLatest[uint64](mem, "counter")
val, version, changed := latest.WaitForChange(0, 5*time.Second)
if changed {
    fmt.Printf("New value: %d (version %d)\n", val, version)
}
```

## Supported Types

The `Numeric` constraint supports:
- `int8`, `int16`, `int32`, `int64`
- `uint8`, `uint16`, `uint32`, `uint64`
- `float32`, `float64`

## CLI Tool

Build and use the CLI for inspecting shared memory:

```bash
# Build
go build -o zeroshm ./cmd/zeroshm

# List shared memory segments
./zeroshm list

# Show segment info
./zeroshm info /mydata

# List all entries
./zeroshm entries /mydata

# Inspect structures
./zeroshm array /mydata temperatures
./zeroshm queue /mydata events
./zeroshm semaphore /mydata permits
```

## Interoperability

The Go implementation is binary-compatible with C++ and Rust. All use the same memory layout:

- **Table Header**: 32 bytes (magic, version, count, max_entries, size, next_offset)
- **Table Entry**: 384 bytes (hash, offset, size, name[256], padding)
- **Array Header**: 8 bytes (capacity)
- **Queue Header**: 16 bytes (head, tail, capacity, elem_size)
- **Stack Header**: 16 bytes (top, capacity, elem_size, reserved)

Format v2: side arrays (the queue's sequence array, the stack's slot-state
array) start at `align8(header_size + elem_size * capacity)`. See
[SPECIFICATION.md](../SPECIFICATION.md) for the authoritative layout.

## Running Tests

```bash
# Unit tests
go test ./zeroshm/... -v

# Interop tests (requires C++ toolchain)
go run ./cmd/interop
```

## Binary Format

See [SPECIFICATION.md](../SPECIFICATION.md) for the complete binary format specification that all language implementations follow.

## License

MIT
