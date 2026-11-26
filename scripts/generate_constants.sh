#!/bin/sh
# Generate language-specific constant files from constants.pkl
# Usage: ./scripts/generate_constants.sh
#
# This script:
# 1. Evaluates constants.pkl to JSON using Pkl
# 2. Generates C++ header, Go file, and Rust file from the JSON values
#
# Dependencies: pkl, jq

set -e

# Get the project root (parent of scripts directory)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

PKL_FILE="$ROOT_DIR/constants.pkl"

echo "Generating constants from $PKL_FILE"

# Evaluate Pkl to JSON
JSON=$(pkl eval "$PKL_FILE" --format json)

# Helper: convert camelCase to SCREAMING_SNAKE_CASE
# e.g., sectionAlign -> SECTION_ALIGN, queueHeadOffset -> QUEUE_HEAD_OFFSET
# Handles: consecutive uppercase (xxHashSeed -> XXHASH_SEED), digits (u32Size -> U32_SIZE)
camel_to_screaming() {
  key="$1"
  # Special cases for backward compatibility with existing constant names
  case "$key" in
    magic) echo "TABLE_MAGIC"; return ;;
    version) echo "TABLE_VERSION"; return ;;
    xxHashSeed) echo "XXHASH_SEED"; return ;;
  esac
  echo "$key" | awk '{
    result = ""
    for (i = 1; i <= length($0); i++) {
      c = substr($0, i, 1)
      prev = (i > 1) ? substr($0, i-1, 1) : ""
      if (c ~ /[A-Z]/ && i > 1 && (prev ~ /[a-z]/ || prev ~ /[0-9]/)) {
        result = result "_" toupper(c)
      } else {
        result = result toupper(c)
      }
    }
    print result
  }'
}

# Helper: convert camelCase to PascalCase
# e.g., sectionAlign -> SectionAlign, queueHeadOffset -> QueueHeadOffset
camel_to_pascal() {
  key="$1"
  # Special cases for backward compatibility with existing constant names
  case "$key" in
    magic) echo "TableMagic"; return ;;
    version) echo "TableVersion"; return ;;
    xxHashSeed) echo "XXHashSeed"; return ;;
  esac
  echo "$key" | awk '{
    result = ""
    first = 1
    for (i = 1; i <= length($0); i++) {
      c = substr($0, i, 1)
      if (c ~ /[A-Z]/ && i > 1) {
        result = result c
      } else if (first) {
        result = result toupper(c)
        first = 0
      } else {
        result = result c
      }
    }
    print result
  }'
}

# Helper: determine the C++ type for a constant based on its key name
cpp_type() {
  case "$1" in
    magic|version) echo "uint32_t" ;;
    xxHashSeed) echo "uint64_t" ;;
    *) echo "std::size_t" ;;
  esac
}

# Helper: determine the Go type for a constant based on its key name
go_type() {
  case "$1" in
    magic|version) echo "uint32" ;;
    xxHashSeed) echo "uint64" ;;
    *) echo "" ;;
  esac
}

# Helper: determine the Rust type for a constant based on its key name
rust_type() {
  case "$1" in
    magic|version) echo "u32" ;;
    xxHashSeed) echo "u64" ;;
    *) echo "usize" ;;
  esac
}

# Helper: format value (hex for magic/seed, decimal for others)
format_value() {
  key="$1"
  value="$2"
  case "$key" in
    magic)
      printf "0x%08X" "$value"
      ;;
    xxHashSeed)
      printf "0x%X" "$value"
      ;;
    *)
      echo "$value"
      ;;
  esac
}

