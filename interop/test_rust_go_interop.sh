#!/bin/sh
# Rust <-> Go interop tests for ZeroIPC
# Tests: Array, Queue, Mutex, Barrier (Go doesn't have Signal)
set -e

# Resolve script directory to absolute path
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
RUST_DIR="$ROOT_DIR/rust"
GO_DIR="$ROOT_DIR/go"

echo "=============================================="
echo " Rust <-> Go Interop Tests"
echo "=============================================="

FAILED=0

# Build Rust interop binary
echo ""
echo "Building Rust interop binary..."
cd "$RUST_DIR"
cargo build -p interop-tests --bin interop 2>&1 | grep -v "^warning" || true
RUST_BIN="$RUST_DIR/target/debug/interop"

# Build Go interop binary
echo ""
echo "Building Go interop binary..."
cd "$GO_DIR"
GO_BIN=$(go build -o /tmp/zeroipc_go_interop ./cmd/test_rust_go_interop && echo "/tmp/zeroipc_go_interop")

run_rust() {
    "$RUST_BIN" "$@"
}

run_go() {
    "$GO_BIN" "$@"
}

test_array_rust_to_go() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Array - Rust creates, Go reads"
    echo "----------------------------------------------"
    
    SHM="/rust_go_array_test"
    run_rust array_create "$SHM" temperatures 5 10.5 20.5 30.5 40.5 50.5
    
    # Go reads and verifies
    if run_go array_read "$SHM" temperatures 2>&1 | grep -q "Go: array\[0\] = 10.5"; then
        echo " PASSED: Array Rust->Go"
    else
        echo " FAILED: Array Rust->Go"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_array_go_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Array - Go creates, Rust reads"
    echo "----------------------------------------------"
    
    SHM="/rust_go_array_test2"
    run_go array_create "$SHM" temperatures 5 100.1 200.2 300.3 400.4 500.5
    
    # Rust reads and verifies
    if run_rust array_read "$SHM" temperatures 2>&1 | grep -q "Rust: array\[0\] = 100.1"; then
        echo " PASSED: Array Go->Rust"
    else
        echo " FAILED: Array Go->Rust"
        FAILED=1
    fi
    
    run_go cleanup "$SHM" 2>/dev/null || true
}

test_queue_rust_to_go() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Queue - Rust pushes, Go pops"
    echo "----------------------------------------------"
    
    SHM="/rust_go_queue_test"
    run_rust queue_create "$SHM" events 16
    run_rust queue_push "$SHM" events 100 200 300 400 500
    
    # Go pops and verifies
    if run_go queue_pop "$SHM" events 5 2>&1 | grep -q "Go: Popped 100"; then
        echo " PASSED: Queue Rust->Go"
    else
        echo " FAILED: Queue Rust->Go"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_queue_go_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Queue - Go pushes, Rust pops"
    echo "----------------------------------------------"
    
    SHM="/rust_go_queue_test2"
    run_go queue_create "$SHM" events 16
    run_go queue_push "$SHM" events 1000 2000 3000
    
    # Rust pops and verifies
    if run_rust queue_pop "$SHM" events 3 2>&1 | grep -q "Rust: Popped 1000"; then
        echo " PASSED: Queue Go->Rust"
    else
        echo " FAILED: Queue Go->Rust"
        FAILED=1
    fi
    
    run_go cleanup "$SHM" 2>/dev/null || true
}

test_mutex_rust_to_go() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Mutex - Rust locks/unlocks, Go acquires"
    echo "----------------------------------------------"
    
    SHM="/rust_go_mutex_test"
    run_rust mutex_create "$SHM" data_mutex
    
    # Rust locks then unlocks
    run_rust mutex_lock "$SHM" data_mutex
    
    # Go acquires (verifies Rust's unlock worked)
    if run_go mutex_unlock "$SHM" data_mutex 2>&1 | grep -q "Go: Acquired mutex"; then
        echo " PASSED: Mutex Rust->Go"
    else
        echo " FAILED: Mutex Rust->Go"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_mutex_go_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Mutex - Go locks/unlocks, Rust acquires"
    echo "----------------------------------------------"
    
    SHM="/rust_go_mutex_test2"
    run_go mutex_create "$SHM" data_mutex
    
    # Go locks then unlocks
    run_go mutex_lock "$SHM" data_mutex
    
    # Rust acquires (verifies Go's unlock worked)
    if run_rust mutex_unlock "$SHM" data_mutex 2>&1 | grep -q "Rust: Acquired mutex"; then
        echo " PASSED: Mutex Go->Rust"
    else
        echo " FAILED: Mutex Go->Rust"
        FAILED=1
    fi
    
    run_go cleanup "$SHM" 2>/dev/null || true
}

