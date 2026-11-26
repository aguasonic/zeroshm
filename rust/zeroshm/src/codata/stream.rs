//! Reactive stream for event-driven data processing in shared memory.
//!
//! Provides the `Stream<T>` type wrapping a Queue for producer-consumer
//! communication with map/filter/fold combinators. Built on the Queue primitive.
//!
//! # Binary Layout
//! Inherits the underlying Queue layout. The Stream is a view over a Queue.
//!
//! # Usage
//! - `emit(value)`: push a value into the stream (queue)
//! - `next()`: pop the next value from the stream
//! - `map(f)`: create a derived stream that transforms each element
//! - `filter(pred)`: create a derived stream that keeps only matching elements
//! - `fold(init, f)`: consume the stream and reduce to a single value
use crate::array::Pod;
use crate::error::Result;
use crate::memory::Memory;
use crate::queue::Queue;
use std::marker::PhantomData;
#[cfg(test)]
use std::sync::Arc;

/// A reactive stream backed by a shared memory Queue.
///
/// The stream wraps a Queue and provides combinators for building
/// data processing pipelines. The queue is stored in shared memory;
/// the combinators (map, filter) are local views that apply transformations
/// on read.
pub struct Stream<T: Pod> {
    queue: Queue<T>,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Stream<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("name", &self.name())
            .field("capacity", &self.capacity())
            .finish()
    }
}

// SAFETY: Stream holds a Queue which is Send + Sync.
unsafe impl<T: Pod> Send for Stream<T> {}
unsafe impl<T: Pod> Sync for Stream<T> {}

impl<T: Pod> Stream<T> {
    /// Create a new Stream backed by a Queue in shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name for the underlying queue
    /// * `capacity` -- Queue capacity in elements
    pub fn new(memory: &Memory, name: &str, capacity: usize) -> Result<Self> {
        let queue = Queue::new(memory, name, capacity)?;
        Ok(Self {
            queue,
            _marker: PhantomData,
        })
    }

    /// Open an existing Stream from shared memory.
    ///
    /// # Arguments
    /// * `memory` -- The shared memory segment
    /// * `name` -- Name of the underlying queue
    pub fn open(memory: &Memory, name: &str) -> Result<Self> {
        let queue = Queue::open(memory, name)?;
        Ok(Self {
            queue,
            _marker: PhantomData,
        })
    }

    /// Emit a value into the stream (blocking).
    pub fn emit(&self, value: T) -> Result<()> {
        self.queue.push(value)
    }

    /// Try to emit a value without blocking.
    ///
    /// Returns `Ok(true)` if emitted, `Ok(false)` if queue is full.
    pub fn try_emit(&self, value: T) -> Result<bool> {
        match self.queue.try_push(value) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Get the next value from the stream (blocking).
    pub fn next(&self) -> Result<T> {
        self.queue.pop()
    }

    /// Try to get the next value without blocking.
    ///
    /// Returns `Some(value)` if available, `None` if queue is empty.
    pub fn try_next(&self) -> Result<Option<T>> {
        self.queue.try_pop()
    }

    /// Get the number of items currently in the stream.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Check if the stream is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Get the stream capacity in elements.
    pub fn capacity(&self) -> usize {
        self.queue.capacity()
    }

    /// Get the underlying queue name.
    pub fn name(&self) -> &str {
        self.queue.name()
    }

    /// Returns a reference to the underlying memory segment.
    pub fn memory(&self) -> &Memory {
        self.queue.memory()
    }

    /// Create a mapped stream that applies `f` to each element.
    ///
    /// The returned stream reads from this stream and transforms each value.
    /// This is a local view -- no new shared memory structure is created.
    pub fn map<U: Pod, F>(&self, f: F) -> MappedStream<'_, T, U, F>
    where
        F: Fn(T) -> U,
    {
        MappedStream {
            source: self,
            f,
            _marker: PhantomData,
        }
    }

    /// Create a filtered stream that keeps only elements matching `pred`.
    ///
    /// The returned stream reads from this stream and skips non-matching values.
    /// This is a local view -- no new shared memory structure is created.
    pub fn filter<F>(&self, pred: F) -> FilteredStream<'_, T, F>
    where
        F: Fn(T) -> bool,
    {
        FilteredStream {
            source: self,
            pred,
            _marker: PhantomData,
        }
    }

    /// Consume the stream and fold all elements into a single value.
    ///
    /// Reads elements until the queue is empty, applying `f` to accumulate.
    pub fn fold<U, F>(&self, init: U, f: F) -> U
    where
        F: Fn(U, T) -> U,
    {
        let mut acc = init;
        while let Ok(Some(value)) = self.queue.try_pop() {
            acc = f(acc, value);
        }
        acc
    }
}

/// A mapped stream view that transforms elements from a source stream.
pub struct MappedStream<'a, T: Pod, U: Pod, F> {
    source: &'a Stream<T>,
    f: F,
    _marker: PhantomData<U>,
}