# Helper: get description for a constant
description() {
  case "$1" in
    magic) echo "Magic number identifying a ZeroSHM table header ('ZSHM')" ;;
    version) echo "Current table format version" ;;
    xxHashSeed) echo "Seed value for xxhash3-64 name hashing" ;;
    headerSize) echo "Size of the table header in bytes" ;;
    entrySize) echo "Size of each table entry in bytes" ;;
    sectionAlign) echo "Section alignment (8-byte boundaries for all structure sections)" ;;
    nameFieldSize) echo "Name field size in table entries (includes null terminator + padding)" ;;
    u32Size) echo "Size of u32 primitive" ;;
    u64Size) echo "Size of u64 primitive" ;;
    tableMagicOffset) echo "Table header: magic field offset" ;;
    tableVersionOffset) echo "Table header: version field offset" ;;
    tableEntryCountOffset) echo "Table header: entry count field offset" ;;
    tableMaxEntriesOffset) echo "Table header: max entries field offset" ;;
    tableMemorySizeOffset) echo "Table header: memory size field offset" ;;
    tableNextOffsetOffset) echo "Table header: next offset field" ;;
    queueHeadOffset) echo "Queue header: head field offset" ;;
    queueTailOffset) echo "Queue header: tail field offset" ;;
    queueCapacityOffset) echo "Queue header: capacity field offset" ;;
    queueElemSizeOffset) echo "Queue header: element size field offset" ;;
    ringWritePosOffset) echo "Ring header: write position field offset" ;;
    ringReadPosOffset) echo "Ring header: read position field offset" ;;
    ringCapacityOffset) echo "Ring header: capacity field offset" ;;
    ringElemSizeOffset) echo "Ring header: element size field offset" ;;
    stackTopOffset) echo "Stack header: top field offset" ;;
    stackCapacityOffset) echo "Stack header: capacity field offset" ;;
    stackElemSizeOffset) echo "Stack header: element size field offset" ;;
    stackReservedOffset) echo "Stack header: reserved field offset" ;;
    mapSizeOffset) echo "Map header: size field offset" ;;
    mapCapacityOffset) echo "Map header: capacity field offset" ;;
    mapKeySizeOffset) echo "Map header: key size field offset" ;;
    mapValueSizeOffset) echo "Map header: value size field offset" ;;
    setSizeOffset) echo "Set header: size field offset" ;;
    setCapacityOffset) echo "Set header: capacity field offset" ;;
    setElemSizeOffset) echo "Set header: element size field offset" ;;
    setReservedOffset) echo "Set header: reserved field offset" ;;
    poolFreeHeadOffset) echo "Pool header: free head field offset" ;;
    poolAllocatedOffset) echo "Pool header: allocated field offset" ;;
    poolPaddingOffset) echo "Pool header: padding field offset" ;;
    poolCapacityOffset) echo "Pool header: capacity field offset" ;;
    poolElemSizeOffset) echo "Pool header: element size field offset" ;;
    barrierArrivedOffset) echo "Barrier header: arrived field offset" ;;
    barrierGenerationOffset) echo "Barrier header: generation field offset" ;;
    barrierNumParticipantsOffset) echo "Barrier header: num participants field offset" ;;
    barrierPaddingOffset) echo "Barrier header: padding field offset" ;;
    semaphoreCountOffset) echo "Semaphore header: count field offset" ;;
    semaphoreWaitingOffset) echo "Semaphore header: waiting field offset" ;;
    semaphoreMaxCountOffset) echo "Semaphore header: max count field offset" ;;
    semaphorePaddingOffset) echo "Semaphore header: padding field offset" ;;
    latchCountOffset) echo "Latch header: count field offset" ;;
    latchInitialCountOffset) echo "Latch header: initial count field offset" ;;
    latchPadding0Offset) echo "Latch header: padding 0 field offset" ;;
    latchPadding1Offset) echo "Latch header: padding 1 field offset" ;;
    eventSignaledOffset) echo "Event state: signaled field offset" ;;
    eventModeOffset) echo "Event state: mode field offset" ;;
    eventWaitingOffset) echo "Event state: waiting field offset" ;;
    onceStateOffset) echo "Once flag: state field offset" ;;
    rwlockReadersOffset) echo "RWLock state: readers field offset" ;;
    rwlockWriterActiveOffset) echo "RWLock state: writer active field offset" ;;
    channelCapacityOffset) echo "Channel header: capacity field offset" ;;
    channelSendersOffset) echo "Channel header: senders field offset" ;;
    channelReceiversOffset) echo "Channel header: receivers field offset" ;;
    channelClosedOffset) echo "Channel header: closed field offset" ;;
    channelSendSeqOffset) echo "Channel header: send sequence field offset" ;;
    channelRecvSeqOffset) echo "Channel header: receive sequence field offset" ;;
    streamSequenceOffset) echo "Stream header: sequence field offset" ;;
    streamSubscribersOffset) echo "Stream header: subscribers field offset" ;;
    streamClosedOffset) echo "Stream header: closed field offset" ;;
    streamBufferCapacityOffset) echo "Stream header: buffer capacity field offset" ;;
    streamPaddingOffset) echo "Stream header: padding field offset" ;;
    streamTransformNameOffset) echo "Stream header: transform name field offset" ;;
    futureStateOffset) echo "Future header: state field offset" ;;
    futureWaitersOffset) echo "Future header: waiters field offset" ;;
    futureCompletionTimeOffset) echo "Future header: completion time field offset" ;;
    lazyStateOffset) echo "Lazy header: state field offset" ;;
    lazyComputeCountOffset) echo "Lazy header: compute count field offset" ;;
    lazyCompletionTimeOffset) echo "Lazy header: completion time field offset" ;;
    *) echo "$1" ;;
  esac
}

