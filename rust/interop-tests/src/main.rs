// ZeroSHM Cross-Language Interop Tests (Part 1)
// Rust <-> Go and Rust <-> C++ interop verification.
//
// Usage:
//   cargo run -p interop-tests --bin interop -- <command> <shm_name> [args...]
//
// Commands:
//   array_create <shm> <name> <capacity> <values...>  - Create array, write values
//   array_read <shm> <name>                            - Read array, print values
//   array_write <shm> <name> <values...>               - Write values to existing array
//   queue_create <shm> <name> <capacity>               - Create queue
//   queue_push <shm> <name> <values...>                - Push values to queue
//   queue_pop <shm> <name> <count>                     - Pop N values from queue
//   mutex_create <shm> <name>                          - Create mutex
//   mutex_lock <shm> <name>                            - Lock mutex
//   mutex_unlock <shm> <name>                          - Unlock mutex
//   barrier_create <shm> <name> <participants>         - Create barrier
//   barrier_wait <shm> <name>                          - Wait at barrier
//   signal_create <shm> <name>                         - Create signal
//   signal_set <shm> <name> <value>                    - Set signal value
//   signal_get <shm> <name>                            - Get signal value
//   latest_create <shm> <name> <initial_value>         - Create Latest with initial value
//   latest_get <shm> <name>                            - Read current value and version
//   latest_set <shm> <name> <value>                    - Set new value
//   latest_wait <shm> <name> <last_version> <timeout_ms> - Wait for change
//   cleanup <shm>                                      - Unlink shared memory

use zeroshm::{Array, Barrier, Latest, Memory, Mutex, Queue, Signal};
use zeroshm::platform;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: interop <command> <shm_name> [args...]");
        process::exit(1);
    }

    let command = args[1].as_str();
    let shm_name = &args[2];

    let result = match command {
        "array_create" => cmd_array_create(shm_name, &args[3..]),
        "array_read" => cmd_array_read(shm_name, &args[3..]),
        "array_write" => cmd_array_write(shm_name, &args[3..]),
        "queue_create" => cmd_queue_create(shm_name, &args[3..]),
        "queue_push" => cmd_queue_push(shm_name, &args[3..]),
        "queue_pop" => cmd_queue_pop(shm_name, &args[3..]),
        "mutex_create" => cmd_mutex_create(shm_name, &args[3..]),
        "mutex_lock" => cmd_mutex_lock(shm_name, &args[3..]),
        "mutex_unlock" => cmd_mutex_unlock(shm_name, &args[3..]),
        "barrier_create" => cmd_barrier_create(shm_name, &args[3..]),
        "barrier_wait" => cmd_barrier_wait(shm_name, &args[3..]),
        "signal_create" => cmd_signal_create(shm_name, &args[3..]),
        "signal_set" => cmd_signal_set(shm_name, &args[3..]),
        "signal_get" => cmd_signal_get(shm_name, &args[3..]),
        "latest_create" => cmd_latest_create(shm_name, &args[3..]),
        "latest_get" => cmd_latest_get(shm_name, &args[3..]),
        "latest_set" => cmd_latest_set(shm_name, &args[3..]),
        "latest_wait" => cmd_latest_wait(shm_name, &args[3..]),
        "cleanup" => cmd_cleanup(shm_name),
        _ => {
            eprintln!("Unknown command: {}", command);
            process::exit(1);
        }
    };

    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("ERROR: {}", e);
            process::exit(1);
        }
    }
}

fn ensure_name(name: &str) -> String {
    if name.starts_with('/') {
        name.to_string()
    } else {
        format!("/{}", name)
    }
}

