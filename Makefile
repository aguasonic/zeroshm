# ZeroSHM - Top-level Makefile
# Builds all language implementations: C++, Go, Rust
#
# Usage:
#   make all          - Build everything
#   make cpp          - Build C++ implementation
#   make go           - Build Go implementation
#   make rust         - Build Rust implementation
#   make test         - Run all tests
#   make test-cpp     - Run C++ tests
#   make test-go      - Run Go tests
#   make test-rust    - Run Rust tests
#   make benchmarks   - Run all benchmarks
#   make bench-cpp    - Run C++ benchmarks
#   make bench-go     - Run Go benchmarks
#   make bench-rust   - Run Rust benchmarks
#   make interop      - Run cross-language interop tests
#   make cli          - Build Go CLI tool
#   make clean        - Clean all build artifacts
#   make help         - Show this help

.PHONY: all help generate cpp cpp-test go go-test rust rust-test interop cli clean docs test test-cpp test-go test-rust check-ascii benchmarks bench-cpp bench-go bench-rust verify-constants

# Generate language-specific constants from constants.pkl, then verify consistency
generate:
	@echo "=== Generating constants from constants.pkl ==="
	./scripts/generate_constants.sh
	$(MAKE) verify-constants

# Verify generated constants have identical values across all languages
verify-constants:
	@echo "=== Verifying constant consistency across languages ==="
	cd go && go run ./cmd/verify_constants

# Default target
all: generate cpp go rust
	@echo "=== ZeroSHM Build Complete ==="

help:
	@echo "ZeroSHM Makefile Targets:"
	@echo ""
	@echo "  all          - Build everything (generates constants, then C++, Go, Rust)"
	@echo "  generate     - Generate language-specific constants from constants.pkl, then verify consistency"
	@echo "  verify-constants - Verify constant consistency across all languages"
	@echo "  cpp          - Build C++ implementation (header-only, builds examples/tests)"
	@echo "  go           - Build Go implementation"
	@echo "  rust         - Build Rust implementation"
	@echo "  cli          - Build Go CLI tool"
	@echo "  test         - Run all tests"
	@echo "  test-cpp     - Run C++ tests"
	@echo "  test-go      - Run Go tests"
	@echo "  test-rust    - Run Rust tests"
	@echo "  benchmarks   - Run all benchmarks (C++, Go, Rust)"
	@echo "  bench-cpp    - Run C++ benchmarks"
	@echo "  bench-go     - Run Go benchmarks"
	@echo "  bench-rust   - Run Rust benchmarks"
	@echo "  interop      - Run cross-language interop tests"
	@echo "  clean        - Clean all build artifacts"

# --- C++ Implementation ---
CPP_BUILD_DIR := build/cpp

cpp:
	@echo "=== Building C++ implementation ==="
	cmake -B $(CPP_BUILD_DIR) cpp
	cmake --build $(CPP_BUILD_DIR)

cpp-test: cpp
	@echo "=== Running C++ tests ==="
	ctest --test-dir $(CPP_BUILD_DIR) --output-on-failure

# --- Go Implementation ---
go:
	@echo "=== Building Go implementation ==="
	cd go && go build ./...

go-test:
	@echo "=== Running Go tests ==="
	cd go && go test ./zeroshm/...

go-cli: go
	@echo "=== Building Go CLI tool ==="
	cd go && go build -o zeroshm ./cmd/zeroshm

cli: go-cli

# --- Rust Implementation ---
rust:
	@echo "=== Building Rust implementation ==="
	cd rust && cargo build --release

rust-test:
	@echo "=== Running Rust tests ==="
	cd rust && cargo test

# --- Interop Tests ---
interop: cpp go rust
	@echo "=== Running cross-language interop tests ==="
	cd interop && ./test_full_matrix.sh

# --- ASCII Check (CI) ---
check-ascii:
	@echo "=== Checking for non-ASCII characters in source code ==="
	./scripts/check_ascii.sh

# --- Benchmarks ---
bench-cpp: cpp
	@echo "=== Running C++ benchmarks ==="
	cd $(CPP_BUILD_DIR)/benchmarks && ./benchmark_queue && ./benchmark_stack && ./benchmark_array

bench-go:
	@echo "=== Running Go benchmarks ==="
	cd go && go test -bench=. -benchtime=1s -run='^$$' ./zeroshm/...

bench-rust: rust
	@echo "=== Running Rust benchmarks ==="
	cd rust && cargo bench --bench benchmark_queue && cargo bench --bench benchmark_stack && cargo bench --bench benchmark_array

benchmarks: bench-cpp bench-go bench-rust
	@echo "=== All Benchmarks Complete ==="

# --- Tests ---
test: cpp-test go-test rust-test
	@echo "=== All Tests Complete ==="

# --- Clean ---
clean:
	@echo "=== Cleaning all build artifacts ==="
	rm -rf build
	rm -rf cpp/build
	cd go && go clean ./...
	cd rust && cargo clean
	rm -f zeroshm
	@echo "=== Clean Complete ==="

# --- Documentation (optional) ---
docs:
	@echo "=== Building documentation ==="
	mkdocs build || echo "Warning: mkdocs not available, skipping docs build"

