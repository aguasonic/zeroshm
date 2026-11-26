use std::time::Instant;
use zeroshm::Memory;
use zeroshm::array::Array;
use zeroshm::platform::unlink_shm;

const SHM_SIZE: usize = 100 * 1024 * 1024; // 100MB

fn bench_array_sequential_read(size: usize, iterations: usize) {
    let name = format!("/bench_a_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let array = Array::new(&mem, "seq", size).unwrap();

    // Initialize
    for i in 0..size {
        array.set(i, i as u64);
    }

    let start = Instant::now();
    let mut sum: u64 = 0;
    for _ in 0..iterations {
        for i in 0..size {
            sum += array.get(i);
        }
    }
    let elapsed = start.elapsed();
    let bytes = (size * 8 * iterations) as f64;
    let bandwidth = bytes / elapsed.as_secs_f64() / (1024.0 * 1024.0 * 1024.0);
    println!("Array Seq Read (size={size}): {bandwidth:.1} GB/s");
    let _ = sum;
}

fn bench_array_sequential_write(size: usize, iterations: usize) {
    let name = format!("/bench_a_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let array = Array::new(&mem, "seq", size).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        for i in 0..size {
            array.set(i, i as u64);
        }
    }
    let elapsed = start.elapsed();
    let bytes = (size * 8 * iterations) as f64;
    let bandwidth = bytes / elapsed.as_secs_f64() / (1024.0 * 1024.0 * 1024.0);
    println!("Array Seq Write (size={size}): {bandwidth:.1} GB/s");
}

fn bench_array_random_read(iterations: usize) {
    let name = format!("/bench_a_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let size = 1_000_000;
    let array = Array::new(&mem, "random", size).unwrap();

    // Initialize
    for i in 0..size {
        array.set(i, i as u64);
    }

    // Generate random indices (deterministic)
    let mut indices = Vec::with_capacity(100_000);
    let mut state: u64 = 42;
    for _ in 0..100_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        indices.push((state >> 33) as usize % size);
    }

    let start = Instant::now();
    let mut sum: u64 = 0;
    for _ in 0..iterations {
        for &idx in &indices {
            sum += array.get(idx);
        }
    }
    let elapsed = start.elapsed();
    let bytes = (indices.len() * 8 * iterations) as f64;
    let bandwidth = bytes / elapsed.as_secs_f64() / (1024.0 * 1024.0 * 1024.0);
    println!("Array Random Read: {bandwidth:.1} GB/s");
    let _ = sum;
}

fn bench_array_random_write(iterations: usize) {
    let name = format!("/bench_a_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let size = 1_000_000;
    let array = Array::new(&mem, "random", size).unwrap();

    // Initialize
    for i in 0..size {
        array.set(i, i as u64);
    }

    // Generate random indices (deterministic)
    let mut indices = Vec::with_capacity(100_000);
    let mut state: u64 = 42;
    for _ in 0..100_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        indices.push((state >> 33) as usize % size);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        for &idx in &indices {
            array.set(idx, (idx * 2) as u64);
        }
    }
    let elapsed = start.elapsed();
    let bytes = (indices.len() * 8 * iterations) as f64;
    let bandwidth = bytes / elapsed.as_secs_f64() / (1024.0 * 1024.0 * 1024.0);
    println!("Array Random Write: {bandwidth:.1} GB/s");
}

fn bench_array_stride(stride: usize, iterations: usize) {
    let name = format!("/bench_a_{}", std::process::id());
    let _ = unlink_shm(&name);
    let mem = Memory::create(&name, SHM_SIZE, 64).unwrap();
    let size = 1_000_000;
    let array = Array::new(&mem, "stride", size).unwrap();

    // Initialize
    for i in 0..size {
        array.set(i, i as u64);
    }

    let start = Instant::now();
    let mut sum: u64 = 0;
    for _ in 0..iterations {
        for i in (0..size).step_by(stride) {
            sum += array.get(i);
        }
    }
    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("Array Stride {stride}: {avg_ns:.0} ns/pass");
    let _ = sum;
}

fn main() {
    println!("=== ZeroSHM Array Performance Benchmarks (Rust) ===");

    println!("\n--- Array Sequential Access ---");
    for size in [1000, 10_000, 100_000, 1_000_000] {
        bench_array_sequential_read(size, 100);
        bench_array_sequential_write(size, 100);
    }

    println!("\n--- Array Random Access ---");
    bench_array_random_read(100);
    bench_array_random_write(100);

    println!("\n--- Array Stride Access ---");
    for stride in [1, 8, 64] {
        bench_array_stride(stride, 10_000);
    }
}