fn cmd_array_create(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 3 {
        return Err("Usage: array_create <shm> <name> <capacity> <values...>".into());
    }

    let name = ensure_name(&args[0]);
    let capacity: usize = args[1].parse().map_err(|e| format!("Invalid capacity: {}", e))?;
    let values: Vec<f64> = args[2..]
        .iter()
        .map(|v| v.parse().map_err(|e| format!("Invalid value '{}': {}", v, e)))
        .collect::<Result<Vec<_>, _>>()?;

    let shm = ensure_name(shm_name);
    let _ = platform::unlink_shm(&shm);
    let memory = Memory::create(&shm, 1024 * 1024, 64)
        .map_err(|e| format!("Failed to create memory: {}", e))?;

    let array: Array<f64> = Array::new(&memory, &name, capacity)
        .map_err(|e| format!("Failed to create array: {}", e))?;

    for (i, &v) in values.iter().enumerate() {
        array.set(i, v);
    }

    println!("Rust: Created array '{}' with {} values", name, values.len());
    for (i, &v) in values.iter().enumerate() {
        println!("Rust: array[{}] = {}", i, v);
    }

    // Don't unlink - the other language needs to read it
    // memory.unlink().map_err(|e| format!("Failed to unlink: {}", e))?;
    drop(memory);
    Ok(())
}

fn cmd_array_read(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: array_read <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let array: Array<f64> = Array::open(&memory, &name)
        .map_err(|e| format!("Failed to open array: {}", e))?;

    println!("Rust: Opened array '{}', capacity={}", name, array.capacity());
    for i in 0..array.capacity() {
        let v = array.get(i);
        println!("Rust: array[{}] = {}", i, v);
    }

    drop(memory);
    Ok(())
}

fn cmd_array_write(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: array_write <shm> <name> <values...>".into());
    }

    let name = ensure_name(&args[0]);
    let values: Vec<f64> = args[1..]
        .iter()
        .map(|v| v.parse().map_err(|e| format!("Invalid value '{}': {}", v, e)))
        .collect::<Result<Vec<_>, _>>()?;

    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let array: Array<f64> = Array::open(&memory, &name)
        .map_err(|e| format!("Failed to open array: {}", e))?;

    for (i, &v) in values.iter().enumerate() {
        array.set(i, v);
    }

    println!("Rust: Wrote {} values to array '{}'", values.len(), name);
    drop(memory);
    Ok(())
}

fn cmd_queue_create(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: queue_create <shm> <name> <capacity>".into());
    }

    let name = ensure_name(&args[0]);
    let capacity: usize = args[1].parse().map_err(|e| format!("Invalid capacity: {}", e))?;

    let shm = ensure_name(shm_name);
    let _ = platform::unlink_shm(&shm);
    let memory = Memory::create(&shm, 1024 * 1024, 64)
        .map_err(|e| format!("Failed to create memory: {}", e))?;

    let queue: Queue<u32> = Queue::new(&memory, &name, capacity)
        .map_err(|e| format!("Failed to create queue: {}", e))?;

    println!("Rust: Created queue '{}', capacity={}", name, queue.capacity());
    drop(queue);
    // Don't unlink - the other language needs to access it
    drop(memory);
    Ok(())
}

fn cmd_queue_push(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: queue_push <shm> <name> <values...>".into());
    }

    let name = ensure_name(&args[0]);
    let values: Vec<u32> = args[1..]
        .iter()
        .map(|v| v.parse().map_err(|e| format!("Invalid value '{}': {}", v, e)))
        .collect::<Result<Vec<_>, _>>()?;

    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let queue: Queue<u32> = Queue::open(&memory, &name)
        .map_err(|e| format!("Failed to open queue: {}", e))?;

    for &v in &values {
        queue.push(v).map_err(|e| format!("Failed to push {}: {}", v, e))?;
    }

    println!("Rust: Pushed {} values to queue '{}'", values.len(), name);
    drop(queue);
    drop(memory);
    Ok(())
}

fn cmd_queue_pop(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: queue_pop <shm> <name> <count>".into());
    }

    let name = ensure_name(&args[0]);
    let count: usize = args[1].parse().map_err(|e| format!("Invalid count: {}", e))?;

    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let queue: Queue<u32> = Queue::open(&memory, &name)
        .map_err(|e| format!("Failed to open queue: {}", e))?;

    let mut all_ok = true;
    for _ in 0..count {
        match queue.pop() {
            Ok(v) => println!("Rust: Popped {} from queue '{}'", v, name),
            Err(_) => {
                println!("Rust: Queue empty");
                all_ok = false;
                break;
            }
        }
    }

    drop(queue);
    drop(memory);

    if !all_ok {
        return Err("Failed to pop all values".into());
    }
    Ok(())
}

