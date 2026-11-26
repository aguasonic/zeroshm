//! Synchronization primitives for cross-process coordination.
//!
//! Provides semaphore, mutex, once, barrier, latch, rwlock, event, monitor,
//! and signal primitives stored in shared memory, enabling coordination
//! between multiple processes.

pub mod semaphore;
pub mod mutex;
pub mod once;
pub mod barrier;
pub mod latch;
pub mod rwlock;
pub mod event;
pub mod monitor;
pub mod signal;

pub use semaphore::Semaphore;
pub use mutex::{Mutex, MutexGuard};
pub use once::Once;
pub use barrier::Barrier;
pub use latch::Latch;
pub use rwlock::{RWLock, RwLockReadGuard, RwLockWriteGuard};
pub use event::{Event, EventMode};
pub use monitor::{Monitor, MonitorGuard};
pub use signal::Signal;
