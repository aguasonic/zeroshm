//go:build linux

package main

import (
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"text/tabwriter"

	"github.com/aguasonic/zeroshm/zeroshm"
)

// cmdList lists all shared memory segments in /dev/shm
func cmdList() error {
	entries, err := os.ReadDir("/dev/shm")
	if err != nil {
		return fmt.Errorf("failed to read /dev/shm: %w", err)
	}

	if len(entries) == 0 {
		fmt.Println("No shared memory segments found.")
		return nil
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "NAME\tSIZE\tZEROSHM")

	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}

		info, err := entry.Info()
		if err != nil {
			continue
		}

		name := "/" + entry.Name()
		size := info.Size()
		isZeroSHM := checkZeroSHM(name)

		zeroshmStr := ""
		if isZeroSHM {
			zeroshmStr = "yes"
		}

		fmt.Fprintf(w, "%s\t%s\t%s\n", name, formatSize(size), zeroshmStr)
	}

	w.Flush()
	return nil
}

// checkZeroSHM checks if a shared memory segment has the ZeroSHM magic number
func checkZeroSHM(name string) bool {
	path := filepath.Join("/dev/shm", strings.TrimPrefix(name, "/"))
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()

	var magic uint32
	if err := binary.Read(f, binary.LittleEndian, &magic); err != nil {
		return false
	}

	return magic == zeroshm.TableMagic
}
