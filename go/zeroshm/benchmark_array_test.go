package zeroshm

import (
	"fmt"
	"math/rand"
	"os"
	"testing"
)

func setupArrayBenchmark(b *testing.B) (*Memory, func()) {
	b.Helper()
	name := fmt.Sprintf("/bench_a_%d", os.Getpid())
	UnlinkName(name)
	mem, err := NewMemory(name, 100*1024*1024, 64)
	if err != nil {
		b.Fatalf("NewMemory failed: %v", err)
	}
	cleanup := func() {
		mem.Close()
		UnlinkName(name)
	}
	return mem, cleanup
}

// BenchmarkArraySequentialRead measures sequential read throughput with various array sizes.
func BenchmarkArraySequentialRead(b *testing.B) {
	sizes := []int{1000, 10000, 100000, 1000000}

	for _, size := range sizes {
		b.Run(fmt.Sprintf("Size%d", size), func(b *testing.B) {
			mem, cleanup := setupArrayBenchmark(b)
			defer cleanup()

			array, err := NewArray[uint64](mem, "arr", size)
			if err != nil {
				b.Fatalf("NewArray failed: %v", err)
			}

			// Initialize
			for i := 0; i < size; i++ {
				array.Set(i, uint64(i))
			}

			b.ResetTimer()
			var sum uint64
			for i := 0; i < b.N; i++ {
				for j := 0; j < size; j++ {
					sum += array.Get(j)
				}
			}
			_ = sum
			b.SetBytes(int64(size) * 8)
		})
	}
}

// BenchmarkArraySequentialWrite measures sequential write throughput.
func BenchmarkArraySequentialWrite(b *testing.B) {
	sizes := []int{1000, 10000, 100000, 1000000}

	for _, size := range sizes {
		b.Run(fmt.Sprintf("Size%d", size), func(b *testing.B) {
			mem, cleanup := setupArrayBenchmark(b)
			defer cleanup()

			array, err := NewArray[uint64](mem, "arr", size)
			if err != nil {
				b.Fatalf("NewArray failed: %v", err)
			}

			b.ResetTimer()
			for i := 0; i < b.N; i++ {
				for j := 0; j < size; j++ {
					array.Set(j, uint64(j))
				}
			}
			b.SetBytes(int64(size) * 8)
		})
	}
}

// BenchmarkArrayRandomRead measures random read throughput.
func BenchmarkArrayRandomRead(b *testing.B) {
	mem, cleanup := setupArrayBenchmark(b)
	defer cleanup()

	const size = 1000000
	array, err := NewArray[uint64](mem, "random", size)
	if err != nil {
		b.Fatalf("NewArray failed: %v", err)
	}

	// Initialize
	for i := 0; i < size; i++ {
		array.Set(i, uint64(i))
	}

	// Generate random indices
	rng := rand.New(rand.NewSource(42))
	indices := make([]int, 100000)
	for i := range indices {
		indices[i] = rng.Intn(size)
	}

	b.ResetTimer()
	var sum uint64
	for i := 0; i < b.N; i++ {
		for _, idx := range indices {
			sum += array.Get(idx)
		}
	}
	_ = sum
	b.SetBytes(int64(len(indices)) * 8)
}

// BenchmarkArrayRandomWrite measures random write throughput.
func BenchmarkArrayRandomWrite(b *testing.B) {
	mem, cleanup := setupArrayBenchmark(b)
	defer cleanup()

	const size = 1000000
	array, err := NewArray[uint64](mem, "random", size)
	if err != nil {
		b.Fatalf("NewArray failed: %v", err)
	}

	// Initialize
	for i := 0; i < size; i++ {
		array.Set(i, uint64(i))
	}

	// Generate random indices
	rng := rand.New(rand.NewSource(42))
	indices := make([]int, 100000)
	for i := range indices {
		indices[i] = rng.Intn(size)
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		for _, idx := range indices {
			array.Set(idx, uint64(idx*2))
		}
	}
	b.SetBytes(int64(len(indices)) * 8)
}

// BenchmarkArrayStrideAccess measures access with different stride patterns.
func BenchmarkArrayStrideAccess(b *testing.B) {
	mem, cleanup := setupArrayBenchmark(b)
	defer cleanup()

	const size = 1000000
	array, err := NewArray[uint64](mem, "stride", size)
	if err != nil {
		b.Fatalf("NewArray failed: %v", err)
	}

	// Initialize
	for i := 0; i < size; i++ {
		array.Set(i, uint64(i))
	}

	strides := []int{1, 8, 64}
	for _, stride := range strides {
		b.Run(fmt.Sprintf("Stride%d", stride), func(b *testing.B) {
			b.ResetTimer()
			var sum uint64
			for i := 0; i < b.N; i++ {
				for j := 0; j < size; j += stride {
					sum += array.Get(j)
				}
			}
			_ = sum
		})
	}
}
