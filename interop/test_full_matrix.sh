#!/bin/sh
# Cross-language interop matrix test for ZeroIPC v3 (xxhash3-64)
# Tests all 3 language pairs with existing implementations:
#   C++ <-> Go, C++ <-> Rust,
#   Go <-> Rust
#
# Note: C and Python implementations are not part of this project,
# so their interop tests are not included.
set -e

SCRIPT_DIR="$(dirname "$0")"

echo "=============================================="
echo " ZeroIPC Cross-Language Interop Matrix"
echo " v3 format: xxhash3-64 + 255-byte names"
echo " Languages: C++, Go, Rust"
echo "=============================================="

FAILED=0

run_test() {
    script=$1
    name=$2
    echo ""
    echo "----------------------------------------------"
    echo " Running: $name"
    echo "----------------------------------------------"
    if sh "$SCRIPT_DIR/$script"; then
        echo " PASSED: $name"
    else
        echo " FAILED: $name"
        FAILED=1
    fi
}

# Run all 3 interop tests (only languages with existing implementations)
run_test "test_cpp_go_interop.sh" "C++ <-> Go"
run_test "test_rust_cpp_interop.sh" "C++ <-> Rust"
run_test "test_rust_go_interop.sh" "Go <-> Rust"

echo ""
echo "=============================================="
if [ $FAILED -eq 0 ]; then
    echo " ALL 3 INTEROP TESTS PASSED"
else
    echo " SOME TESTS FAILED"
    exit 1
fi
echo "=============================================="
