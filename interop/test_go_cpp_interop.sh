#!/bin/sh
# Run Go <-> C++ interop tests
# NOTE: This test requires Linux (uses -lrt). Skip on other platforms.

OS=$(uname -s)
if [ "$OS" != "Linux" ]; then
    echo "SKIP: Go <-> C++ interop test requires Linux (uses -lrt). Current platform: $OS"
    exit 0
fi

cd "$(dirname "$0")/../go"
go run ./cmd/interop
