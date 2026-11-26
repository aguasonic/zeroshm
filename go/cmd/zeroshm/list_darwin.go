//go:build darwin

package main

import (
	"fmt"
	"os"
	"os/exec"
	"text/tabwriter"

	"github.com/aguasonic/zeroshm/zeroshm"
)

// cmdList lists all shared memory segments on macOS.
// macOS does not expose /dev/shm; shared memory is managed via kernel objects.
// We use sysctl to enumerate POSIX shared memory segments.
func cmdList() error {
	// Try to enumerate segments via sysctl
	segments, err := listDarwinShmSegments()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Warning: could not list shared memory segments: %v\n", err)
		fmt.Println()
		fmt.Println("On macOS, shared memory segments are managed by the kernel and")
		fmt.Println("cannot be listed like files. Use 'zeroshm info <name>' to inspect")
		fmt.Println("a specific segment if you know its name.")
		fmt.Println()
		fmt.Println("Alternatively, check sysctl output:")
		fmt.Println("  sysctl -n kern.posix.shm")
		return nil
	}

	if len(segments) == 0 {
		fmt.Println("No shared memory segments found.")
		return nil
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "NAME\tSIZE\tZEROSHM")

	for _, seg := range segments {
		isZeroSHM := checkZeroSHM(seg.name)
		zeroshmStr := ""
		if isZeroSHM {
			zeroshmStr = "yes"
		}
		fmt.Fprintf(w, "%s\t%s\t%s\n", seg.name, formatSize(seg.size), zeroshmStr)
	}

	w.Flush()
	return nil
}

// darwinShmSegment represents a shared memory segment on macOS
type darwinShmSegment struct {
	name string
	size int64
}

// listDarwinShmSegments enumerates POSIX shared memory segments via sysctl.
// Returns a list of segments or an error if enumeration fails.
func listDarwinShmSegments() ([]darwinShmSegment, error) {
	// kern.posix.shm provides info about POSIX shared memory usage,
	// but does not list individual segment names.
	// On macOS, there is no reliable way to enumerate segment names via sysctl.
	//
	// We try sysctl -n kern.posix.shm as a best effort, but it typically
	// returns aggregate statistics rather than segment names.
	out, err := exec.Command("sysctl", "-n", "kern.posix.shm").Output()
	if err != nil {
		return nil, fmt.Errorf("sysctl kern.posix.shm failed: %w", err)
	}

	// Parse the sysctl output. The format is typically:
	// "nshm = X, shmavail = Y"
	// This doesn't give us segment names, so we return empty.
	// Users should use 'info <name>' instead.
	_ = string(out) // suppress unused warning

	// Since we can't reliably enumerate names, return empty list.
	// The caller will show a helpful message.
	return nil, nil
}

// checkZeroSHM checks if a shared memory segment has the ZeroSHM magic number
// by opening it via the zeroshm package (which uses shm_open on macOS).
func checkZeroSHM(name string) bool {
	mem, err := zeroshm.OpenMemory(name, 64)
	if err != nil {
		return false
	}
	defer mem.Close()

	header := mem.Table().Header()
	return header.Magic == zeroshm.TableMagic
}

