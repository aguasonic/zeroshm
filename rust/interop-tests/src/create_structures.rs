//! Creates a shared memory segment with various structures for CLI inspection.
//!
//! This is a helper program used to test the zeroshm-cli inspection commands.
//! It creates an array, queue, stack, mutex, semaphore, barrier, latch, and
//! once flag in a shared memory segment, leaving the segment available for
//! inspection by the CLI.

use zeroshm::{Memory, Array, Queue, Stack, Mutex, Semaphore, Barrier, Latch, Once};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = "/test_cli_structures";
    let size = 64 * 1024; // 64KB

    // Clean up any existing segment
    let _ = zeroshm::platform::unlink_shm(name);

    // Create segment
    let mem = Memory::create(name, size, 64)?;

    // Create an array of 10 f32 values
    let array = Array::<f32>::new(&mem, "temperatures", 10)?;
    
    // Write some values to the array
    for i in 0..10 {
        array.set(i, i as f32 * 1.5);
    }
    drop(array);

    // Create a queue of u32 with capacity 16
    let queue = Queue::<u32>::new(&mem, "events", 16)?;
    
    // Push some values into the queue
    for i in 0..5 {
        queue.push(i)?;
    }
    drop(queue);

    // Create a stack of u64 with capacity 8
    let stack = Stack::<u64>::new(&mem, "history", 8)?;
    
    // Push some values into the stack
    for i in 0..3 {
        stack.push(i * 100)?;
    }
    drop(stack);

    // Create a mutex
    let _mutex = Mutex::new(&mem, "data_mutex")?;

    // Create a semaphore with initial count 3, max 10
    let _semaphore = Semaphore::new(&mem, "resource_sem", 3, 10)?;

    // Create a barrier with 4 participants
    let _barrier = Barrier::new(&mem, "sync_barrier", 4)?;

    // Create a latch with initial count 5
    let _latch = Latch::new(&mem, "startup_latch", 5)?;

    // Create a once flag
    let _once = Once::new(&mem, "init_once")?;

    // Call the once flag
    let once = Once::open(&mem, "init_once")?;
    once.call(|| {});
    drop(once);

    println!("Created segment {} with structures: temperatures (array), events (queue), history (stack), data_mutex (mutex), resource_sem (semaphore), sync_barrier (barrier), startup_latch (latch), init_once (once)", name);
    
    // Keep the segment alive by not unlinking
    // The segment will persist after this process exits
    drop(mem);

    Ok(())
}

