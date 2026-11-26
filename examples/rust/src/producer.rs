// Producer creates shared memory, writes data to an array, and pushes to a queue.
use std::process::exit;
use std::thread::sleep;
use std::time::{Duration, SystemTime};

use zeroshm::platform;
use zeroshm::{Array, Memory, Queue};

fn main() {
    println!("=== ZeroSHM Rust Producer ===");

    let shm_name = "/zeroshm_example";
    let array_name = "temperatures";
    let queue_name = "events";
    let array_size = 100;
    let queue_cap = 32;
    let num_readings = 10;

    // Clean up any existing shared memory
    let _ = platform::unlink_shm(shm_name);

    // Create shared memory segment (1 MB)
    let mem = match Memory::create(shm_name, 1024 * 1024, 64) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to create memory: {}", e);
            exit(1);
        }
    };

    // Create a typed array for temperature readings
    let arr: Array<f64> = match Array::new(&mem, array_name, array_size) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to create array: {}", e);
            exit(1);
        }
    };

    // Create a lock-free queue for event notifications
    let q: Queue<i64> = match Queue::new(&mem, queue_name, queue_cap) {
        Ok(queue) => queue,
        Err(e) => {
            eprintln!("Failed to create queue: {}", e);
            exit(1);
        }
    };

    // Simulate sensor readings: write to array and push notifications to queue
    println!("Producing {} sensor readings...", num_readings);
    for i in 0..num_readings {
        // Generate a synthetic temperature reading
        let temp = 20.0 + (i as f64) * 0.5;
        arr.set(i, temp);

        // Push a timestamp notification to the queue
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64;
        if let Err(e) = q.push(ts) {
            println!("Queue full, waiting... {}", e);
            sleep(Duration::from_millis(10));
            let _ = q.push(ts);
        }
        println!("  Reading {}: temp={}C, event pushed", i, temp);

        // Small delay to simulate real sensor interval
        sleep(Duration::from_millis(100));
    }

    // Signal completion by pushing a sentinel value (-1)
    let _ = q.push(-1);
    println!("Sent completion signal. Consumer can now exit.");

    // Keep the process alive briefly so the consumer can read all data
    println!("Waiting for consumer to finish reading...");
    sleep(Duration::from_secs(2));

    println!("Producer done.");

    // Clean up shared memory (only the owner should unlink)
    let _ = mem.unlink();
}
