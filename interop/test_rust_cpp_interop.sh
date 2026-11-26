#!/bin/sh
# Rust <-> C++ interop tests for ZeroIPC
# Tests: Array, Queue, Mutex, Barrier, Signal
set -e

# Resolve script directory to absolute path
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
RUST_DIR="$ROOT_DIR/rust"
CPP_DIR="$ROOT_DIR/cpp"

echo "=============================================="
echo " Rust <-> C++ Interop Tests"
echo "=============================================="

FAILED=0

# Build Rust interop binary
echo ""
echo "Building Rust interop binary..."
cd "$RUST_DIR"
cargo build -p interop-tests --bin interop 2>&1 | grep -v "^warning" || true
RUST_BIN="$RUST_DIR/target/debug/interop"

# Build C++ interop binary
echo ""
echo "Building C++ interop binary..."
CPP_BIN="/tmp/zeroipc_cpp_interop"
g++ -std=c++23 -I"$CPP_DIR/include" "$SCRIPT_DIR/cpp_rust_interop.cpp" "$CPP_DIR/include/zeroshm/detail/xxhash.c" -o "$CPP_BIN" -pthread

run_rust() {
    "$RUST_BIN" "$@"
}

run_cpp() {
    "$CPP_BIN" "$@"
}

test_array_rust_to_cpp() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Array - Rust creates, C++ reads"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_array_test"
    run_rust array_create "$SHM" temperatures 5 10.5 20.5 30.5 40.5 50.5
    
    # C++ reads and verifies
    if run_cpp array_read "$SHM" temperatures 2>&1 | grep -q "C++: array\[0\] = 10.5"; then
        echo " PASSED: Array Rust->C++"
    else
        echo " FAILED: Array Rust->C++"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_array_cpp_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Array - C++ creates, Rust reads"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_array_test2"
    run_cpp array_create "$SHM" temperatures 5 100.1 200.2 300.3 400.4 500.5
    
    # Rust reads and verifies
    if run_rust array_read "$SHM" temperatures 2>&1 | grep -q "Rust: array\[0\] = 100.1"; then
        echo " PASSED: Array C++->Rust"
    else
        echo " FAILED: Array C++->Rust"
        FAILED=1
    fi
    
    run_cpp cleanup "$SHM" 2>/dev/null || true
}

test_queue_rust_to_cpp() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Queue - Rust pushes, C++ pops"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_queue_test"
    run_rust queue_create "$SHM" events 16
    run_rust queue_push "$SHM" events 100 200 300 400 500
    
    # C++ pops and verifies
    if run_cpp queue_pop "$SHM" events 5 2>&1 | grep -q "C++: Popped 100"; then
        echo " PASSED: Queue Rust->C++"
    else
        echo " FAILED: Queue Rust->C++"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_queue_cpp_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Queue - C++ pushes, Rust pops"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_queue_test2"
    run_cpp queue_create "$SHM" events 16
    run_cpp queue_push "$SHM" events 1000 2000 3000
    
    # Rust pops and verifies
    if run_rust queue_pop "$SHM" events 3 2>&1 | grep -q "Rust: Popped 1000"; then
        echo " PASSED: Queue C++->Rust"
    else
        echo " FAILED: Queue C++->Rust"
        FAILED=1
    fi
    
    run_cpp cleanup "$SHM" 2>/dev/null || true
}

test_mutex_rust_to_cpp() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Mutex - Rust locks/unlocks, C++ acquires"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_mutex_test"
    run_rust mutex_create "$SHM" data_mutex
    
    # Rust locks then unlocks
    run_rust mutex_lock "$SHM" data_mutex
    
    # C++ acquires (verifies Rust's unlock worked)
    if run_cpp mutex_unlock "$SHM" data_mutex 2>&1 | grep -q "C++: Acquired mutex"; then
        echo " PASSED: Mutex Rust->C++"
    else
        echo " FAILED: Mutex Rust->C++"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_mutex_cpp_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Mutex - C++ locks/unlocks, Rust acquires"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_mutex_test2"
    run_cpp mutex_create "$SHM" data_mutex
    
    # C++ locks then unlocks
    run_cpp mutex_lock "$SHM" data_mutex
    
    # Rust acquires (verifies C++'s unlock worked)
    if run_rust mutex_unlock "$SHM" data_mutex 2>&1 | grep -q "Rust: Acquired mutex"; then
        echo " PASSED: Mutex C++->Rust"
    else
        echo " FAILED: Mutex C++->Rust"
        FAILED=1
    fi
    
    run_cpp cleanup "$SHM" 2>/dev/null || true
}

