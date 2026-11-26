#!/bin/sh
# Test C++ <-> Go interop with long names and hash verification
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

SHM_NAME="/test_cpp_go_interop"

echo "=== C++ <-> Go Interop Test ==="
echo "Testing long names and hash-first lookup across C++ and Go implementations"

# Clean up any existing shared memory
rm -f /dev/shm$SHM_NAME 2>/dev/null || true
if command -v shm_unlink >/dev/null 2>&1; then
    :
fi

# Step 1: Build Go and C++ test programs
echo ""
echo "--- Step 1: Building test programs ---"

GO_TEST="$PROJECT_ROOT/go/bin/test_go_interop"
CPP_TEST="$PROJECT_ROOT/cpp/build/test_c_interop"

# Build Go test program
if [ ! -f "$GO_TEST" ]; then
    echo "Building Go test program..."
    (cd "$PROJECT_ROOT/go" && mkdir -p bin && go build -o bin/test_go_interop ./cmd/test_go_interop)
else
    echo "Go test program already built: $GO_TEST"
fi

# Build C++ test program
if [ ! -f "$CPP_TEST" ]; then
    echo "Building C++ test program..."
    if [ ! -d "$PROJECT_ROOT/cpp/build" ]; then
        (cd "$PROJECT_ROOT/cpp" && mkdir build && cd build && cmake .. -DCMAKE_BUILD_TYPE=Release > /dev/null 2>&1)
    else
        (cd "$PROJECT_ROOT/cpp/build" && cmake .. > /dev/null 2>&1)
    fi
    (cd "$PROJECT_ROOT/cpp/build" && make test_c_interop > /dev/null 2>&1)
else
    echo "C++ test program already built: $CPP_TEST"
fi

if [ ! -f "$GO_TEST" ] || [ ! -f "$CPP_TEST" ]; then
    echo "ERROR: Failed to build test programs"
    exit 1
fi

# Step 2: C++ writes, Go reads
echo ""
echo "--- Step 2: C++ writes, Go reads ---"
"$CPP_TEST" write "$SHM_NAME"
"$GO_TEST" read "$SHM_NAME"

# Clean up for next test
rm -f /dev/shm$SHM_NAME 2>/dev/null || true

# Step 3: Go writes, C++ reads
echo ""
echo "--- Step 3: Go writes, C++ reads ---"
"$GO_TEST" write "$SHM_NAME"
"$CPP_TEST" read "$SHM_NAME"

# Clean up
rm -f /dev/shm$SHM_NAME 2>/dev/null || true

echo ""
echo "=== C++ <-> Go Interop Test PASSED ==="