fn cmd_mutex_create(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: mutex_create <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let _ = platform::unlink_shm(&shm);
    let memory = Memory::create(&shm, 1024 * 1024, 64)
        .map_err(|e| format!("Failed to create memory: {}", e))?;

    let mutex = Mutex::new(&memory, &name)
        .map_err(|e| format!("Failed to create mutex: {}", e))?;

    println!("Rust: Created mutex '{}'", name);
    drop(mutex);
    // Don't unlink - the other language needs to access it
    drop(memory);
    Ok(())
}

fn cmd_mutex_lock(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: mutex_lock <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let mutex = Mutex::open(&memory, &name)
        .map_err(|e| format!("Failed to open mutex: {}", e))?;

    let _guard = mutex.lock().map_err(|e| format!("Failed to lock: {}", e))?;
    println!("Rust: Locked mutex '{}'", name);

    // Hold the lock for a moment so another process can detect it's locked
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Guard dropped here, mutex unlocked
    drop(_guard);
    drop(mutex);
    drop(memory);
    Ok(())
}

fn cmd_mutex_unlock(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: mutex_unlock <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let mutex = Mutex::open(&memory, &name)
        .map_err(|e| format!("Failed to open mutex: {}", e))?;

    // Try to lock and unlock (verifies cross-language unlock worked)
    let _guard = mutex.lock().map_err(|e| format!("Failed to lock: {}", e))?;
    println!("Rust: Acquired mutex '{}' (was unlocked by other language)", name);
    drop(_guard);
    drop(mutex);
    drop(memory);
    Ok(())
}

fn cmd_barrier_create(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: barrier_create <shm> <name> <participants>".into());
    }

    let name = ensure_name(&args[0]);
    let participants: i32 = args[1].parse().map_err(|e| format!("Invalid participants: {}", e))?;

    let shm = ensure_name(shm_name);
    let _ = platform::unlink_shm(&shm);
    let memory = Memory::create(&shm, 1024 * 1024, 64)
        .map_err(|e| format!("Failed to create memory: {}", e))?;

    let barrier = Barrier::new(&memory, &name, participants)
        .map_err(|e| format!("Failed to create barrier: {}", e))?;

    println!("Rust: Created barrier '{}' with {} participants", name, participants);
    drop(barrier);
    // Don't unlink - the other language needs to access it
    drop(memory);
    Ok(())
}

fn cmd_barrier_wait(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: barrier_wait <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let barrier = Barrier::open(&memory, &name)
        .map_err(|e| format!("Failed to open barrier: {}", e))?;

    println!("Rust: Waiting at barrier '{}'...", name);
    barrier.wait().map_err(|e| format!("Barrier wait failed: {}", e))?;
    println!("Rust: Barrier '{}' released", name);

    drop(barrier);
    drop(memory);
    Ok(())
}

fn cmd_signal_create(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: signal_create <shm> <name> <initial_value>".into());
    }

    let name = ensure_name(&args[0]);
    let initial: u64 = args[1].parse().map_err(|e| format!("Invalid initial value: {}", e))?;

    let shm = ensure_name(shm_name);
    let _ = platform::unlink_shm(&shm);
    let memory = Memory::create(&shm, 1024 * 1024, 64)
        .map_err(|e| format!("Failed to create memory: {}", e))?;

    let signal: Signal<u64> = Signal::new(&memory, &name, initial)
        .map_err(|e| format!("Failed to create signal: {}", e))?;

    println!("Rust: Created signal '{}' with initial value {}", name, initial);
    drop(signal);
    // Don't unlink - the other language needs to access it
    drop(memory);
    Ok(())
}

