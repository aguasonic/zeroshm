use std::time::Instant;
use std::thread;
use std::sync::Arc;
use zeroshm::{Memory, Queue};
use zeroshm::platform::unlink_shm;

const QUEUE_CAPACITY: usize = 100000;
const SHM_SIZE: usize = 512 * 1024 * 1024; // 512MB

fn bench_queue_push_u8(iterations: usize) {
    let name = format!("/bench_q_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let queue = Queue::new(&mem, "que", QUEUE_CAPACITY).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        while queue.try_push(0u8).is_err() {
            let _ = queue.try_pop();
        }
    }
    let elapsed = start.elapsed();
    let throughput = (iterations as f64) / elapsed.as_secs_f64();
    println!("Queue Push U8: {throughput:.0} ops/sec");
}

fn bench_queue_push_u64(iterations: usize) {
    let name = format!("/bench_q_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let queue = Queue::new(&mem, "que", QUEUE_CAPACITY).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        while queue.try_push(0u64).is_err() {
            let _ = queue.try_pop();
        }
    }
    let elapsed = start.elapsed();
    let throughput = (iterations as f64) / elapsed.as_secs_f64();
    println!("Queue Push U64: {throughput:.0} ops/sec");
}

fn bench_queue_pop_u64(iterations: usize) {
    let name = format!("/bench_q_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let queue = Queue::new(&mem, "que", QUEUE_CAPACITY).unwrap();

    // Pre-fill
    for i in 0..QUEUE_CAPACITY {
        queue.push(i as u64).unwrap();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        while queue.try_pop().unwrap().is_none() {
            queue.push(0u64).unwrap();
        }
    }
    let elapsed = start.elapsed();
    let throughput = (iterations as f64) / elapsed.as_secs_f64();
    println!("Queue Pop U64: {throughput:.0} ops/sec");
}

fn bench_queue_latency(iterations: usize) {
    let name = format!("/bench_q_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let queue = Queue::new(&mem, "lat", QUEUE_CAPACITY).unwrap();

    // Warmup
    for i in 0..1000 {
        queue.try_push(i as u64).unwrap();
        let _ = queue.try_pop();
    }

    let start = Instant::now();
    for i in 0..iterations {
        queue.try_push(i as u64).unwrap();
    }
    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("Queue Push Latency: {avg_ns:.0} ns/op");
}

fn bench_queue_pop_latency(iterations: usize) {
    let name = format!("/bench_q_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let queue = Queue::new(&mem, "lat", QUEUE_CAPACITY).unwrap();

    // Pre-fill
    for i in 0..QUEUE_CAPACITY {
        queue.push(i as u64).unwrap();
    }

    // Warmup
    for i in 0..1000 {
        let _ = queue.try_pop();
        queue.try_push(i as u64).unwrap();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = queue.try_pop();
    }
    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("Queue Pop Latency: {avg_ns:.0} ns/op");
}

fn bench_queue_concurrent(num_threads: usize, iterations: usize) {
    let name = format!("/bench_q_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let queue = Arc::new(Queue::new(&mem, "conc", QUEUE_CAPACITY).unwrap());

    let mut handles = vec![];
    for _ in 0..num_threads {
        let queue = Arc::clone(&queue);
        handles.push(thread::spawn(move || {
            for _ in 0..iterations {
                while queue.try_push(42u64).is_err() {
                    let _ = queue.try_pop();
                }
            }
        }));
    }

    let start = Instant::now();
    for handle in handles {
        handle.join().unwrap();
    }
    let elapsed = start.elapsed();
    let throughput = (num_threads * iterations) as f64 / elapsed.as_secs_f64();
    println!("Queue Concurrent ({num_threads} threads): {throughput:.0} ops/sec");
}

fn main() {
    println!("=== ZeroSHM Queue Performance Benchmarks (Rust) ===");

    println!("\n--- Queue Single-Thread Throughput ---");
    bench_queue_push_u8(1_000_000);
    bench_queue_push_u64(1_000_000);

    println!("\n--- Queue Pop Throughput ---");
    bench_queue_pop_u64(1_000_000);

    println!("\n--- Queue Operation Latency ---");
    bench_queue_latency(100_000);
    bench_queue_pop_latency(100_000);

    println!("\n--- Queue Concurrent Throughput ---");
    for threads in [2, 4, 8, 12] {
        bench_queue_concurrent(threads, 100_000);
    }
}
