// Producer creates shared memory, writes data to an array, and pushes to a queue.
package main

import (
	"fmt"
	"os"
	"time"

	"github.com/aguasonic/zeroshm/zeroshm"
)

const (
	shmName    = "/zeroshm_example"
	arrayName  = "temperatures"
	queueName  = "events"
	arraySize  = 100
	queueCap   = 32
	numReadings = 10
)

func main() {
	fmt.Println("=== ZeroSHM Go Producer ===")

	// Clean up any existing shared memory
	zeroshm.UnlinkName(shmName)

	// Create shared memory segment (1 MB)
	mem, err := zeroshm.NewMemory(shmName, 1024*1024, 64)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to create memory: %v\n", err)
		os.Exit(1)
	}
	defer mem.Close()
	defer mem.Unlink()

	// Create a typed array for temperature readings
	arr, err := zeroshm.NewArray[float64](mem, arrayName, arraySize)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to create array: %v\n", err)
		os.Exit(1)
	}

	// Create a lock-free queue for event notifications
	q, err := zeroshm.NewQueue[int64](mem, queueName, queueCap)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to create queue: %v\n", err)
		os.Exit(1)
	}

	// Simulate sensor readings: write to array and push notifications to queue
	fmt.Printf("Producing %d sensor readings...\n", numReadings)
	for i := 0; i < numReadings; i++ {
		// Generate a synthetic temperature reading
		temp := 20.0 + float64(i)*0.5
		arr.Set(i, temp)

		// Push a timestamp notification to the queue
		timestamp := time.Now().UnixNano()
		if !q.Push(timestamp) {
			fmt.Println("Queue full, waiting...")
			time.Sleep(10 * time.Millisecond)
			q.Push(timestamp)
		}
		fmt.Printf("  Reading %d: temp=%.1fC, event pushed\n", i, temp)

		// Small delay to simulate real sensor interval
		time.Sleep(100 * time.Millisecond)
	}

	// Signal completion by pushing a sentinel value (-1)
	q.Push(-1)
	fmt.Println("Sent completion signal. Consumer can now exit.")

	// Keep the process alive briefly so the consumer can read all data
	fmt.Println("Waiting for consumer to finish reading...")
	time.Sleep(2 * time.Second)

	fmt.Println("Producer done.")
}
