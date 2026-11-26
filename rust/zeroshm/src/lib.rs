//! ZeroSHM - Zero-copy shared memory
//!
//! Rust implementation of the ZeroSHM binary format specification.
//! Provides zero-copy data sharing between processes via shared memory.

pub mod constants_generated;
pub mod error;
pub mod layout;
pub mod hash;
pub mod platform;
pub mod memory;
pub mod table;
pub mod array;
pub mod queue;
pub mod stack;
pub mod ring;
pub mod latest;
pub mod map;
pub mod set;
pub mod pool;
pub mod sync;
pub mod codata;

pub use error::{Result, ZeroShmError};
pub use hash::hash_name;
pub use memory::Memory;
pub use table::{Table, TableEntry, TableHeader};
pub use array::{Array, Pod};
pub use queue::Queue;
pub use stack::Stack;
pub use ring::Ring;
pub use latest::Latest;
pub use map::Map;
pub use set::Set;
pub use pool::{Pool, PoolSlot};
pub use sync::{Barrier, Event, EventMode, Latch, Monitor, MonitorGuard, Mutex, MutexGuard, Once, RWLock, RwLockReadGuard, RwLockWriteGuard, Semaphore, Signal};
pub use codata::{Channel, Future, Lazy, Stream};

