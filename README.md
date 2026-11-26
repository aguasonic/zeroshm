# ZeroSHM

**High-performance cross-language shared memory system.**

Inspired by the work of Alex Towell, at https://github.com/queelius/zeroipc

This project drops support for C, and Python. Where that is required, write some bindings. 🙂

Zero-copy data sharing between processes in C++, Go, and Rust.

No serialization, no bindings -- parallel native implementations of the same binary format.

## Overview

ZeroSHM provides a binary format for data structures in shared memory that can be accessed from multiple processes and programming languages. All three implementations (C++, Go, Rust) produce bit-identical binary layouts, enabling seamless cross-language interop.

**Key features:**
- **Zero-copy**: Data is accessed directly in shared memory, no serialization
- **Lock-free**: High-performance atomic operations (Vyukov MPMC queue, 3-state CAS stack)
- **Blocking reads**: `Latest[T]` supports true OS-level blocking via pthread condition variables -- readers sleep until the writer signals, consuming zero CPU
- **Cross-language**: C++23, Go, and Rust implementations of the same format
- **Rich data structures**: Arrays, queues, stacks, rings, maps, sets, pools + 10 sync primitives + 4 codata structures

## At a Glance

| Property | Value |
|----------|-------|
| **Languages** | C++23, Go, Rust |
| **Queue latency** | 1.48ns/op (Go), 27ns (C++) |
| **Queue throughput** | 160M ops/sec single-thread (Rust) |
| **Array bandwidth** | 10.8 GB/s read (Rust) |
| **Data structures** | 8 core + 10 sync + 4 codata |
| **Serialization** | None (zero-copy binary format) |

## Building

ZeroSHM uses a top-level `Makefile` that builds all three language implementations. A shared `constants.pkl` file defines the binary format contract (magic number, version, hash seed, sizes) that all implementations must agree on for interoperability.

### Prerequisites

