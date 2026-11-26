package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
)

func setupQueueBenchmark(b *testing.B, capacity int) (*Memory, func()) {
	b.Helper()
	name := fmt.Sprintf("/bench_q_%d", os.Getpid())
	UnlinkName(name)
	mem, err := NewMemory(name, 512*1024*1024, 64)
	if err != nil {
		b.Fatalf("NewMemory failed: %v", err)
	}
	cleanup := func() {
		mem.Close()
		UnlinkName(name)
	}
	return mem, cleanup
}

// BenchmarkQueuePush measures single-thread push throughput with different element sizes.
func BenchmarkQueuePush(b *testing.B) {
	cases := []struct {
		name     string
		capacity int
		bench    func(*testing.B, *Memory, int)
	}{
		{"U8", 100000, benchQueuePushU8},
		{"U32", 100000, benchQueuePushU32},
		{"U64", 100000, benchQueuePushU64},
	}

	for _, tc := range cases {
		b.Run(tc.name, func(b *testing.B) {
			mem, cleanup := setupQueueBenchmark(b, tc.capacity)
			defer cleanup()

			// Create the queue with the type that matches what we'll benchmark
			switch tc.name {
			case "U8":
				_, err := NewQueue[uint8](mem, "que", tc.capacity)
				if err != nil {
					b.Fatalf("NewQueue failed: %v", err)
				}
			case "U32":
				_, err := NewQueue[uint32](mem, "que", tc.capacity)
				if err != nil {
					b.Fatalf("NewQueue failed: %v", err)
				}
			default:
				_, err := NewQueue[uint64](mem, "que", tc.capacity)
				if err != nil {
					b.Fatalf("NewQueue failed: %v", err)
				}
			}

			b.ResetTimer()
			tc.bench(b, mem, tc.capacity)
		})
	}
}

// BenchmarkQueuePop measures single-thread pop throughput.
func BenchmarkQueuePop(b *testing.B) {
	cases := []struct {
		name     string
		capacity int
		bench    func(*testing.B, *Memory, int)
	}{
		{"U8", 100000, benchQueuePopU8},
		{"U32", 100000, benchQueuePopU32},
		{"U64", 100000, benchQueuePopU64},
	}

	for _, tc := range cases {
		b.Run(tc.name, func(b *testing.B) {
			mem, cleanup := setupQueueBenchmark(b, tc.capacity)
			defer cleanup()

			// Create and pre-fill with matching type
			switch tc.name {
			case "U8":
				queue, err := NewQueue[uint8](mem, "que", tc.capacity)
				if err != nil {
					b.Fatalf("NewQueue failed: %v", err)
				}
				for i := 0; i < tc.capacity; i++ {
					queue.Push(uint8(i))
				}
			case "U32":
				queue, err := NewQueue[uint32](mem, "que", tc.capacity)
				if err != nil {
					b.Fatalf("NewQueue failed: %v", err)
				}
				for i := 0; i < tc.capacity; i++ {
					queue.Push(uint32(i))
				}
			default:
				queue, err := NewQueue[uint64](mem, "que", tc.capacity)
				if err != nil {
					b.Fatalf("NewQueue failed: %v", err)
				}
				for i := 0; i < tc.capacity; i++ {
					queue.Push(uint64(i))
				}
			}

			b.ResetTimer()
			tc.bench(b, mem, tc.capacity)
		})
	}
}

func benchQueuePushU8(b *testing.B, mem *Memory, capacity int) {
	queue, _ := OpenQueue[uint8](mem, "que")
	for i := 0; i < b.N; i++ {
		for !queue.Push(0) {
			queue.Pop()
		}
	}
}

func benchQueuePushU32(b *testing.B, mem *Memory, capacity int) {
	queue, _ := OpenQueue[uint32](mem, "que")
	for i := 0; i < b.N; i++ {
		for !queue.Push(0) {
			queue.Pop()
		}
	}
}

