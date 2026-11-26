use std::time::Instant;
use std::thread;
use std::sync::Arc;
use zeroshm::{Memory, Stack};
use zeroshm::platform::unlink_shm;

const STACK_CAPACITY: usize = 100000;
const SHM_SIZE: usize = 512 * 1024 * 1024; // 512MB

fn bench_stack_push_u8(iterations: usize) {
    let name = format!("/bench_s_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let stack = Stack::new(&mem, "stk", STACK_CAPACITY).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        while !stack.try_push(0u8).unwrap() {
            let _ = stack.try_pop();
        }
    }
    let elapsed = start.elapsed();
    let throughput = (iterations as f64) / elapsed.as_secs_f64();
    println!("Stack Push U8: {throughput:.0} ops/sec");
}

fn bench_stack_push_u64(iterations: usize) {
    let name = format!("/bench_s_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let stack = Stack::new(&mem, "stk", STACK_CAPACITY).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        while !stack.try_push(0u64).unwrap() {
            let _ = stack.try_pop();
        }
    }
    let elapsed = start.elapsed();
    let throughput = (iterations as f64) / elapsed.as_secs_f64();
    println!("Stack Push U64: {throughput:.0} ops/sec");
}

fn bench_stack_pop_u64(iterations: usize) {
    let name = format!("/bench_s_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let stack = Stack::new(&mem, "stk", STACK_CAPACITY).unwrap();

    // Pre-fill
    for i in 0..STACK_CAPACITY {
        stack.try_push(i as u64).unwrap();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        while stack.try_pop().unwrap().is_none() {
            stack.try_push(0u64).unwrap();
        }
    }
    let elapsed = start.elapsed();
    let throughput = (iterations as f64) / elapsed.as_secs_f64();
    println!("Stack Pop U64: {throughput:.0} ops/sec");
}

fn bench_stack_concurrent(num_threads: usize, iterations: usize) {
    let name = format!("/bench_s_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let stack = Arc::new(Stack::new(&mem, "conc", STACK_CAPACITY).unwrap());

    let mut handles = vec![];
    for _ in 0..num_threads {
        let stack = Arc::clone(&stack);
        handles.push(thread::spawn(move || {
            for _ in 0..iterations {
                while !stack.try_push(42u64).unwrap() {
                    let _ = stack.try_pop();
                }
                while stack.try_pop().unwrap().is_none() {
                    stack.try_push(42u64).unwrap();
                }
            }
        }));
    }

    let start = Instant::now();
    for handle in handles {
        handle.join().unwrap();
    }
    let elapsed = start.elapsed();
    let throughput = (num_threads * iterations * 2) as f64 / elapsed.as_secs_f64();
    println!("Stack Concurrent ({num_threads} threads): {throughput:.0} ops/sec");
}

fn main() {
    println!("=== ZeroSHM Stack Performance Benchmarks (Rust) ===");

    println!("\n--- Stack Single-Thread Throughput ---");
    bench_stack_push_u8(1_000_000);
    bench_stack_push_u64(1_000_000);

    println!("\n--- Stack Pop Throughput ---");
    bench_stack_pop_u64(1_000_000);

    println!("\n--- Stack Concurrent Throughput ---");
    for threads in [2, 4, 8, 12] {
        bench_stack_concurrent(threads, 100_000);
    }
}
