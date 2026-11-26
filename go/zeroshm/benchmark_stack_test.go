package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
)

func setupStackBenchmark(b *testing.B, capacity int) (*Memory, func()) {
	b.Helper()
	name := fmt.Sprintf("/bench_s_%d", os.Getpid())
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

// BenchmarkStackPush measures single-thread push throughput.
func BenchmarkStackPush(b *testing.B) {
	cases := []struct {
		name     string
		capacity int
		bench    func(*testing.B, *Memory, int)
	}{
		{"U8", 100000, benchStackPushU8},
		{"U32", 100000, benchStackPushU32},
		{"U64", 100000, benchStackPushU64},
	}

	for _, tc := range cases {
		b.Run(tc.name, func(b *testing.B) {
			mem, cleanup := setupStackBenchmark(b, tc.capacity)
			defer cleanup()

			b.ResetTimer()
			tc.bench(b, mem, tc.capacity)
		})
	}
}

// BenchmarkStackPop measures single-thread pop throughput.
func BenchmarkStackPop(b *testing.B) {
	cases := []struct {
		name     string
		capacity int
		bench    func(*testing.B, *Memory, int)
	}{
		{"U8", 100000, benchStackPopU8},
		{"U32", 100000, benchStackPopU32},
		{"U64", 100000, benchStackPopU64},
	}

	for _, tc := range cases {
		b.Run(tc.name, func(b *testing.B) {
			mem, cleanup := setupStackBenchmark(b, tc.capacity)
			defer cleanup()

			b.ResetTimer()
			tc.bench(b, mem, tc.capacity)
		})
	}
}

func benchStackPushU8(b *testing.B, mem *Memory, capacity int) {
	stack, err := NewStack[uint8](mem, "stk", capacity)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}
	for i := 0; i < b.N; i++ {
		for !stack.Push(0) {
			stack.Pop()
		}
	}
}

func benchStackPushU32(b *testing.B, mem *Memory, capacity int) {
	stack, err := NewStack[uint32](mem, "stk", capacity)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}
	for i := 0; i < b.N; i++ {
		for !stack.Push(0) {
			stack.Pop()
		}
	}
}

func benchStackPushU64(b *testing.B, mem *Memory, capacity int) {
	stack, err := NewStack[uint64](mem, "stk", capacity)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}
	for i := 0; i < b.N; i++ {
		for !stack.Push(0) {
			stack.Pop()
		}
	}
}

func benchStackPopU8(b *testing.B, mem *Memory, capacity int) {
	stack, err := NewStack[uint8](mem, "stk", capacity)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}
	// Pre-fill the stack
	for i := 0; i < capacity; i++ {
		stack.Push(uint8(i))
	}
	for i := 0; i < b.N; i++ {
		for _, ok := stack.Pop(); !ok; _, ok = stack.Pop() {
			stack.Push(0)
		}
	}
}

func benchStackPopU32(b *testing.B, mem *Memory, capacity int) {
	stack, err := NewStack[uint32](mem, "stk", capacity)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}
	// Pre-fill the stack
	for i := 0; i < capacity; i++ {
		stack.Push(uint32(i))
	}
	for i := 0; i < b.N; i++ {
		for _, ok := stack.Pop(); !ok; _, ok = stack.Pop() {
			stack.Push(0)
		}
	}
}

func benchStackPopU64(b *testing.B, mem *Memory, capacity int) {
	stack, err := NewStack[uint64](mem, "stk", capacity)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}
	// Pre-fill the stack
	for i := 0; i < capacity; i++ {
		stack.Push(uint64(i))
	}
	for i := 0; i < b.N; i++ {
		for _, ok := stack.Pop(); !ok; _, ok = stack.Pop() {
			stack.Push(0)
		}
	}
}

// BenchmarkStackTop measures per-top latency.
func BenchmarkStackTop(b *testing.B) {
	mem, cleanup := setupStackBenchmark(b, 100000)
	defer cleanup()

	stack, err := NewStack[uint64](mem, "top", 100000)
	if err != nil {
		b.Fatalf("NewStack failed: %v", err)
	}

	// Pre-fill
	for i := 0; i < 1000; i++ {
		stack.Push(uint64(i))
	}

	// Warmup
	for i := 0; i < 1000; i++ {
		stack.Top()
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		stack.Top()
	}
}

// BenchmarkStackConcurrent measures concurrent push/pop throughput.
func BenchmarkStackConcurrent(b *testing.B) {
	threadCounts := []int{2, 4, 8, 12}

	for _, numThreads := range threadCounts {
		b.Run(fmt.Sprintf("Threads%d", numThreads), func(b *testing.B) {
			mem, cleanup := setupStackBenchmark(b, 100000)
			defer cleanup()

			stack, err := NewStack[uint64](mem, "conc", 100000)
			if err != nil {
				b.Fatalf("NewStack failed: %v", err)
			}

			var wg sync.WaitGroup
			stop := make(chan struct{})

			// All threads do push/pop alternately
			for i := 0; i < numThreads; i++ {
				wg.Add(1)
				go func() {
					defer wg.Done()
					for {
						select {
						case <-stop:
							return
						default:
							for !stack.Push(42) {
								stack.Pop()
							}
							for _, ok := stack.Pop(); !ok; _, ok = stack.Pop() {
								stack.Push(42)
							}
						}
					}
				}()
			}

			b.ResetTimer()

			for i := 0; i < b.N; i++ {
				stack.Push(42)
				stack.Pop()
			}

			b.StopTimer()
			close(stop)
			wg.Wait()
		})
	}
}
