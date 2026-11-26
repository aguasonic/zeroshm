# ZeroSHM Examples

This directory contains example programs demonstrating how to use ZeroSHM in each supported language: C++, Go, and Rust.

Each example implements a simple **producer-consumer** pattern that demonstrates:

- Creating and opening shared memory segments
- Using typed arrays for data storage (zero-copy access)
- Using lock-free MPMC queues for inter-process communication
- Proper cleanup of shared resources

## Directory Structure

```
examples/
├── cpp/        # C++23 example (header-only library)
├── go/         # Go example
└── rust/       # Rust example
```

## Building and Running

### C++

```bash
cd examples/cpp
mkdir build && cd build
cmake ..
make
./producer   # Run in one terminal
./consumer   # Run in another terminal
```

### Go

```bash
cd examples/go
go run ./producer    # Run in one terminal
go run ./consumer    # Run in another terminal
```

### Rust

```bash
cd examples/rust
cargo run --bin producer   # Run in one terminal
cargo run --bin consumer   # Run in another terminal
```

## Cross-Language Interop

Since all three implementations use the same binary format, you can mix languages:

- Start the Go producer and the Rust consumer
- Start the C++ producer and the Go consumer

Just make sure both processes use the same shared memory name.