test_barrier_rust_cpp() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Barrier - Rust + C++ sync"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_barrier_test"
    run_rust barrier_create "$SHM" sync_barrier 2
    
    # Start C++ waiting at barrier in background
    run_cpp barrier_wait "$SHM" sync_barrier > /tmp/cpp_barrier_out.txt 2>&1 &
    CPP_PID=$!
    
    # Wait a moment for C++ to start waiting
    sleep 0.5
    
    # Rust waits at barrier (should release both)
    if run_rust barrier_wait "$SHM" sync_barrier 2>&1 | grep -q "Rust: Barrier.*released"; then
        echo " PASSED: Barrier Rust+C++"
    else
        echo " FAILED: Barrier Rust+C++"
        FAILED=1
    fi
    
    # Wait for C++ to finish
    wait $CPP_PID 2>/dev/null || true
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_signal_rust_to_cpp() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Signal - Rust sets, C++ reads"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_signal_test"
    run_rust signal_create "$SHM" counter 0
    run_rust signal_set "$SHM" counter 42
    
    # C++ reads and verifies
    if run_cpp signal_get "$SHM" counter 2>&1 | grep -q "C++: Signal.*value = 42"; then
        echo " PASSED: Signal Rust->C++"
    else
        echo " FAILED: Signal Rust->C++"
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
}

test_signal_cpp_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Signal - C++ sets, Rust reads"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_signal_test2"
    run_cpp signal_create "$SHM" counter 0
    run_cpp signal_set "$SHM" counter 99
    
    # Rust reads and verifies
    if run_rust signal_get "$SHM" counter 2>&1 | grep -q "Rust: Signal.*value = 99"; then
        echo " PASSED: Signal C++->Rust"
    else
        echo " FAILED: Signal C++->Rust"
        FAILED=1
    fi
    
    run_cpp cleanup "$SHM" 2>/dev/null || true
}

# Run all tests
test_array_rust_to_cpp
test_array_cpp_to_rust
test_queue_rust_to_cpp
test_queue_cpp_to_rust
test_mutex_rust_to_cpp
test_mutex_cpp_to_rust
test_barrier_rust_cpp
test_signal_rust_to_cpp
test_signal_cpp_to_rust

test_latest_rust_to_cpp() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Latest - Rust creates, C++ waits (blocking)"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_latest_test"
    run_rust latest_create "$SHM" counter 100
    
    # Start C++ waiting in background
    run_cpp latest_wait "$SHM" counter 1 5000 > /tmp/cpp_latest_wait_out.txt 2>&1 &
    CPP_PID=$!
    
    # Wait a moment for C++ to start waiting
    sleep 0.5
    
    # Rust sets a new value
    run_rust latest_set "$SHM" counter 200
    
    # Wait for C++ to finish
    wait $CPP_PID 2>/dev/null || true
    
    if grep -q "C++: Latest '.*counter.*' changed: value = 200" /tmp/cpp_latest_wait_out.txt; then
        echo " PASSED: Latest Rust->C++ blocking"
    else
        echo " FAILED: Latest Rust->C++ blocking"
        cat /tmp/cpp_latest_wait_out.txt
        FAILED=1
    fi
    
    run_rust cleanup "$SHM" 2>/dev/null || true
    rm -f /tmp/cpp_latest_wait_out.txt
}

test_latest_cpp_to_rust() {
    echo ""
    echo "----------------------------------------------"
    echo " Test: Latest - C++ creates, Rust waits (blocking)"
    echo "----------------------------------------------"
    
    SHM="/rust_cpp_latest_test2"
    run_cpp latest_create "$SHM" counter 300
    
    # Start Rust waiting in background
    run_rust latest_wait "$SHM" counter 1 5000 > /tmp/rust_latest_wait_out.txt 2>&1 &
    RUST_PID=$!
    
    # Wait a moment for Rust to start waiting
    sleep 0.5
    
    # C++ sets a new value
    run_cpp latest_set "$SHM" counter 400
    
    # Wait for Rust to finish
    wait $RUST_PID 2>/dev/null || true
    
    if grep -q "Rust: Latest '.*counter.*' changed: value = 400" /tmp/rust_latest_wait_out.txt; then
        echo " PASSED: Latest C++->Rust blocking"
    else
        echo " FAILED: Latest C++->Rust blocking"
        cat /tmp/rust_latest_wait_out.txt
        FAILED=1
    fi
    
    run_cpp cleanup "$SHM" 2>/dev/null || true
    rm -f /tmp/rust_latest_wait_out.txt
}

test_latest_rust_to_cpp
test_latest_cpp_to_rust

echo ""
echo "=============================================="
if [ $FAILED -eq 0 ]; then
    echo " ALL Rust <-> C++ INTEROP TESTS PASSED"
else
    echo " SOME Rust <-> C++ INTEROP TESTS FAILED"
    exit 1
fi
echo "=============================================="

# Cleanup
rm -f "$CPP_BIN" /tmp/cpp_barrier_out.txt
