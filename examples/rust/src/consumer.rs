// Consumer opens shared memory, reads from an array, and pops from a queue.
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use zeroshm::{Array, Memory, Queue};

fn main() {
    println!("=== ZeroSHM Rust Consumer ===");

    let shm_name = "/zeroshm_example";
    let array_name = "temperatures";
    let queue_name = "events";

    // Wait for producer to create shared memory
    println!("Waiting for producer to create shared memory...");
    let mem = loop {
        match Memory::open(shm_name, 64) {
            Ok(m) => break m,
            Err(_) => sleep(Duration::from_millis(100)),
        }
    };

    // Open the temperature array
    let arr: Array<f64> = match Array::open(&mem, array_name) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to open array: {}", e);
            exit(1);
        }
    };

    // Open the event queue
    let q: Queue<i64> = match Queue::open(&mem, queue_name) {
        Ok(queue) => queue,
        Err(e) => {
            eprintln!("Failed to open queue: {}", e);
            exit(1);
        }
    };

    // Read data from queue until completion signal
    println!("Consuming events...");
    let mut count = 0usize;
    loop {
        match q.try_pop() {
            Ok(Some(ts)) => {
                // Check for completion signal
                if ts == -1 {
                    println!("Received completion signal.");
                    break;
                }

                count += 1;

                // Read the corresponding temperature from the array
                let temp = arr.get(count - 1);
                println!("  Event {}: timestamp={}, temp={}C", count, ts, temp);
            }
            Ok(None) => {
                // Queue empty, wait a bit and retry
                sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("Queue error: {}", e);
                sleep(Duration::from_millis(50));
            }
        }
    }

    println!("Consumed {} events total.", count);

    // Read all temperatures from the array and compute stats
    println!("\nReading all temperatures from array:");
    let mut sum = 0.0f64;
    for i in 0..count {
        let temp = arr.get(i);
        sum += temp;
        println!("  arr[{}] = {}C", i, temp);
    }
    if count > 0 {
        println!("Average temperature: {:.2}C", sum / count as f64);
    }

    println!("Consumer done.");
}