func benchQueuePushU64(b *testing.B, mem *Memory, capacity int) {
	queue, _ := OpenQueue[uint64](mem, "que")
	for i := 0; i < b.N; i++ {
		for !queue.Push(0) {
			queue.Pop()
		}
	}
}

func benchQueuePopU8(b *testing.B, mem *Memory, capacity int) {
	queue, _ := OpenQueue[uint8](mem, "que")
	for i := 0; i < b.N; i++ {
		for _, ok := queue.Pop(); !ok; _, ok = queue.Pop() {
			queue.Push(0)
		}
	}
}

func benchQueuePopU32(b *testing.B, mem *Memory, capacity int) {
	queue, _ := OpenQueue[uint32](mem, "que")
	for i := 0; i < b.N; i++ {
		for _, ok := queue.Pop(); !ok; _, ok = queue.Pop() {
			queue.Push(0)
		}
	}
}

func benchQueuePopU64(b *testing.B, mem *Memory, capacity int) {
	queue, _ := OpenQueue[uint64](mem, "que")
	for i := 0; i < b.N; i++ {
		for _, ok := queue.Pop(); !ok; _, ok = queue.Pop() {
			queue.Push(0)
		}
	}
}

// BenchmarkQueueLatency measures per-operation push latency.
func BenchmarkQueueLatency(b *testing.B) {
	mem, cleanup := setupQueueBenchmark(b, 100000)
	defer cleanup()

	queue, err := NewQueue[uint64](mem, "lat", 100000)
	if err != nil {
		b.Fatalf("NewQueue failed: %v", err)
	}

	// Warmup
	for i := 0; i < 1000; i++ {
		queue.Push(uint64(i))
		queue.Pop()
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		queue.Push(uint64(i))
	}
}

// BenchmarkQueuePopLatency measures per-pop latency.
func BenchmarkQueuePopLatency(b *testing.B) {
	mem, cleanup := setupQueueBenchmark(b, 100000)
	defer cleanup()

	queue, err := NewQueue[uint64](mem, "lat", 100000)
	if err != nil {
		b.Fatalf("NewQueue failed: %v", err)
	}

	// Pre-fill
	for i := 0; i < 100000; i++ {
		queue.Push(uint64(i))
	}

	// Warmup
	for i := 0; i < 1000; i++ {
		queue.Pop()
		queue.Push(uint64(i))
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		queue.Pop()
	}
}

// BenchmarkQueueConcurrent measures throughput with multiple producer/consumer goroutines.
func BenchmarkQueueConcurrent(b *testing.B) {
	threadCounts := []int{2, 4, 8, 12}

	for _, numThreads := range threadCounts {
		b.Run(fmt.Sprintf("Threads%d", numThreads), func(b *testing.B) {
			mem, cleanup := setupQueueBenchmark(b, 100000)
			defer cleanup()

			queue, err := NewQueue[uint64](mem, "que", 100000)
			if err != nil {
				b.Fatalf("NewQueue failed: %v", err)
			}

			numProducers := numThreads / 2
			numConsumers := numThreads - numProducers

			var wg sync.WaitGroup
			stop := make(chan struct{})

			// Start producers
			for i := 0; i < numProducers; i++ {
				wg.Add(1)
				go func() {
					defer wg.Done()
					for {
						select {
						case <-stop:
							return
						default:
							for !queue.Push(42) {
								queue.Pop()
							}
						}
					}
				}()
			}

			// Start consumers
			for i := 0; i < numConsumers; i++ {
				wg.Add(1)
				go func() {
					defer wg.Done()
					for {
						select {
						case <-stop:
							return
						default:
							for _, ok := queue.Pop(); !ok; _, ok = queue.Pop() {
								queue.Push(42)
							}
						}
					}
				}()
			}

			b.ResetTimer()

			for i := 0; i < b.N; i++ {
				queue.Push(42)
			}

			b.StopTimer()
			close(stop)
			wg.Wait()
		})
	}
}
