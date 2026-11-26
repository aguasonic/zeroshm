package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
	"unsafe"
)

func setupLatestTest[T any](t *testing.T, namePrefix string, initialValue T) (*Memory, *Latest[T]) {
	t.Helper()
	name := fmt.Sprintf("/latest_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	latest, err := NewLatest[T](mem, "latest_"+namePrefix, initialValue)
	if err != nil {
		t.Fatalf("NewLatest failed: %v", err)
	}
	t.Cleanup(func() { latest.Destroy() })

	return mem, latest
}

// GPSFix is a struct used to test Latest with composite types.
type GPSFix struct {
	Latitude  float64
	Longitude float64
	Altitude  float64
	Timestamp int64
}

// Test 1: Basic Get/Set
func TestLatestBasicGetSet(t *testing.T) {
	_, latest := setupLatestTest[int32](t, "basic", 42)

	// Initial value should be 42 with version 1
	value, version := latest.Get()
	if value != 42 {
		t.Errorf("expected initial value 42, got %d", value)
	}
	if version != 1 {
		t.Errorf("expected initial version 1, got %d", version)
	}

	// Set new value
	latest.Set(99)

	value, version = latest.Get()
	if value != 99 {
		t.Errorf("expected value 99 after set, got %d", value)
	}
	if version != 2 {
		t.Errorf("expected version 2 after set, got %d", version)
	}
}

// Test 2: Version monotonicity
func TestLatestVersionMonotonicity(t *testing.T) {
	_, latest := setupLatestTest[int32](t, "monotonic", 0)

	prevVersion := latest.Version()
	for i := 1; i <= 100; i++ {
		latest.Set(int32(i))
		value, version := latest.Get()

		if value != int32(i) {
			t.Errorf("iteration %d: expected value %d, got %d", i, i, value)
		}
		if version != prevVersion+1 {
			t.Errorf("iteration %d: expected version %d, got %d", i, prevVersion+1, version)
		}
		if version <= prevVersion {
			t.Errorf("iteration %d: version not monotonically increasing (%d -> %d)", i, prevVersion, version)
		}
		prevVersion = version
	}
}

// Test 3: WaitForChange success
func TestLatestWaitForChangeSuccess(t *testing.T) {
	_, latest := setupLatestTest[int32](t, "wait_success", 0)

	_, lastVersion := latest.Get()

	// Producer: set a new value after a delay
	go func() {
		time.Sleep(50 * time.Millisecond)
		latest.Set(42)
	}()

	// Consumer: wait for change with generous timeout
	value, version, changed := latest.WaitForChange(lastVersion, 500*time.Millisecond)
	if !changed {
		t.Error("expected WaitForChange to detect a change")
	}
	if value != 42 {
		t.Errorf("expected value 42, got %d", value)
	}
	if version <= lastVersion {
		t.Errorf("expected version > %d, got %d", lastVersion, version)
	}
}

// Test 4: WaitForChange timeout
func TestLatestWaitForChangeTimeout(t *testing.T) {
	_, latest := setupLatestTest[int32](t, "wait_timeout", 42)

	_, lastVersion := latest.Get()

	// Wait for change with short timeout, but no producer writes
	start := time.Now()
	value, version, changed := latest.WaitForChange(lastVersion, 50*time.Millisecond)
	elapsed := time.Since(start)

	if changed {
		t.Error("expected WaitForChange to time out (no change)")
	}
	if value != 42 {
		t.Errorf("expected value 42, got %d", value)
	}
	if version != lastVersion {
		t.Errorf("expected version %d, got %d", lastVersion, version)
	}
	if elapsed < 40*time.Millisecond {
		t.Errorf("expected wait of ~50ms, got %v", elapsed)
	}
}

// Test 5: Multiple concurrent readers
func TestLatestConcurrentReaders(t *testing.T) {
	_, latest := setupLatestTest[int32](t, "concurrent", 0)

	const numReaders = 8
	const duration = time.Second

	done := make(chan struct{})
	var wg sync.WaitGroup

	// Producer
	go func() {
		i := int32(0)
		for {
			select {
			case <-done:
				return
			default:
			}
			latest.Set(i)
			i++
			time.Sleep(1 * time.Millisecond)
		}
	}()

	// Readers
	for r := 0; r < numReaders; r++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			prevVersion := uint64(0)
			for {
				select {
				case <-done:
					return
				default:
				}
				_, version := latest.Get()
				if version < prevVersion && prevVersion > 0 {
					// Version should never decrease
					t.Errorf("reader saw decreasing version: %d -> %d", prevVersion, version)
					return
				}
				if version > 0 {
					prevVersion = version
				}
			}
		}()
	}

	// Run for the specified duration
	time.Sleep(duration)
	close(done)
	wg.Wait()

	// Verify final version is reasonably high (producer ran for 1 second at 1ms intervals)
	finalVersion := latest.Version()
	if finalVersion < 500 {
		t.Errorf("expected at least 500 versions after 1 second, got %d", finalVersion)
	}
}