test_barrier_rust_go() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Barrier - Rust + Go sync"
    echo "----------------------------------------------"
    
    SHM="/rust_go_barrier_test"
    run_rust barrier_create "$SHM" sync_barrier 2
    
    # Start Go waiting at barrier in background
    run_go barrier_wait "$SHM" sync_barrier > /tmp/go_barrier_out.txt 2>&1 &
    GO_PID=$!
    
    # Wait a moment for Go to start waiting
    sleep 0.5
    
    # Rust waits at barrier (should release both)
    if run_rust barrier_wait "$SHM" sync_barrier 2>&1 | grep -q "Rust: Barrier.*released"; then
        echo " PASSED: Barrier Rust+Go"
    else
        echo " FAILED: Barrier Rust+Go"
        FAILED=1
    fi
    
    # Wait for Go to finish
    wait $GO_PID 2>/dev/null || true
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

# Run all tests
test_array_rust_to_go
test_array_go_to_rust
test_queue_rust_to_go
test_queue_go_to_rust
test_mutex_rust_to_go
test_mutex_go_to_rust
test_barrier_rust_go

test_latest_rust_to_go() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Latest - Rust creates, Go waits (blocking)"
    echo "----------------------------------------------"
    
    SHM="/rust_go_latest_test"
    run_rust latest_create "$SHM" counter 100
    
    # Start Go waiting in background
    run_go latest_wait "$SHM" counter 1 5000 > /tmp/go_latest_wait_out.txt 2>&1 &
    GO_PID=$!
    
    # Wait a moment for Go to start waiting
    sleep 0.5
    
    # Rust sets a new value
    run_rust latest_set "$SHM" counter 200
    
    # Wait for Go to finish
    wait $GO_PID 2>/dev/null || true
    
    if grep -q "Go: Latest '.*counter.*' changed: value = 200" /tmp/go_latest_wait_out.txt; then
        echo " PASSED: Latest Rust->Go blocking"
    else
        echo " FAILED: Latest Rust->Go blocking"
        cat /tmp/go_latest_wait_out.txt
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
    rm -f /tmp/go_latest_wait_out.txt
}

test_latest_go_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Latest - Go creates, Rust waits (blocking)"
    echo "----------------------------------------------"
    
    SHM="/rust_go_latest_test2"
    run_go latest_create "$SHM" counter 300
    
    # Start Rust waiting in background
    run_rust latest_wait "$SHM" counter 1 5000 > /tmp/rust_latest_wait_out.txt 2>&1 &
    RUST_PID=$!
    
    # Wait a moment for Rust to start waiting
    sleep 0.5
    
    # Go sets a new value
    run_go latest_set "$SHM" counter 400
    
    # Wait for Rust to finish
    wait $RUST_PID 2>/dev/null || true
    
    if grep -q "Rust: Latest '.*counter.*' changed: value = 400" /tmp/rust_latest_wait_out.txt; then
        echo " PASSED: Latest Go->Rust blocking"
    else
        echo " FAILED: Latest Go->Rust blocking"
        cat /tmp/rust_latest_wait_out.txt
        FAILED=1
    fi
    
    run_go cleanup "$SHM" 2>/dev/null || true
    rm -f /tmp/rust_latest_wait_out.txt
}

test_latest_rust_to_go
test_latest_go_to_rust

echo ""
echo "=============================================="
if [ $FAILED -eq 0 ]; then
    echo " ALL Rust <-> Go INTEROP TESTS PASSED"
else
    echo " SOME Rust <-> Go INTEROP TESTS FAILED"
    exit 1
fi
echo "=============================================="

# Cleanup
rm -f /tmp/zeroipc_go_interop /tmp/go_barrier_out.txt
