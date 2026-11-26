//! CSP-style channel for message passing between processes.
//!
//! Provides the `Channel<T>` type implementing bounded and unbounded channels
//! with backpressure. Built on Queue + Semaphore primitives.
//!
//! # Binary Layout
//! ```text
//! [queue: Queue layout][capacity_sem: 16 bytes][data_sem: 16 bytes]
//! ```
//!
//! # Usage
//! - `send(value)`: send a message (blocks if channel is full)
//! - `recv()`: receive a message (blocks if channel is empty)
//! - `try_send(value)`: non-blocking send
//! - `try_recv()`: non-blocking receive
//! - Bounded channels provide backpressure; unbounded channels never block on send
use crate::array::Pod;
use crate::error::{Result, ZeroShmError};
use crate::memory::Memory;
use crate::queue::Queue;
use crate::sync::Semaphore;
use std::marker::PhantomData;
use std::sync::Arc;

/// A CSP-style channel for message passing in shared memory.
///
/// Composed of:
/// - `Queue<T>`: stores the messages
/// - `capacity_sem`: Semaphore with initial count = capacity (limits how many can be sent)
/// - `data_sem`: Semaphore with initial count = 0 (signals available data)
///
/// For unbounded channels (capacity=0), the capacity semaphore is unbounded.
pub struct Channel<T: Pod> {
    memory: Arc<Memory>,
    name: String,
    queue: Queue<T>,
    capacity_sem: Semaphore,
    data_sem: Semaphore,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Channel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("name", &self.name())
            .field("len", &self.len())
            .finish()
    }
}

// SAFETY: Channel holds Queue and Semaphores which are Send + Sync.
unsafe impl<T: Pod> Send for Channel<T> {}
unsafe impl<T: Pod> Sync for Channel<T> {}

