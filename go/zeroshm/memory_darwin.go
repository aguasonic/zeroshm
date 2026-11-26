//go:build darwin

// macOS/Darwin backend: POSIX shared memory via shm_open/shm_unlink.
// macOS does not expose /dev/shm; shared memory is implemented via kernel
// objects accessed through the POSIX shm_open API.
//
// Uses the gopkg.in/ro-ag/posix.v1 package which provides CGO-free bindings
// to libc functions via //go:cgo_import_dynamic, allowing builds with CGO_ENABLED=0.

package zeroshm

import (
	"errors"
	"fmt"

	"golang.org/x/sys/unix"
	posix "gopkg.in/ro-ag/posix.v1"
)

// Note: macOS shared memory names are capped at 31 characters.

func (m *Memory) create() error {
	// Hash the logical name to produce a short OS-level name (macOS limit: 31 chars)
	name := osSegmentName(m.name)

	// Try to create shared memory (O_EXCL means fail if exists)
	fd, err := posix.ShmOpen(name, unix.O_CREAT|unix.O_RDWR|unix.O_EXCL, uint32(0666))
	if err != nil {
		if errors.Is(err, unix.EEXIST) {
			// Unlink and retry
			_ = posix.ShmUnlink(name)
			fd, err = posix.ShmOpen(name, unix.O_CREAT|unix.O_RDWR|unix.O_EXCL, uint32(0666))
		}
		if err != nil {
			return fmt.Errorf("shm_open: %w", err)
		}
	}
	m.fd = int(fd)

	// Set size
	if err := unix.Ftruncate(m.fd, int64(m.size)); err != nil {
		unix.Close(m.fd)
		posix.ShmUnlink(name)
		return fmt.Errorf("ftruncate: %w", err)
	}

	// Map memory
	data, err := unix.Mmap(m.fd, 0, m.size, unix.PROT_READ|unix.PROT_WRITE, unix.MAP_SHARED)
	if err != nil {
		unix.Close(m.fd)
		posix.ShmUnlink(name)
		return fmt.Errorf("mmap: %w", err)
	}
	m.data = data

	// Zero out memory (macOS does not guarantee zero-fill like Linux /dev/shm)
	for i := range data {
		data[i] = 0
	}

	return nil
}

func (m *Memory) open() error {
	// Hash the logical name to produce a short OS-level name
	name := osSegmentName(m.name)

	// Open existing shared memory
	fd, err := posix.ShmOpen(name, unix.O_RDWR, uint32(0666))
	if err != nil {
		return fmt.Errorf("shm_open: %w", err)
	}
	m.fd = int(fd)

	// Get size using fstat
	var stat unix.Stat_t
	if err := unix.Fstat(m.fd, &stat); err != nil {
		unix.Close(m.fd)
		return fmt.Errorf("fstat: %w", err)
	}
	m.size = int(stat.Size)

	// Map memory
	data, err := unix.Mmap(m.fd, 0, m.size, unix.PROT_READ|unix.PROT_WRITE, unix.MAP_SHARED)
	if err != nil {
		unix.Close(m.fd)
		return fmt.Errorf("mmap: %w", err)
	}
	m.data = data

	return nil
}

// Close unmaps the memory and closes the file descriptor.
func (m *Memory) Close() error {
	var errs []error

	if m.data != nil {
		if err := unix.Munmap(m.data); err != nil {
			errs = append(errs, fmt.Errorf("munmap: %w", err))
		}
		m.data = nil
	}

	if m.fd >= 0 {
		if err := unix.Close(m.fd); err != nil {
			errs = append(errs, fmt.Errorf("close: %w", err))
		}
		m.fd = -1
	}

	if len(errs) > 0 {
		return errors.Join(errs...)
	}
	return nil
}

// Unlink removes the shared memory segment from the system.
func (m *Memory) Unlink() error {
	return posix.ShmUnlink(osSegmentName(m.name))
}

// UnlinkName removes a shared memory segment by name.
func UnlinkName(name string) error {
	return posix.ShmUnlink(osSegmentName(name))
}