fn cmd_signal_set(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: signal_set <shm> <name> <value>".into());
    }

    let name = ensure_name(&args[0]);
    let value: u64 = args[1].parse().map_err(|e| format!("Invalid value: {}", e))?;

    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let signal: Signal<u64> = Signal::open(&memory, &name)
        .map_err(|e| format!("Failed to open signal: {}", e))?;

    signal.set(value).map_err(|e| format!("Failed to set signal: {}", e))?;
    println!("Rust: Set signal '{}' to value {}", name, value);

    drop(signal);
    drop(memory);
    Ok(())
}

fn cmd_signal_get(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: signal_get <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let signal: Signal<u64> = Signal::open(&memory, &name)
        .map_err(|e| format!("Failed to open signal: {}", e))?;

    let value = signal.get().map_err(|e| format!("Failed to get signal: {}", e))?;
    let version = signal.version();
    println!("Rust: Signal '{}' value = {}, version = {}", name, value, version);

    drop(signal);
    drop(memory);
    Ok(())
}

fn cmd_latest_create(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: latest_create <shm> <name> <initial_value>".into());
    }

    let name = ensure_name(&args[0]);
    let initial_value: u64 = args[1].parse().map_err(|e| format!("Invalid value: {}", e))?;

    let shm = ensure_name(shm_name);
    let _ = platform::unlink_shm(&shm);
    let memory = Memory::create(&shm, 1024 * 1024, 64)
        .map_err(|e| format!("Failed to create memory: {}", e))?;

    let latest = Latest::<u64>::new(&memory, &name, initial_value)
        .map_err(|e| format!("Failed to create Latest: {}", e))?;

    println!("Rust: Created Latest '{}' with value {}", name, initial_value);
    drop(latest);
    // Don't unlink - the other language needs to read it
    drop(memory);
    Ok(())
}

fn cmd_latest_get(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 1 {
        return Err("Usage: latest_get <shm> <name>".into());
    }

    let name = ensure_name(&args[0]);
    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let latest = Latest::<u64>::open(&memory, &name)
        .map_err(|e| format!("Failed to open Latest: {}", e))?;

    let (value, version) = latest.get();
    println!("Rust: Latest '{}' value = {}, version = {}", name, value, version);

    drop(latest);
    drop(memory);
    Ok(())
}

fn cmd_latest_set(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: latest_set <shm> <name> <value>".into());
    }

    let name = ensure_name(&args[0]);
    let value: u64 = args[1].parse().map_err(|e| format!("Invalid value: {}", e))?;

    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let latest = Latest::<u64>::open(&memory, &name)
        .map_err(|e| format!("Failed to open Latest: {}", e))?;

    latest.set(value);
    println!("Rust: Set Latest '{}' to {}", name, value);

    drop(latest);
    drop(memory);
    Ok(())
}

fn cmd_latest_wait(shm_name: &str, args: &[String]) -> Result<(), String> {
    if args.len() < 3 {
        return Err("Usage: latest_wait <shm> <name> <last_version> <timeout_ms>".into());
    }

    let name = ensure_name(&args[0]);
    let last_version: u64 = args[1].parse().map_err(|e| format!("Invalid version: {}", e))?;
    let timeout_ms: u64 = args[2].parse().map_err(|e| format!("Invalid timeout: {}", e))?;

    let shm = ensure_name(shm_name);
    let memory = Memory::open(&shm, 64)
        .map_err(|e| format!("Failed to open memory: {}", e))?;

    let latest = Latest::<u64>::open(&memory, &name)
        .map_err(|e| format!("Failed to open Latest: {}", e))?;

    let timeout = std::time::Duration::from_millis(timeout_ms);
    let (value, version, changed) = latest.wait_for_change(last_version, timeout);

    if changed {
        println!("Rust: Latest '{}' changed: value = {}, version = {}", name, value, version);
    } else {
        println!("Rust: Latest '{}' timeout: value = {}, version = {}", name, value, version);
    }

    drop(latest);
    drop(memory);
    Ok(())
}

fn cmd_cleanup(shm_name: &str) -> Result<(), String> {
    let shm = ensure_name(shm_name);
    platform::unlink_shm(&shm).map_err(|e| format!("Failed to unlink: {}", e))?;
    println!("Rust: Cleaned up {}", shm);
    Ok(())
}

