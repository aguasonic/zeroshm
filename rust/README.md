# ZeroSHM Rust Implementation

Rust implementation of ZeroSHM - high-performance cross-language shared memory IPC.

## Requirements

- Rust 1.70 or later (stable)
- Linux or macOS (uses POSIX shared memory: `shm_open`/`mmap`)

## Platform support

The shared-memory backend uses POSIX shared memory (`shm_open`/`mmap`), which is
available on Linux and macOS. On other platforms, `Memory::create` and
`Memory::open` return a clear error at runtime.

Cross-language interop requires all implementations (C++, Go, Rust) to use the
same OS shared-memory scheme, so cross-language tests are supported on Linux
and macOS.

## Installation

```bash
# As a dependency in your Cargo.toml
[dependencies]
zeroshm = { git = "https://github.com/aguasonic/zeroshm", path = "rust/zeroshm" }
```

Or for local development:

```bash
cd rust/zeroshm
cargo build
```

## Quick Start

### Creating Shared Memory

```rust
use zeroshm::{Memory, Array};

fn main() -> zeroshm::Result<()> {
    // Create new shared memory (10MB, 64 entries)
    let mem = Memory::create("/mydata", 10 * 1024 * 1024, 64)?;

    // Create an array of f32
    let temps = Array::<f32>::create(&mem, "temperatures", 1000)?;
    temps.set(0, 23.5);
    temps.set(1, 24.0);

    println!("Created temperatures array");
    Ok(())
}
```

### Opening Existing Shared Memory

```rust
use zeroshm::{Memory, Array};

fn main() -> zeroshm::Result<()> {
    // Open existing shared memory created by another process
    let mem = Memory::open("/mydata", 10 * 1024 * 1024)?;

    // Open existing array
    let temps = Array::<f32>::open(&mem, "temperatures")?;
    println!("Temperature: {:.1}", temps.get(0));

    Ok(())
}
```

## Data Structures

### Array\<T\>
Fixed-size contiguous storage with type safety via generics and the `Pod` trait.

```rust
let arr = Array::<f64>::create(&mem, "data", 100)?;
arr.set(0, 3.14159);
let val = arr.get(0);
```

### Queue\<T\>
Lock-free MPMC circular buffer (Vyukov algorithm).

```rust
let q = Queue::<i32>::create(&mem, "events", 1000)?;
q.push(42);
if let Some(val) = q.pop() {
    println!("{val}");
}
```

### Stack\<T\>
Lock-free LIFO stack (3-state CAS algorithm).

```rust
let s = Stack::<i64>::create(&mem, "history", 100)?;
s.push(100);
if let Some(val) = s.pop() {
    println!("{val}");
}
```

### Ring\<T\>
Lock-free SPSC ring buffer.

```rust
let ring = Ring::<u32>::create(&mem, "ring_buf", 1000)?;
ring.write(42);
if let Some(val) = ring.read() {
    println!("{val}");
}
```

### Map, Set, Pool
Additional data structures for key-value storage, unique element sets, and object pooling.

## Synchronization Primitives

### Semaphore
Cross-process counting semaphore.

```rust
let sem = Semaphore::create(&mem, "permits", 5, 10)?;
sem.acquire();  // Blocks if count is 0
sem.release();
```

### Mutex
Binary semaphore for mutual exclusion.

```rust
let mtx = Mutex::create(&mem, "lock")?;
let guard = mtx.lock();
// Critical section
drop(guard);
```

### Barrier
Multi-process synchronization point.

```rust
let barrier = Barrier::create(&mem, "sync_point", 4)?;
barrier.wait();  // Blocks until 4 processes arrive
```

### Latch
One-shot countdown synchronization.

```rust
let latch = Latch::create(&mem, "ready", 3)?;
latch.count_down_one();  // Decrement count
latch.wait();            // Block until count reaches 0
```

### Once
One-time initialization primitive.

```rust
let once = Once::create(&mem, "init")?;
once.call(|| {
    // Executes exactly once across all processes
    initialize_resources();
});
```

### Event
Auto-reset or manual-reset event for signaling.

```rust
let event = Event::create(&mem, "trigger", EventMode::ManualReset)?;
event.signal();
event.wait();  // Blocks until signaled
```

