// verify_constants - Verify that generated constants have identical values
// across C++, Go, and Rust.
//
// Usage: go run go/cmd/verify_constants
//
// This program:
// 1. Evaluates constants.pkl to JSON using Pkl
// 2. Extracts all constant values from each generated file
// 3. Compares them to ensure consistency
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

func main() {
	// Get the project root (assumes running from project root or walks up to find go.mod)
	rootDir, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "ERROR: Failed to find project root: %v\n", err)
		os.Exit(1)
	}

	pklFile := filepath.Join(rootDir, "constants.pkl")
	cppFile := filepath.Join(rootDir, "cpp/include/zeroshm/constants_generated.h")
	goFile := filepath.Join(rootDir, "go/zeroshm/constants_generated.go")
	rustFile := filepath.Join(rootDir, "rust/zeroshm/src/constants_generated.rs")

	fmt.Println("==============================================")
	fmt.Println(" Constant Value Verification")
	fmt.Println(" Source: constants.pkl")
	fmt.Println(" Generated: C++, Go, Rust")
	fmt.Println("==============================================")
	fmt.Println()

	// Get the JSON from Pkl
	cmd := exec.Command("pkl", "eval", pklFile, "--format", "json")
	output, err := cmd.CombinedOutput()
	if err != nil {
		fmt.Println("ERROR: Failed to evaluate constants.pkl")
		fmt.Println(string(output))
		os.Exit(1)
	}

	var pklData map[string]int64
	if err := json.Unmarshal(output, &pklData); err != nil {
		fmt.Printf("ERROR: Failed to parse pkl JSON: %v\n", err)
		os.Exit(1)
	}

	// Read generated files
	cppContent, err := os.ReadFile(cppFile)
	if err != nil {
		fmt.Printf("ERROR: Failed to read C++ file: %v\n", err)
		os.Exit(1)
	}
	goContent, err := os.ReadFile(goFile)
	if err != nil {
		fmt.Printf("ERROR: Failed to read Go file: %v\n", err)
		os.Exit(1)
	}
	rustContent, err := os.ReadFile(rustFile)
	if err != nil {
		fmt.Printf("ERROR: Failed to read Rust file: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Source (constants.pkl): %d constants\n", len(pklData))

	// Count constants in each generated file
	cppCount := len(regexp.MustCompile(`constexpr`).FindAllString(string(cppContent), -1))
	goCount := len(regexp.MustCompile(`(?m)^\s+[A-Z][a-zA-Z0-9]+`).FindAllString(string(goContent), -1))
	rustCount := len(regexp.MustCompile(`pub const`).FindAllString(string(rustContent), -1))

	fmt.Printf("C++: %d constants\n", cppCount)
	fmt.Printf("Go: %d constants\n", goCount)
	fmt.Printf("Rust: %d constants\n", rustCount)
	fmt.Println()

	// Verify counts match
	if !(len(pklData) == cppCount && cppCount == goCount && goCount == rustCount) {
		fmt.Println("ERROR: Constant count mismatch!")
		fmt.Printf("  Pkl=%d, C++=%d, Go=%d, Rust=%d\n", len(pklData), cppCount, goCount, rustCount)
		os.Exit(1)
	}
	fmt.Printf("PASS: All files have %d constants\n", len(pklData))
	fmt.Println()

	// Verify values match
	mismatches := 0
	keys := make([]string, 0, len(pklData))
	for key := range pklData {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	for _, key := range keys {
		expected := pklData[key]
		cppName := camelToScreaming(key)
		goName := camelToPascal(key)
		rustName := camelToScreaming(key)

		cppVal := extractValue(string(cppContent), cppName, "cpp")
		goVal := extractValue(string(goContent), goName, "go")
		rustVal := extractValue(string(rustContent), rustName, "rust")

		if cppVal != expected || goVal != expected || rustVal != expected {
			mismatches++
			fmt.Printf("MISMATCH: %s\n", key)
			fmt.Printf("    Expected: %d\n", expected)
			fmt.Printf("    C++ (%s): %d\n", cppName, cppVal)
			fmt.Printf("    Go (%s): %d\n", goName, goVal)
			fmt.Printf("    Rust (%s): %d\n", rustName, rustVal)
		}
	}

	if mismatches == 0 {
		fmt.Printf("PASS: All %d constant values match across C++, Go, and Rust\n", len(pklData))
		fmt.Println()
		fmt.Println("==============================================")
		fmt.Println(" VERIFICATION PASSED")
		fmt.Printf(" All %d constants have identical values in C++, Go, and Rust\n", len(pklData))
		fmt.Println("==============================================")
	} else {
		fmt.Println()
		fmt.Println("==============================================")
		fmt.Printf(" VERIFICATION FAILED: %d mismatches\n", mismatches)
		fmt.Println("==============================================")
		os.Exit(1)
	}
}

func camelToScreaming(key string) string {
	// Special cases
	switch key {
	case "magic":
		return "TABLE_MAGIC"
	case "version":
		return "TABLE_VERSION"
	case "xxHashSeed":
		return "XXHASH_SEED"
	}

	// General case: insert _ before uppercase preceded by lowercase or digit, then uppercase
	var result strings.Builder
	for i, c := range key {
		if c >= 'A' && c <= 'Z' && i > 0 {
			prev := rune(key[i-1])
			if (prev >= 'a' && prev <= 'z') || (prev >= '0' && prev <= '9') {
				result.WriteRune('_')
			}
		}
		if c >= 'a' && c <= 'z' {
			result.WriteRune(c - 'a' + 'A')
		} else {
			result.WriteRune(c)
		}
	}
	return result.String()
}

func camelToPascal(key string) string {
	// Special cases
	switch key {
	case "magic":
		return "TableMagic"
	case "version":
		return "TableVersion"
	case "xxHashSeed":
		return "XXHashSeed"
	}

	// General case: capitalize first letter, keep the rest
	if key == "" {
		return key
	}
	first := rune(key[0])
	if first >= 'a' && first <= 'z' {
		return string(first-'a'+'A') + key[1:]
	}
	return key
}

func extractValue(text, name, lang string) int64 {
	// Use word boundary to avoid matching prefixes (e.g., TableVersion vs TableVersionOffset)
	var patterns []*regexp.Regexp

	switch lang {
	case "cpp":
		patterns = []*regexp.Regexp{
			regexp.MustCompile(`constexpr\s+\S+\s+` + regexp.QuoteMeta(name) + `\b\s*=\s*(0x[0-9A-Fa-f]+|\d+)`),
			regexp.MustCompile(`\b` + regexp.QuoteMeta(name) + `\b\s*(?:\S+\s*)?=\s*(0x[0-9A-Fa-f]+|\d+)`),
		}
	case "go":
		patterns = []*regexp.Regexp{
			regexp.MustCompile(`\b` + regexp.QuoteMeta(name) + `\b\s*(?:\S+\s*)?=\s*(0x[0-9A-Fa-f]+|\d+)`),
		}
	case "rust":
		patterns = []*regexp.Regexp{
			regexp.MustCompile(`pub\s+const\s+` + regexp.QuoteMeta(name) + `\b\s*:\s*\S+\s*=\s*(0x[0-9A-Fa-f]+|\d+)`),
			regexp.MustCompile(`\b` + regexp.QuoteMeta(name) + `\b\s*(?:\S+\s*)?=\s*(0x[0-9A-Fa-f]+|\d+)`),
		}
	}

	for _, pattern := range patterns {
		m := pattern.FindStringSubmatch(text)
		if m != nil {
			// Use base 0 to auto-detect number format (handles 0x prefix for hex)
			n, _ := strconv.ParseInt(m[1], 0, 64)
			return n
		}
	}
	return -1
}

func findProjectRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", err
	}
	// Walk up the directory tree looking for go.mod
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			// Found go.mod (in the go/ subdirectory). The project root is the
			// parent directory which contains constants.pkl.
			parent := filepath.Dir(dir)
			if _, err := os.Stat(filepath.Join(parent, "constants.pkl")); err == nil {
				return parent, nil
			}
			// If parent doesn't have constants.pkl, use the go.mod directory
			return dir, nil
		}
		// If current dir has constants.pkl, it's the project root
		if _, err := os.Stat(filepath.Join(dir, "constants.pkl")); err == nil {
			return dir, nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	// Fallback: use current directory
	return os.Getwd()
}
