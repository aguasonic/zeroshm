#!/bin/sh
# Check for non-ASCII characters in source code files.
# Usage: ./scripts/check_ascii.sh [root_dir]
#
# Exits with code 1 if non-ASCII characters are found in source code files.
# Exits with code 0 if all source code files are pure ASCII.
#
# Documentation files (.md) are NOT checked - they may contain author-placed
# unicode (e.g., emoji).

set -eu

ROOT="${1:-.}"

FOUNDFILE=$(mktemp)
echo "0" > "$FOUNDFILE"

# Find all source code files and check for non-ASCII characters
find "$ROOT" \
    \( -name '.git' -o -name '.junie' -o -name 'target' -o -name 'build' -o -name 'node_modules' \) -prune -o \
    \( -name '*.cpp' -o -name '*.h' -o -name '*.hpp' -o -name '*.c' -o -name '*.go' -o -name '*.rs' -o -name '*.toml' -o -name '*.pkl' -o -name '*.txt' \) \
    -type f -exec sh -c '
        foundfile="$1"
        shift
        for file do
            if LC_ALL=C grep -q "[^[:cntrl:][:print:]]" "$file" 2>/dev/null; then
                echo "ERROR: Non-ASCII characters found in: $file"
                echo "1" > "$foundfile"
            fi
        done
    ' sh "$FOUNDFILE" {} +

found=$(cat "$FOUNDFILE")
rm -f "$FOUNDFILE"

if [ "$found" -eq 1 ]; then
    exit 1
fi

echo "OK: All source code files are pure ASCII"
exit 0
