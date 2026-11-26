#!/bin/sh
# ASCII normalization script for ZeroSHM project.
# Usage: ./scripts/normalize_ascii.sh [root_dir]
#
# Normalizes source code files to pure ASCII. Documentation files get
# typographical unicode replaced (em dashes, curly quotes) but retain
# author-placed expressive unicode (e.g., emoji).

set -eu

ROOT="${1:-.}"

# Source code extensions (strict ASCII required)
SOURCE_EXTS="cpp h hpp c go rs toml pkl txt"

MODFILE=$(mktemp)
echo "0" > "$MODFILE"

# Find all text files and normalize them
find "$ROOT" \
    \( -name '.git' -o -name '.junie' -o -name 'target' -o -name 'build' -o -name 'node_modules' \) -prune -o \
    -type f -print | while IFS= read -r file; do
    # Determine if this is a source code file
    is_source=0
    ext="${file##*.}"
    case " $SOURCE_EXTS " in
        *" $ext "*) is_source=1 ;;
    esac

    # Apply typographical replacements using perl (handles UTF-8 reliably)
    perl -i -pe '
        s/\x{2014}/--/g;   # Em dash -> double hyphen
        s/\x{2013}/-/g;    # En dash -> hyphen
        s/\x{201C}/"/g;    # Left double quote -> straight quote
        s/\x{201D}/"/g;    # Right double quote -> straight quote
        s/\x{2018}/'"'"'/g;    # Left single quote -> straight quote
        s/\x{2019}/'"'"'/g;    # Right single quote -> straight quote
        s/\x{2026}/.../g;  # Ellipsis -> three periods
        s/\x{2192}/->/g;   # Right arrow -> ->
    ' "$file"

    # For source code files, also strip any remaining non-ASCII characters
    if [ "$is_source" -eq 1 ]; then
        # Remove any remaining non-ASCII bytes
        if LC_ALL=C grep -q '[^[:cntrl:][:print:]]' "$file" 2>/dev/null; then
            LC_ALL=C tr -d '\128-\377' < "$file" > "${file}.tmp" && mv "${file}.tmp" "$file"
        fi
    fi

    # Check if file was modified (compare with git)
    if ! git diff --quiet "$file" 2>/dev/null; then
        modified=$(cat "$MODFILE")
        modified=$((modified + 1))
        echo "$modified" > "$MODFILE"
        echo "Modified: $file"
    fi
done

modified=$(cat "$MODFILE")
rm -f "$MODFILE"

echo ""
echo "Total files modified: $modified"