impl<'a, T: Pod, U: Pod, F> MappedStream<'a, T, U, F>
where
    F: Fn(T) -> U,
{
    /// Get the next transformed value (blocking).
    pub fn next(&self) -> Result<U> {
        let value = self.source.next()?;
        Ok((self.f)(value))
    }

    /// Try to get the next transformed value without blocking.
    pub fn try_next(&self) -> Result<Option<U>> {
        match self.source.try_next()? {
            Some(value) => Ok(Some((self.f)(value))),
            None => Ok(None),
        }
    }
}

/// A filtered stream view that keeps only elements matching a predicate.
pub struct FilteredStream<'a, T: Pod, F> {
    source: &'a Stream<T>,
    pred: F,
    _marker: PhantomData<T>,
}

impl<'a, T: Pod, F> FilteredStream<'a, T, F>
where
    F: Fn(T) -> bool,
{
    /// Get the next matching value (blocking).
    ///
    /// Skips non-matching elements until a match is found.
    pub fn next(&self) -> Result<T> {
        loop {
            let value = self.source.next()?;
            if (self.pred)(value) {
                return Ok(value);
            }
        }
    }

    /// Try to get the next matching value without blocking.
    ///
    /// Returns `Some(value)` for the first matching element, `None` if no
    /// more elements are available.
    pub fn try_next(&self) -> Result<Option<T>> {
        loop {
            match self.source.try_next()? {
                Some(value) if (self.pred)(value) => return Ok(Some(value)),
                Some(_) => continue,
                None => return Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;
    use std::thread;

    #[test]
    fn test_stream_emit_and_next() {
        let name = "/test_stream_basic";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let stream = Stream::<i32>::new(&memory, "stream_test", 16).unwrap();

        assert!(stream.is_empty());

        stream.emit(1).unwrap();
        stream.emit(2).unwrap();
        stream.emit(3).unwrap();

        assert_eq!(stream.len(), 3);

        assert_eq!(stream.next().unwrap(), 1);
        assert_eq!(stream.next().unwrap(), 2);
        assert_eq!(stream.next().unwrap(), 3);
        assert!(stream.is_empty());

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_stream_map() {
        let name = "/test_stream_map";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let stream = Stream::<i32>::new(&memory, "stream_map", 8).unwrap();

        for i in 1..=5 {
            stream.emit(i).unwrap();
        }

        let mapped = stream.map(|x| x * 2);
        assert_eq!(mapped.next().unwrap(), 2);
        assert_eq!(mapped.next().unwrap(), 4);
        assert_eq!(mapped.next().unwrap(), 6);
        assert_eq!(mapped.next().unwrap(), 8);
        assert_eq!(mapped.next().unwrap(), 10);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_stream_filter() {
        let name = "/test_stream_filter";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let stream = Stream::<i32>::new(&memory, "stream_filter", 16).unwrap();

        for i in 1..=10 {
            stream.emit(i).unwrap();
        }

        let filtered = stream.filter(|x| x % 2 == 0);
        assert_eq!(filtered.try_next().unwrap(), Some(2));
        assert_eq!(filtered.try_next().unwrap(), Some(4));
        assert_eq!(filtered.try_next().unwrap(), Some(6));
        assert_eq!(filtered.try_next().unwrap(), Some(8));
        assert_eq!(filtered.try_next().unwrap(), Some(10));
        assert_eq!(filtered.try_next().unwrap(), None);

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_stream_fold() {
        let name = "/test_stream_fold";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let stream = Stream::<i32>::new(&memory, "stream_fold", 8).unwrap();

        for i in 1..=5 {
            stream.emit(i).unwrap();
        }

        let sum = stream.fold(0, |acc, x| acc + x);
        assert_eq!(sum, 15); // 1+2+3+4+5

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_stream_producer_consumer_threads() {
        let name = "/test_stream_threads";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();
        let stream = Arc::new(Stream::<i32>::new(&memory, "stream_threads", 8).unwrap());

        // Producer
        let producer_stream = Arc::clone(&stream);
        let producer = thread::spawn(move || {
            for i in 0..10 {
                producer_stream.emit(i).unwrap();
            }
        });

        // Consumer
        let consumer_stream = Arc::clone(&stream);
        let consumer = thread::spawn(move || {
            let mut sum = 0;
            for _ in 0..10 {
                sum += consumer_stream.next().unwrap();
            }
            sum
        });

        producer.join().unwrap();
        let sum = consumer.join().unwrap();
        assert_eq!(sum, 45); // 0+1+...+9

        memory.unlink().unwrap();
        drop(memory);
    }

    #[test]
    fn test_stream_open() {
        let name = "/test_stream_open";
        let _ = platform::unlink_shm(name);
        let memory = Memory::create(name, 1 << 20, 64).unwrap();

        {
            let stream = Stream::<i32>::new(&memory, "stream_open", 4).unwrap();
            stream.emit(42).unwrap();
        }

        let memory2 = Memory::open(name, 64).unwrap();
        let stream = Stream::<i32>::open(&memory2, "stream_open").unwrap();
        assert_eq!(stream.next().unwrap(), 42);

        memory2.unlink().unwrap();
        drop(memory2);
        drop(memory);
    }
}
