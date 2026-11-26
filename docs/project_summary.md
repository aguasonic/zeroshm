# ZeroSHM Project Summary

**High-performance cross-language shared memory system.**

ZeroIPC is a library for zero-copy data sharing between processes on a single machine, with parallel native implementations in **C++23**, **Go**, and **Rust**. No serialization, no bindings -- each language implementation reads and writes the same binary format directly.

---

## Core Philosophy

The project is built on two foundational documents:

### The Single-Machine Thesis

Shared memory IPC on a single machine is a *fundamentally different computational model* than distributed systems over a network:

- **Guaranteed delivery** -- writes to RAM always succeed; no packet loss
- **Bounded latency** -- 50-200ns vs 50-100μs for TCP localhost
- **Causal ordering** -- provided by CPU memory fences, not vector clocks
- **Shared mutable state** -- processes share actual bits, not copies

This enables **100-1000x latency improvement** over network-style IPC by eliminating serialization, protocol overhead, and kernel involvement.

### Design Philosophy: Simplicity Through Constraints

ZeroIPC makes deliberate constraints that enable extraordinary simplicity and performance:

- **Stack/bump allocation** -- O(1), no fragmentation, no individual deallocation
- **Fixed-size tables** with compile-time limits
- **Minimal metadata** -- hash + offset + size + name, no type information
- **Trivially-copyable types only** -- zero-copy, no serialization

Explicitly rejected: complex allocators, defragmentation, serialization, network transparency, persistence.

---

## Architecture

### Shared Memory Management

All three languages use **POSIX shared memory** (`shm_open` + `mmap`) with identical APIs:

| Operation | Description |
|-----------|-------------|
| `create()`/`new()` | Creates a segment, zeros memory, initializes table |
| `open()` | Opens an existing segment |
| `allocate(name, size)` | Reserves space via the table |
| `find(name)` | Looks up an entry by name |

### Binary Format (The Cross-Language Contract)

The **Table** at the beginning of shared memory enables cross-language compatibility:

**Header (32 bytes):**
- magic: `uint32` (0x5A53484D = 'ZSHM')
- version: `uint32` (3)
- entry_count: `uint32`
- max_entries: `uint32`
- memory_size: `uint64`
- next_offset: `uint64`

**Entry (384 bytes):**
- hash: `uint64` (xxhash3-64 of name, seed 0x2FE7)
- offset: `uint64`
- size: `uint64`
- name: `[256]byte` (null-terminated, max 254 chars)
- padding: 104 bytes

All integers are **little-endian**. The 384-byte Entry size is identical across C++, Go, and Rust.

**Hash-first lookup**: computes xxhash3-64 of the name (8-byte comparison) before scanning entries, with full string comparison as a collision fallback.

### Data Structures

The library provides **8 core + 9 sync + 4 codata** structures:

**Core Data Structures** (all three languages):
- `Array<T>` -- contiguous typed array
- `Queue<T>` -- lock-free MPMC (Vyukov algorithm)
- `Stack<T>` -- LIFO structure
- `Map<K,V>` / `Table` -- key-value lookup
- `Set<T>` -- unique elements
- `Ring<T>` -- circular buffer (SPSC)
- `Pool<T>` -- object pool with free list

**Synchronization Primitives**:
- `Mutex`, `RwLock`, `Semaphore` -- cross-process locks
- `Barrier`, `Latch`, `Once` -- coordination
- `Event`, `Signal` -- notification

**Co-data Structures** (C++ and Rust):
- `Channel<T>` -- producer-consumer stream
- `Stream<T>` -- append-only log
- `Future<T>` -- async result
- `Lazy<T>` -- on-demand initialization

### The Vyukov MPMC Queue

The flagship data structure, claiming **30ns latency** and **119M ops/sec**:

- **Lock-free** multi-producer multi-consumer bounded queue
- **Binary layout**: Header (16 bytes: head, tail, capacity, elem_size) + data array + 8-aligned sequence array
- **Capacity** rounded up to power of two for wraparound safety
- **Algorithm**:
  - Push: CAS tail → write data → publish via sequence store (release)
  - Pop: CAS head → read data → release slot via sequence store
- **Per-slot sequence numbers** enable lock-free coordination without locks

---

## Language-Specific Details

| Aspect | C++ | Go | Rust |
|--------|-----|----|----|
| **Memory** | `Memory` class, non-copyable/movable | `Memory` struct, `unsafe.Pointer` | `Memory` struct, `Arc<AtomicUsize>` refcounting |
| **Type Safety** | Templates + `static_assert` | Generics + `Numeric` constraint | Generics + `Pod` trait |
| **Platform** | POSIX (Linux/macOS) | Linux `/dev/shm`, macOS `shm_open` | Platform-specific module (Linux/macOS) |
| **Safety** | Raw pointers, manual bounds | `unsafe` for atomic access | `unsafe` blocks, `Send + Sync` impls |
| **Hash** | `detail/xxhash.h` | `github.com/zeebo/xxh3` | `xxhash-rust` |

---

## Cross-Language Interop

The `interop/` directory contains tests for **all language pairs**:

- `test_cpp_go_interop.sh` -- C++ ↔ Go
- `test_go_cpp_interop.sh` -- Go ↔ C++
- `test_rust_cpp_interop.sh` -- Rust ↔ C++
- `test_rust_go_interop.sh` -- Rust ↔ Go

Interop tests use a writer/reader pattern: one language creates shared memory and writes data structures, another language opens the same segment and reads the data. This validates binary format compatibility.

---

## Build System

Top-level `Makefile` orchestrates all implementations:

```bash
make all          # Build C++, Go, Rust
make cpp          # Build C++ (CMake)
make go           # Build Go
make rust         # Build Rust (Cargo)
make test         # Run all tests
make test-cpp     # Run C++ tests (ctest)
make test-go      # Run Go tests
make test-rust    # Run Rust tests (cargo test)
make interop      # Run cross-language interop tests
make cli          # Build Go CLI tool
make clean        # Clean all build artifacts
```

---

## Performance Claims

| Metric | Value |
|--------|-------|
| Queue latency | 30ns (Vyukov MPMC) |
| Queue throughput | 119M ops/sec single-thread |
| Array bandwidth | 14.4 GB/s read |

Real benchmarks comparing communication patterns:

| Method | Latency | Throughput |
|--------|---------|------------|
| TCP localhost | 50-100 μs | 2-5 GB/s |
| Unix socket | 20-50 μs | 5-10 GB/s |
| Pipe | 10-30 μs | 3-6 GB/s |
| Shared memory | 50-200 ns | 40+ GB/s |
| ZeroIPC queue | 100-150 ns | 8M+ ops/s |

---

## When to Use

**Use ZeroIPC if you have:**
- Multiple processes on a single node
- Performance-critical requirements (nanosecond latency)
- Known data structures at compile time
- Need for zero-overhead reads
- Simulation or real-time systems

**Don't use if you need:**
- General-purpose shared memory allocation
- Dynamic structure creation/destruction
- Network-distributed communication
- Arbitrary container types

---

## Key Takeaway

ZeroIPC demonstrates that **constraints are features**: by deliberately limiting scope (single-machine, trivially-copyable types, stack allocation, fixed tables), it achieves extraordinary simplicity and performance that general-purpose systems cannot match. The 384-byte Entry format and Vyukov queue algorithm are replicated identically across C++, Go, and Rust, enabling true zero-copy IPC between processes written in different languages.

---

*Validated by production systems: Unity's Burst Compiler/Job System, LMAX Disruptor, NASA JPL Coding Standard.*