- **C++**: CMake 3.20+, C++23-compatible compiler (Clang 15+ or GCC 13+)
- **Go**: Go 1.21+
- **Rust**: Rust 1.70+ (stable)
- **Pkl**: Required for `make generate` (install via `brew install pkl` on macOS, or see [pkl-lang.org](https://pkl-lang.org))
- **jq**: Required for constant generation (usually pre-installed)

### Build All Languages

```bash
make all
```

This runs the full build pipeline:
1. **`make generate`** -- Evaluates `constants.pkl` and generates language-specific constant files (`constants_generated.h`, `constants_generated.go`, `constants_generated.rs`). This ensures all implementations use identical magic numbers, hash seeds, and structure sizes.
2. **`make cpp`** -- Builds the C++ implementation (header-only library, tests, benchmarks, CLI tool)
3. **`make go`** -- Builds the Go implementation
4. **`make rust`** -- Builds the Rust implementation (release mode)

### Build Individual Languages

```bash
make generate    # Regenerate shared constants from constants.pkl
make cpp         # Build C++ implementation
make go          # Build Go implementation
make rust        # Build Rust implementation
```

### Running Tests

```bash
make test         # Run all tests (C++, Go, Rust)
make test-cpp     # Run C++ tests only (ctest)
make test-go      # Run Go tests only
make test-rust    # Run Rust tests only (cargo test)
```

### Cross-Language Interop

```bash
make interop      # Run cross-language interop tests
```

### Clean

```bash
make clean        # Clean all build artifacts
```

### Help

```bash
make help         # Show all available targets
```

## Examples

The `examples/` directory contains working producer-consumer programs in all three languages, demonstrating how to use ZeroSHM for shared memory IPC.

Each example implements the same scenario: a **temperature sensor simulation** where:
- The **producer** creates a shared memory segment, writes 10 temperature readings to a typed array (`temperatures`), and pushes timestamp events to a lock-free MPMC queue (`events`)
- The **consumer** opens the same shared memory, pops events from the queue, and reads the corresponding temperature values from the array

All three implementations use the same shared memory name (`/zeroshm_example`) and structure names, enabling **cross-language interop** -- you can run a Go producer with a Rust consumer, for example.

### Running the Examples

**C++**
```bash
cd examples/cpp
mkdir build && cd build
cmake ..
make
./producer   # Terminal 1
./consumer   # Terminal 2
```

**Go**
```bash
cd examples/go
go run ./producer    # Terminal 1
go run ./consumer    # Terminal 2
```

**Rust**
```bash
cd examples/rust
cargo run --bin producer   # Terminal 1
cargo run --bin consumer   # Terminal 2
```

See [examples/README.md](examples/README.md) for more details.

## Performance Benchmarks

All benchmarks were run on **Apple M5 Max (18 cores)**.

### Queue Performance

| Metric | C++ | Go | Rust |
|--------|-----|-----|------|
| **Push** | 14.6M ops/sec (4B) | 141M ops/sec (8B) | 160M ops/sec (8B) |
| **Pop** | 16.2M ops/sec (4B) | 144M ops/sec (8B) | 154M ops/sec (8B) |
| **Push Latency** | 27 ns/op | 1.48 ns/op | 4 ns/op |
| **Pop Latency** | 28 ns/op | 1.50 ns/op | 4 ns/op |

**Queue Throughput Scaling (multi-thread):**

| Threads | C++ | Go | Rust |
|---------|-----|-----|------|
| 2 | 19.9M ops/sec | 186 ns/op | 18.6M ops/sec |
| 4 | 15.1M ops/sec | 339 ns/op | 9.2M ops/sec |
| 8 | 9.5M ops/sec | 843 ns/op | 5.4M ops/sec |
| 12 | 9.3M ops/sec | 1383 ns/op | 4.9M ops/sec |

*C++ and Rust concurrent benchmarks report total throughput; Go reports main-thread latency per operation.*

**C++ Queue Throughput by Element Size:**

| Element Size | Push | Pop |
|--------------|------|-----|
| 4B (int) | 14.6M ops/sec | 16.2M ops/sec |
| 64B | 15.3M ops/sec | 15.2M ops/sec |
| 256B | 13.4M ops/sec | 13.2M ops/sec |
| 1KB | 16.1M ops/sec | 14.7M ops/sec |
| 4KB | 12.3M ops/sec | 8.8M ops/sec |

### Stack Performance

| Metric | C++ | Go | Rust |
|--------|-----|-----|------|
| **Push** | 16.9M ops/sec (4B) | 108M ops/sec (8B) | 120M ops/sec (8B) |
| **Pop** | 18.3M ops/sec (4B) | 105M ops/sec (8B) | 131M ops/sec (8B) |
| **Top Latency** | 25 ns/op | 4.33 ns/op | -- |

**Stack Concurrent Throughput:**

| Threads | C++ | Go | Rust |
|---------|-----|-----|------|
| 2 | 10.3M ops/sec | 94 ns/op | 25.1M ops/sec |
| 4 | 9.8M ops/sec | 165 ns/op | 15.5M ops/sec |
| 8 | 6.3M ops/sec | 286 ns/op | 21.0M ops/sec |
| 12 | 6.9M ops/sec | 436 ns/op | 13.3M ops/sec |

*C++ and Rust concurrent benchmarks report total throughput; Go reports main-thread latency per operation.*

**C++ Stack Throughput by Element Size:**

| Element Size | Push | Pop |
|--------------|------|-----|
| 4B (int) | 16.9M ops/sec | 18.3M ops/sec |
| 64B | 16.8M ops/sec | 16.4M ops/sec |
| 256B | 16.0M ops/sec | 15.7M ops/sec |
| 1KB | 28.9M ops/sec | 22.3M ops/sec |

### Array Performance

**Sequential Access Bandwidth:**

*C++ uses 4B (int) elements; Go and Rust use 8B (uint64) elements.*

| Size | C++ Read | C++ Write | Go Read | Go Write | Rust Read | Rust Write |
|------|----------|-----------|---------|----------|-----------|------------|
| 1K | 2.0 GB/s | 2.0 GB/s | 7.94 GB/s | 8.17 GB/s | 7.8 GB/s | 7.8 GB/s |
| 10K | 1.3 GB/s | 1.5 GB/s | 7.95 GB/s | 8.20 GB/s | 8.5 GB/s | 9.0 GB/s |
| 100K | 1.3 GB/s | 1.5 GB/s | 7.93 GB/s | 8.17 GB/s | 9.3 GB/s | 10.0 GB/s |
| 1M | 1.6 GB/s | 1.4 GB/s | 7.71 GB/s | 7.98 GB/s | 10.8 GB/s | 9.8 GB/s |

**Random Access Bandwidth:**

*C++: 100k random ops on 1M array (4B elements). Go/Rust: 8B elements.*

| Operation | C++ | Go | Rust |
|-----------|-----|-----|------|
| Read | 0.74 GB/s | 6.10 GB/s | 10.8 GB/s |
| Write | 0.88 GB/s | 5.65 GB/s | 7.5 GB/s |

**Stride Access:**

*C++ reports ops/sec (100k strided accesses); Go/Rust report ns/pass (1M elements).*

| Stride | C++ (ops/sec) | Go (ns/pass) | Rust (ns/pass) |
|--------|---------------|--------------|----------------|
| 1 | 323.6M | 1,101,872 | 685,739 |
| 8 | -- | 144,726 | 85,599 |
| 16 | 336.7M | -- | -- |
| 64 | 324.7M | 19,499 | 10,672 |

## Constant Generation

The binary format contract is defined in a single `constants.pkl` file at the project root. This file is evaluated by Pkl and used to generate language-specific constant files:

```
constants.pkl → (pkl eval → JSON) →
  cpp/include/zeroshm/constants_generated.h
  go/zeroshm/constants_generated.go
  rust/zeroshm/src/constants_generated.rs
```

The generated files are committed to the repository, so end users don't need Pkl installed to build. Run `make generate` only when you change `constants.pkl`.

### Constants Overview

The `constants.pkl` file defines **76 constants** that govern the entire binary format:

- **Core constants** (5): Magic number (`0x5A53484D`), format version (`3`), hash seed (`0x2FE7`), table header size (`32`), table entry size (`384`)
- **Layout constants** (4): Section alignment (`8`), name field size (`256`), u32 size (`4`), u64 size (`8`)
- **Structure field offsets** (67): Field offsets for all 18 data structure types (table header, queue, ring, stack, map, set, pool, barrier, semaphore, latch, event, once, rwlock, channel, stream, future, lazy)

Every offset and size used in the binary format is defined here, ensuring all three implementations agree on the exact byte layout. The generated files include a "DO NOT EDIT" header — manual edits will be overwritten by the next `make generate`.

### Naming Conventions

Each constant is defined in `constants.pkl` using `camelCase` (e.g., `queueHeadOffset`) and converted to each language's convention during generation:

| Pkl Source | C++ (`constants_generated.h`) | Go (`constants_generated.go`) | Rust (`constants_generated.rs`) |
|------------|-------------------------------|-------------------------------|---------------------------------|
| `queueHeadOffset` | `QUEUE_HEAD_OFFSET` | `QueueHeadOffset` | `QUEUE_HEAD_OFFSET` |
| `u32Size` | `U32_SIZE` | `U32Size` | `U32_SIZE` |
| `latchPadding0Offset` | `LATCH_PADDING0_OFFSET` | `LatchPadding0Offset` | `LATCH_PADDING0_OFFSET` |
| `magic` | `TABLE_MAGIC` (legacy) | `TableMagic` (legacy) | `TABLE_MAGIC` (legacy) |
| `version` | `TABLE_VERSION` (legacy) | `TableVersion` (legacy) | `TABLE_VERSION` (legacy) |
| `xxHashSeed` | `XXHASH_SEED` (legacy) | `XXHashSeed` (legacy) | `XXHASH_SEED` (legacy) |

**Type mapping:**
- Magic number and version: `uint32_t` / `uint32` / `u32`
- Hash seed: `uint64_t` / `uint64` / `u64`
- All sizes and offsets: `std::size_t` / untyped (int) / `usize`

## Project Structure

```
.
├── constants.pkl              # Single source of truth for shared constants
├── Makefile                   # Top-level build (all languages)
├── CMakeLists.txt             # Top-level CMake configuration
├── docs/                      # Documentation
│   ├── SPECIFICATION.md       # Binary format specification v3.0
│   ├── design_philosophy.md   # Design philosophy
│   ├── single_machine_thesis.md
│   └── ...
├── cpp/                       # C++23 implementation
│   ├── include/zeroshm/       # Header-only library
│   ├── tests/                 # Google Test suite
│   ├── benchmarks/            # Performance benchmarks
│   └── tools/                 # CLI inspection tool
├── go/                        # Go implementation
│   ├── zeroshm/               # Core library
│   └── cmd/                   # CLI and interop tools
├── rust/                      # Rust implementation
│   ├── zeroshm/               # Core library (crate)
│   ├── zeroshm-cli/           # CLI tool
│   └── interop-tests/         # Cross-language tests
├── examples/                  # Example programs (C++, Go, Rust)
│   ├── cpp/                   # C++ producer/consumer
│   ├── go/                    # Go producer/consumer
│   └── rust/                  # Rust producer/consumer
├── interop/                   # Cross-language interop tests
└── scripts/                   # Build scripts (generate_constants.sh, check_ascii.sh, normalize_ascii.sh)
```

## Documentation

- [Binary format specification](docs/SPECIFICATION.md) -- The definitive reference for the binary layout
- [Design philosophy](docs/design_philosophy.md) -- The deliberate constraints behind ZeroSHM
- [Single-machine thesis](docs/single_machine_thesis.md) -- Why shared memory IPC is different from distributed systems

### Language-Specific READMEs

- [C++ implementation](cpp/README.md)
- [Go implementation](go/README.md)
- [Rust implementation](rust/README.md)

## Links

- [Source code](https://github.com/aguasonic/zeroshm)