### RWLock
Read-write lock for read-heavy workloads.

```rust
let rwlock = RWLock::create(&mem, "rw")?;
let read_guard = rwlock.reader_lock();
// Multiple readers can hold this simultaneously
drop(read_guard);

let write_guard = rwlock.writer_lock();
// Exclusive write access
drop(write_guard);
```

### Monitor
Mutex + condition variable for predicate-based waiting.

```rust
let monitor = Monitor::create(&mem, "monitor")?;
let guard = monitor.lock();
// Critical section with wait/notify
```

### Signal\<T\>
Reactive signal with version tracking.

```rust
let signal = Signal::<i32>::create(&mem, "value", 0)?;
signal.set(42);
let val = signal.get();
```

### Latest\<T\>
Single-producer, multi-reader value with change detection. Supports true OS-level blocking reads via pthread condition variables -- readers sleep (zero CPU) until the writer signals.

```rust
// Producer
let latest = Latest::<u64>::create(&mem, "counter", 0)?;
latest.set(42);

// Consumer
let latest = Latest::<u64>::open(&mem, "counter")?;
let (val, version, changed) = latest.wait_for_change(0, Duration::from_secs(5));
if changed {
    println!("New value: {val} (version {version})");
}
```

## Codata Structures

### Channel\<T\>
Lock-free bounded channel for producer-consumer patterns.

```rust
let ch = Channel::<i32>::create(&mem, "channel", 100)?;
ch.send(42);
if let Some(val) = ch.recv() {
    println!("{val}");
}
```

### Future\<T\>
One-shot async result placeholder.

```rust
let future = Future::<i32>::create(&mem, "result")?;
future.set(42);
let val = future.get();
```

### Lazy\<T\>
Thread-safe lazy initialization.

```rust
let lazy = Lazy::<i32>::create(&mem, "computed")?;
let val = lazy.get(|| expensive_computation());  // Computed once
```

### Stream\<T\>
Append-only log for reactive data processing.

```rust
let stream = Stream::<f64>::create(&mem, "sensor_data", 1000)?;
stream.emit(23.5);
if let Some(val) = stream.next() {
    println!("{val}");
}
```

## Supported Types

Types must be `Pod` (plain old data: trivially copyable, no padding issues).
Common supported types:

- `i8`, `i16`, `i32`, `i64`, `isize`
- `u8`, `u16`, `u32`, `u64`, `usize`
- `f32`, `f64`
- `bool`
- Fixed-size structs and arrays of the above

## CLI Tool

The Rust implementation includes a CLI tool for inspecting shared memory:

```bash
# Build
cd rust
cargo build --release -p zeroshm-cli

# List shared memory segments
./target/release/zeroshm-cli list

# Show segment info
./target/release/zeroshm-cli info /mydata

# List all entries
./target/release/zeroshm-cli entries /mydata

# Inspect structures
./target/release/zeroshm-cli array /mydata temperatures
./target/release/zeroshm-cli queue /mydata events
```

## Interoperability

The Rust implementation is binary-compatible with C++ and Go. All use the same memory layout:

- **Table Header**: 32 bytes (magic, version, entry_count, max_entries, memory_size, next_offset)
- **Table Entry**: 384 bytes (hash, offset, size, name[256], padding)
- **Array Header**: 8 bytes (capacity)
- **Queue Header**: 16 bytes (head, tail, capacity, elem_size) + data + sequence array
- **Stack Header**: 16 bytes (top, capacity, elem_size, reserved) + data + state array

The hash seed, magic number, and structure sizes are defined in `constants.pkl` at the
project root and generated into `src/constants_generated.rs` via `make generate`.

Format v3: entries use xxhash3-64 (seed 0x2FE7) for fast hash-first lookup with
full-string collision resolution. See [SPECIFICATION.md](../SPECIFICATION.md) for the
authoritative layout.

## Running Tests

```bash
cd rust

# Unit tests
cargo test

# Release build
cargo build --release

# Interop tests
cd interop-tests
cargo test
```

## Binary Format

See [SPECIFICATION.md](../SPECIFICATION.md) for the complete binary format specification that all language implementations follow.

## License

MIT