// Test 6: Struct type support
func TestLatestStructSupport(t *testing.T) {
	initial := GPSFix{Latitude: 0, Longitude: 0, Altitude: 0, Timestamp: 0}
	_, latest := setupLatestTest[GPSFix](t, "struct", initial)

	fix := GPSFix{Latitude: 37.7749, Longitude: -122.4194, Altitude: 52.3, Timestamp: 1234567890}
	latest.Set(fix)

	value, version := latest.Get()
	if value.Latitude != fix.Latitude {
		t.Errorf("expected lat %f, got %f", fix.Latitude, value.Latitude)
	}
	if value.Longitude != fix.Longitude {
		t.Errorf("expected lon %f, got %f", fix.Longitude, value.Longitude)
	}
	if value.Altitude != fix.Altitude {
		t.Errorf("expected alt %f, got %f", fix.Altitude, value.Altitude)
	}
	if value.Timestamp != fix.Timestamp {
		t.Errorf("expected ts %d, got %d", fix.Timestamp, value.Timestamp)
	}
	if version != 2 {
		t.Errorf("expected version 2, got %d", version)
	}
}

// Test 7: Alignment verification
func TestLatestAlignment(t *testing.T) {
	tests := []struct {
		size     int
		expected int // expected aligned size
	}{
		{size: 1, expected: 8},
		{size: 3, expected: 8},
		{size: 7, expected: 8},
		{size: 8, expected: 8},
		{size: 9, expected: 16},
		{size: 15, expected: 16},
		{size: 16, expected: 16},
	}

	for _, tc := range tests {
		t.Run(fmt.Sprintf("size_%d", tc.size), func(t *testing.T) {
			aligned := alignUp(tc.size, 8)
			if aligned != tc.expected {
				t.Errorf("alignUp(%d, 8) = %d, expected %d", tc.size, aligned, tc.expected)
			}
		})
	}

	// Verify alignment for actual types in shared memory
	name := fmt.Sprintf("/latest_align_%d", os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	// Create Latest with uint8 (1 byte) -- version should be at offset align_up(1,8) = 8
	l8, err := NewLatest[uint8](mem, "latest_u8", 42)
	if err != nil {
		t.Fatalf("NewLatest[uint8] failed: %v", err)
	}
	l8.Destroy()
	if l8.latestVersionOffset()%8 != 0 {
		t.Errorf("uint8: version offset %d is not 8-byte aligned", l8.latestVersionOffset())
	}

	// Create Latest with int64 (8 bytes) -- version should be at offset align_up(8,8) = 8
	l64, err := NewLatest[int64](mem, "latest_i64", 42)
	if err != nil {
		t.Fatalf("NewLatest[int64] failed: %v", err)
	}
	l64.Destroy()
	if l64.latestVersionOffset()%8 != 0 {
		t.Errorf("int64: version offset %d is not 8-byte aligned", l64.latestVersionOffset())
	}

	// Create Latest with GPSFix (32 bytes) -- version should be at offset 32
	lGPS, err := NewLatest[GPSFix](mem, "latest_gps", GPSFix{})
	if err != nil {
		t.Fatalf("NewLatest[GPSFix] failed: %v", err)
	}
	lGPS.Destroy()
	if lGPS.latestVersionOffset()%8 != 0 {
		t.Errorf("GPSFix: version offset %d is not 8-byte aligned", lGPS.latestVersionOffset())
	}
}

// Test 8: Open after create
func TestLatestOpenAfterCreate(t *testing.T) {
	mem, latest := setupLatestTest[int32](t, "open", 42)

	latest.Set(99)

	// Open the same Latest by name
	opened, err := OpenLatest[int32](mem, "latest_open")
	if err != nil {
		t.Fatalf("OpenLatest failed: %v", err)
	}
	t.Cleanup(func() { opened.Destroy() })

	// Verify same value and version
	origValue, origVersion := latest.Get()
	openValue, openVersion := opened.Get()
	if origValue != openValue {
		t.Errorf("value mismatch: original %d, opened %d", origValue, openValue)
	}
	if origVersion != openVersion {
		t.Errorf("version mismatch: original %d, opened %d", origVersion, openVersion)
	}
}

// Test 9: Error cases
func TestLatestErrorCases(t *testing.T) {
	name := fmt.Sprintf("/latest_err_%d", os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	// Duplicate name
	_, err = NewLatest[int32](mem, "latest_dup", 42)
	if err != nil {
		t.Fatalf("first NewLatest failed: %v", err)
	}

	_, err = NewLatest[int32](mem, "latest_dup", 99)
	if err == nil {
		t.Error("expected error for duplicate name")
	}

	// Non-existent name
	_, err = OpenLatest[int32](mem, "latest_nonexistent")
	if err == nil {
		t.Error("expected error for non-existent name")
	}

	// Wrong type (size mismatch)
	lInt32, err := NewLatest[int32](mem, "latest_type", 42)
	if err != nil {
		t.Fatalf("NewLatest[int32] failed: %v", err)
	}
	t.Cleanup(func() { lInt32.Destroy() })

	// Try to open with int64 (different size)
	_, err = OpenLatest[int64](mem, "latest_type")
	if err == nil {
		t.Error("expected error for wrong type (size mismatch)")
	}
}

// Test 10: Zero-size type rejection
func TestLatestZeroSizeType(t *testing.T) {
	type Empty struct{}

	name := fmt.Sprintf("/latest_zero_%d", os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	_, err = NewLatest[Empty](mem, "latest_zero", Empty{})
	if err == nil {
		t.Error("expected error for zero-size type")
	}
}

// Additional: Verify unsafe.Sizeof for GPSFix is 32 bytes
func TestLatestGPSSize(t *testing.T) {
	size := int(unsafe.Sizeof(GPSFix{}))
	if size != 32 {
		t.Errorf("expected GPSFix size 32 bytes, got %d", size)
	}
}

// Additional: Version() method
func TestLatestVersionMethod(t *testing.T) {
	_, latest := setupLatestTest[int32](t, "version", 0)

	if latest.Version() != 1 {
		t.Errorf("expected initial version 1, got %d", latest.Version())
	}

	latest.Set(1)
	if latest.Version() != 2 {
		t.Errorf("expected version 2, got %d", latest.Version())
	}

	latest.Set(2)
	if latest.Version() != 3 {
		t.Errorf("expected version 3, got %d", latest.Version())
	}
}

// 
// Latest layout tests (unified binary format with pthread mutex + condvar)
// 

// TestLatestPthreadSizes verifies pthread structure sizes and alignments.
func TestLatestPthreadSizes(t *testing.T) {
	fmt.Printf("pthread_mutex_t: size=%d, align=%d\n", pthreadMutexSize, pthreadMutexAlign)
	fmt.Printf("pthread_cond_t: size=%d, align=%d\n", pthreadCondSize, pthreadCondAlign)
	if pthreadMutexAlign > 8 {
		t.Errorf("pthread_mutex_t alignment %d exceeds 8-byte guarantee", pthreadMutexAlign)
	}
	if pthreadCondAlign > 8 {
		t.Errorf("pthread_cond_t alignment %d exceeds 8-byte guarantee", pthreadCondAlign)
	}
}

// TestLatestLayoutElemSize4 verifies layout for elem_size=4.
func TestLatestLayoutElemSize4(t *testing.T) {
	elemSize := 4
	total := LatestTotalSize(elemSize)
	verOff := LatestVersionOffset(0, elemSize)
	mutexOff := LatestMutexOffset(0, elemSize)
	condOff := LatestCondOffset(0, elemSize)

	fmt.Printf("Latest layout (elem_size=%d): total=%d, version=%d, mutex=%d, cond=%d\n",
		elemSize, total, verOff, mutexOff, condOff)

	if verOff%8 != 0 {
		t.Errorf("version offset %d is not 8-aligned", verOff)
	}
	if verOff >= mutexOff {
		t.Errorf("version offset %d should be < mutex offset %d", verOff, mutexOff)
	}
	if mutexOff >= condOff {
		t.Errorf("mutex offset %d should be < cond offset %d", mutexOff, condOff)
	}
	if total != condOff+pthreadCondSize {
		t.Errorf("total size %d should equal cond offset %d + cond size %d",
			total, condOff, pthreadCondSize)
	}
}

// TestLatestLayoutElemSize8 verifies layout for elem_size=8.
func TestLatestLayoutElemSize8(t *testing.T) {
	elemSize := 8
	total := LatestTotalSize(elemSize)
	verOff := LatestVersionOffset(0, elemSize)
	mutexOff := LatestMutexOffset(0, elemSize)
	condOff := LatestCondOffset(0, elemSize)

	fmt.Printf("Latest layout (elem_size=%d): total=%d, version=%d, mutex=%d, cond=%d\n",
		elemSize, total, verOff, mutexOff, condOff)

	if verOff%8 != 0 {
		t.Errorf("version offset %d is not 8-aligned", verOff)
	}
	if verOff >= mutexOff {
		t.Errorf("version offset %d should be < mutex offset %d", verOff, mutexOff)
	}
	if mutexOff >= condOff {
		t.Errorf("mutex offset %d should be < cond offset %d", mutexOff, condOff)
	}
	if total != condOff+pthreadCondSize {
		t.Errorf("total size %d should equal cond offset %d + cond size %d",
			total, condOff, pthreadCondSize)
	}
}

// TestLatestLayoutElemSize16 verifies layout for elem_size=16.
func TestLatestLayoutElemSize16(t *testing.T) {
	elemSize := 16
	total := LatestTotalSize(elemSize)
	verOff := LatestVersionOffset(0, elemSize)
	mutexOff := LatestMutexOffset(0, elemSize)
	condOff := LatestCondOffset(0, elemSize)

	fmt.Printf("Latest layout (elem_size=%d): total=%d, version=%d, mutex=%d, cond=%d\n",
		elemSize, total, verOff, mutexOff, condOff)

	if verOff%8 != 0 {
		t.Errorf("version offset %d is not 8-aligned", verOff)
	}
	if verOff >= mutexOff {
		t.Errorf("version offset %d should be < mutex offset %d", verOff, mutexOff)
	}
	if mutexOff >= condOff {
		t.Errorf("mutex offset %d should be < cond offset %d", mutexOff, condOff)
	}
	if total != condOff+pthreadCondSize {
		t.Errorf("total size %d should equal cond offset %d + cond size %d",
			total, condOff, pthreadCondSize)
	}
}

// TestLatestLayoutSummary prints all layout values for cross-language comparison.
func TestLatestLayoutSummary(t *testing.T) {
	fmt.Println("\n=== Go Latest Layout Summary ===")
	fmt.Printf("pthread_mutex_t: size=%d, align=%d\n", pthreadMutexSize, pthreadMutexAlign)
	fmt.Printf("pthread_cond_t: size=%d, align=%d\n", pthreadCondSize, pthreadCondAlign)
	for _, elemSize := range []int{4, 8, 16} {
		fmt.Printf("elem_size=%d: total=%d, version=%d, mutex=%d, cond=%d\n",
			elemSize,
			LatestTotalSize(elemSize),
			LatestVersionOffset(0, elemSize),
			LatestMutexOffset(0, elemSize),
			LatestCondOffset(0, elemSize),
		)
	}
	fmt.Println("=== End Go Summary ===")
}
