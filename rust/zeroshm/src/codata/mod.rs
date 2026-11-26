//! Codata structures for coordination and reactive patterns in shared memory.
//!
//! Provides Future, Lazy, Stream, and Channel types built on top of the
//! primitive synchronization and data structures. These enable advanced
//! coordination patterns across process boundaries.

pub mod future;
pub mod lazy;
pub mod stream;
pub mod channel;

pub use future::Future;
pub use lazy::Lazy;
pub use stream::Stream;
pub use channel::Channel;