impl<T: Pod> Channel<T> {
    /// Create a new bounded Channel in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Base name (creates `{name}`, `{name}_cap`, `{name}_data`)
    /// * `capacity` -- Maximum number of buffered messages (0 = unbounded)
    pub fn new(memory: &Memory, name: &str, capacity: usize) -> Result<Self> {
        // Check if already exists
        if memory.find(name)?.is_some() {
            return Err(ZeroShmError::AlreadyExists(name.to_string()));
        }

        // Create the queue
        // For bounded channels, queue capacity = channel capacity
        // For unbounded channels, we use a large queue capacity
        let queue_capacity = if capacity == 0 { 1024 } else { capacity };
        let queue = Queue::new(memory, name, queue_capacity)?;

        // Capacity semaphore: starts at capacity (how many can be sent)
        // For unbounded, max_count=0 (unbounded)
        let cap_name = format!("{}_cap", name);
        let capacity_sem = Semaphore::new(
            memory,
            &cap_name,
            capacity as i32,
            if capacity == 0 { 0 } else { capacity as i32 },
        )?;

        // Data semaphore: starts at 0 (no data available), unbounded
        let data_name = format!("{}_data", name);
        let data_sem = Semaphore::new(memory, &data_name, 0, 0)?;

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            queue,
            capacity_sem,
            data_sem,
            _marker: PhantomData,
        })
    }

    /// Open an existing Channel from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Base name of the Channel
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let (_offset, _size) = match memory.find(name)? {
            Some(entry) => entry,
            None => {
                return Err(ZeroShmError::NotFoundError(format!(
                    "channel '{}' not found",
                    name
                )))
            }
        };

        let queue = Queue::open(memory, name)?;

        let cap_name = format!("{}_cap", name);
        let capacity_sem = Semaphore::open(memory, &cap_name)?;

        let data_name = format!("{}_data", name);
        let data_sem = Semaphore::open(memory, &data_name)?;

        Ok(Self {
            memory: Arc::new(memory.clone_shallow()),
            name: name.to_string(),
            queue,
            capacity_sem,
            data_sem,
            _marker: PhantomData,
        })
    }

    /// Send a message to the channel (blocking).
    ///
    /// Blocks if the channel is full (bounded) until space becomes available.
    pub fn send(&self, value: T) -> Result<()> {
        // Acquire capacity permit (blocks if full)
        self.capacity_sem.acquire()?;
        // Push to queue (should succeed since we have capacity)
        self.queue.push(value)?;
        // Signal data availability
        self.data_sem.release()?;
        Ok(())
    }

    /// Try to send a message without blocking.
    ///
    /// Returns `Ok(true)` if sent, `Ok(false)` if channel is full.
    pub fn try_send(&self, value: T) -> Result<bool> {
        if !self.capacity_sem.try_acquire()? {
            return Ok(false);
        }
        // We have capacity, try to push
        match self.queue.try_push(value) {
            Ok(()) => {
                // Signal data availability
                self.data_sem.release()?;
                Ok(true)
            }
            Err(_) => {
                // Queue push failed, return the capacity permit
                self.capacity_sem.release()?;
                Ok(false)
            }
        }
    }

    /// Try to send a message with a timeout.
    ///
    /// Returns `Ok(true)` if sent, `Ok(false)` if timeout elapsed.
    pub fn send_timeout(&self, value: T, timeout: std::time::Duration) -> Result<bool> {
        if !self.capacity_sem.try_acquire_timeout(timeout)? {
            return Ok(false);
        }
        match self.queue.try_push(value) {
            Ok(()) => {
                self.data_sem.release()?;
                Ok(true)
            }
            Err(_) => {
                self.capacity_sem.release()?;
                Ok(false)
            }
        }
    }

    /// Receive a message from the channel (blocking).
    ///
    /// Blocks if the channel is empty until a message becomes available.
    pub fn recv(&self) -> Result<T> {
        // Wait for data
        self.data_sem.acquire()?;
        // Pop from queue
        let value = self.queue.pop()?;
        // Release capacity permit
        self.capacity_sem.release()?;
        Ok(value)
    }

    /// Try to receive a message without blocking.
    ///
    /// Returns `Some(value)` if received, `None` if channel is empty.
    pub fn try_recv(&self) -> Result<Option<T>> {
        if !self.data_sem.try_acquire()? {
            return Ok(None);
        }
        match self.queue.try_pop()? {
            Some(value) => {
                self.capacity_sem.release()?;
                Ok(Some(value))
            }
            None => {
                // Data sem was acquired but queue is empty (race)
                self.data_sem.release()?;
                Ok(None)
            }
        }
    }

    /// Try to receive a message with a timeout.
    ///
    /// Returns `Some(value)` if received, `None` on timeout.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Result<Option<T>> {
        if !self.data_sem.try_acquire_timeout(timeout)? {
            return Ok(None);
        }
        match self.queue.try_pop()? {
            Some(value) => {
                self.capacity_sem.release()?;
                Ok(Some(value))
            }
            None => {
                self.data_sem.release()?;
                Ok(None)
            }
        }
    }

    /// Get the number of messages currently buffered.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Check if the channel is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Get the channel capacity in elements (0 = unbounded).
    pub fn capacity(&self) -> usize {
        self.queue.capacity()
    }

    /// Returns the Channel name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Returns the byte offset in the memory segment.
    pub fn offset(&self) -> usize {
        self.queue.offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_channel_send_recv() {
        let name = "/test_channel_basic";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let channel = Channel::<i32>::new(&memory, "channel_test", 4).unwrap();

        assert!(channel.is_empty());

        channel.send(1).unwrap();
        channel.send(2).unwrap();
        assert_eq!(channel.len(), 2);

        assert_eq!(channel.recv().unwrap(), 1);
        assert_eq!(channel.recv().unwrap(), 2);
        assert!(channel.is_empty());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_channel_backpressure() {
        let name = "/test_channel_bp";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let channel = Channel::<i32>::new(&memory, "channel_bp", 2).unwrap();

        // Fill the channel
        assert!(channel.try_send(1).unwrap());
        assert!(channel.try_send(2).unwrap());

        // Channel is full, try_send should fail
        assert!(!channel.try_send(3).unwrap());

        // Receive one, now there's space
        assert_eq!(channel.recv().unwrap(), 1);
        assert!(channel.try_send(3).unwrap());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_channel_producer_consumer_threads() {
        let name = "/test_channel_threads";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let channel = Arc::new(Channel::<i32>::new(&memory, "channel_threads", 4).unwrap());

        // Producer
        let producer_channel = Arc::clone(&channel);
        let producer = thread::spawn(move || {
            for i in 0..20 {
                producer_channel.send(i).unwrap();
            }
        });

        // Consumer
        let consumer_channel = Arc::clone(&channel);
        let consumer = thread::spawn(move || {
            let mut sum = 0;
            for _ in 0..20 {
                sum += consumer_channel.recv().unwrap();
            }
            sum
        });

        producer.join().unwrap();
        let sum = consumer.join().unwrap();
        assert_eq!(sum, 190); // 0+1+...+19

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_channel_timeout() {
        let name = "/test_channel_timeout";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let channel = Channel::<i32>::new(&memory, "channel_timeout", 2).unwrap();

        // Empty channel, recv should timeout
        assert_eq!(
            channel
                .recv_timeout(Duration::from_millis(50))
                .unwrap(),
            None
        );

        // Full channel, send should timeout
        channel.send(1).unwrap();
        channel.send(2).unwrap();
        assert!(!channel.send_timeout(3, Duration::from_millis(50)).unwrap());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_channel_open() {
        let name = "/test_channel_open";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let channel = Channel::<i32>::new(&memory, "channel_open", 4).unwrap();
            channel.send(42).unwrap();
        }

        let memory2 = Memory::open(name, 64).unwrap();
        let channel = Channel::<i32>::open(&memory2, "channel_open").unwrap();
        assert_eq!(channel.recv().unwrap(), 42);

        memory2.unlink().unwrap();
        drop(memory2);
        drop(memory);
    }
}
