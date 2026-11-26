// Consumer opens shared memory, reads from an array, and pops from a queue.
package main

import (
	"fmt"
	"os"
	"time"

	"github.com/aguasonic/zeroshm/zeroshm"
)

const (
	shmName   = "/zeroshm_example"
	arrayName = "temperatures"
	queueName = "events"
)

func main() {
	fmt.Println("=== ZeroSHM Go Consumer ===")

	// Wait for producer to create shared memory
	fmt.Println("Waiting for producer to create shared memory...")
	var mem *zeroshm.Memory
	var err error
	for i := 0; i < 50; i++ {
		mem, err = zeroshm.OpenMemory(shmName, 64)
		if err == nil {
			break
		}
		time.Sleep(100 * time.Millisecond)
	}
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to open memory: %v\n", err)
		os.Exit(1)
	}
	defer mem.Close()

	// Open the temperature array
	arr, err := zeroshm.OpenArray[float64](mem, arrayName)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to open array: %v\n", err)
		os.Exit(1)
	}

	// Open the event queue
	q, err := zeroshm.OpenQueue[int64](mem, queueName)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to open queue: %v\n", err)
		os.Exit(1)
	}

	// Read data from queue until completion signal
	fmt.Println("Consuming events...")
	count := 0
	for {
		ts, ok := q.Pop()
		if !ok {
			// Queue empty, wait a bit and retry
			time.Sleep(50 * time.Millisecond)
			continue
		}

		// Check for completion signal
		if ts == -1 {
			fmt.Println("Received completion signal.")
			break
		}

		count++

		// Read the corresponding temperature from the array
		temp := arr.Get(count - 1)
		fmt.Printf("  Event %d: timestamp=%d, temp=%.1fC\n", count, ts, temp)
	}

	fmt.Printf("Consumed %d events total.\n", count)

	// Read all temperatures from the array and compute stats
	fmt.Println("\nReading all temperatures from array:")
	var sum float64
	for i := 0; i < count; i++ {
		temp := arr.Get(i)
		sum += temp
		fmt.Printf("  arr[%d] = %.1fC\n", i, temp)
	}
	if count > 0 {
		fmt.Printf("Average temperature: %.2fC\n", sum/float64(count))
	}

	fmt.Println("Consumer done.")
}