# Helper: get group category for a constant
group_category() {
  case "$1" in
    magic|version|xxHashSeed|headerSize|entrySize) echo "Table Format" ;;
    sectionAlign|nameFieldSize|u32Size|u64Size) echo "Core" ;;
    table*) echo "Table Header" ;;
    queue*) echo "Queue Header" ;;
    ring*) echo "Ring Header" ;;
    stack*) echo "Stack Header" ;;
    map*) echo "Map Header" ;;
    set*) echo "Set Header" ;;
    pool*) echo "Pool Header" ;;
    barrier*) echo "Barrier Header" ;;
    semaphore*) echo "Semaphore Header" ;;
    latch*) echo "Latch Header" ;;
    event*) echo "Event State" ;;
    once*) echo "Once Flag" ;;
    rwlock*) echo "RWLock State" ;;
    channel*) echo "Channel Header" ;;
    stream*) echo "Stream Header" ;;
    future*) echo "Future Header" ;;
    lazy*) echo "Lazy Header" ;;
    *) echo "Other" ;;
  esac
}

# Generate C++ header
CPP_FILE="$ROOT_DIR/cpp/include/zeroshm/constants_generated.h"
{
  echo "#pragma once"
  echo "// Auto-generated from constants.pkl - DO NOT EDIT"
  echo "// Run 'make generate' to regenerate."
  echo ""
  echo "#include <cstdint>"
  echo "#include <cstddef>"
  echo ""
  echo "namespace zeroshm {"
  echo ""

  # Track current group for section headers
  prev_group=""

  # Get all keys sorted
  echo "$JSON" | jq -r 'keys[]' | sort | while read -r key; do
    value=$(echo "$JSON" | jq -r --arg key "$key" '.[$key]')
    formatted_value=$(format_value "$key" "$value")
    screaming=$(camel_to_screaming "$key")
    type=$(cpp_type "$key")
    desc=$(description "$key")
    group=$(group_category "$key")

    # Add group separator if changed
    if [ "$group" != "$prev_group" ] && [ -n "$prev_group" ]; then
      echo ""
    fi
    prev_group="$group"

    echo "/// $desc"
    echo "constexpr $type $screaming = $formatted_value;"
  done

  echo ""
  echo "} // namespace zeroshm"
} > "$CPP_FILE"
echo "Generated $CPP_FILE"

# Generate Go file
GO_FILE="$ROOT_DIR/go/zeroshm/constants_generated.go"
{
  echo "package zeroshm"
  echo ""
  echo "// Code generated from constants.pkl - DO NOT EDIT"
  echo "// Run 'make generate' to regenerate."
  echo ""
  echo "const ("

  # Get all keys sorted
  echo "$JSON" | jq -r 'keys[]' | sort | while read -r key; do
    value=$(echo "$JSON" | jq -r --arg key "$key" '.[$key]')
    formatted_value=$(format_value "$key" "$value")
    pascal=$(camel_to_pascal "$key")
    type=$(go_type "$key")
    desc=$(description "$key")

    if [ -n "$type" ]; then
      echo "\t// $desc"
      echo "\t$pascal $type = $formatted_value"
    else
      echo "\t// $desc"
      echo "\t$pascal = $formatted_value"
    fi
  done

  echo ")"
} > "$GO_FILE"
echo "Generated $GO_FILE"

# Generate Rust file
RUST_FILE="$ROOT_DIR/rust/zeroshm/src/constants_generated.rs"
{
  echo "// Auto-generated from constants.pkl - DO NOT EDIT"
  echo "// Run 'make generate' to regenerate."
  echo ""

  # Get all keys sorted
  echo "$JSON" | jq -r 'keys[]' | sort | while read -r key; do
    value=$(echo "$JSON" | jq -r --arg key "$key" '.[$key]')
    formatted_value=$(format_value "$key" "$value")
    screaming=$(camel_to_screaming "$key")
    type=$(rust_type "$key")
    desc=$(description "$key")

    echo "/// $desc"
    echo "pub const $screaming: $type = $formatted_value;"
    echo ""
  done
} > "$RUST_FILE"
echo "Generated $RUST_FILE"

echo "=== Constant generation complete ==="

