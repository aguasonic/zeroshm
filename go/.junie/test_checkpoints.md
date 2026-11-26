# Unit Test Build Checkpoints

This file tracks progress building unit tests for all public interfaces in the Go solution.
One interface is tested at a time, with a checkpoint after each.

## Interfaces and Status

| # | Interface | Constructor | Methods Tested | Status |
|---|-----------|-------------|----------------|--------|
| 1 | Memory | NewMemory, OpenMemory | Size, Name, IsOwner, Data, At, Dump, Close, Unlink, UnlinkName, Table, Allocate, Find | ✅ COMPLETE |
| 2 | Table | NewTable | Header, Entry, Find, Add, Allocate, EntryCount, MaxEntries, NextOffset, Entries, CalculateTableSize, Entry.NameString | ✅ COMPLETE |
| 3 | Array[T] | NewArray, OpenArray | Get, Set, Capacity, Name, Fill, Slice, CopyFrom, Range | ✅ COMPLETE |
| 4 | Queue[T] | NewQueue, OpenQueue | Push, Pop, TryPush, TryPop, Empty, Full, Size, Capacity, Name | ✅ COMPLETE |
| 5 | Stack[T] | NewStack, OpenStack | Push, Pop, Top, Empty, Full, Size, Capacity, Name | ✅ COMPLETE |
| 6 | Semaphore | NewSemaphore, OpenSemaphore | Acquire, TryAcquire, AcquireTimeout, Release, Count, Waiting, MaxCount, Name | ✅ COMPLETE |
| 7 | Mutex | NewMutex, OpenMutex | Lock, Unlock, TryLock, LockTimeout, Name, IsLocked | ✅ COMPLETE |
| 8 | Barrier | NewBarrier, OpenBarrier | Wait, WaitTimeout, Arrived, Generation, NumParticipants, Name | ✅ COMPLETE |
| 9 | Latch | NewLatch, OpenLatch | CountDown, CountDownOne, Wait, TryWait, WaitTimeout, Count, InitialCount, Name, Reset | ✅ COMPLETE |
| 10 | Once | NewOnce, OpenOnce | Do, Call, IsCalled, AlreadyCalled, Name, ResetUnsafe | ✅ COMPLETE |
| 11 | Signal[T] | NewSignal, OpenSignal | Get, Set, Update, Version, HasChanged, WaitForChange, Destroy, Name | ✅ COMPLETE |
| 12 | Event | NewEvent, OpenEvent | Signal, Wait, WaitTimeout, Reset, Pulse, IsSignaled, Destroy, Name | ✅ COMPLETE |
| 13 | RWLock | NewRWLock, OpenRWLock | ReaderLock, ReaderUnlock, WriterLock, WriterUnlock, TryReaderLock, TryWriterLock, Destroy, Name | ✅ COMPLETE |
| 14 | Monitor | NewMonitor, OpenMonitor | Lock, Unlock, TryLock, Wait, WaitWithPredicate, WaitTimeoutWithPredicate, NotifyOne, NotifyAll, Destroy, Name | ✅ COMPLETE |
| 15 | Set[T] | NewSet, OpenSet | Insert, Contains, Erase, Size, Capacity, Empty, Clear, Entries, Range, Destroy, Name | ✅ COMPLETE |
| 16 | Map[K,V] | NewMap, OpenMap | Set, Get, Delete, Contains, Size, Capacity, Empty, Clear, Keys, Range, Entries, Destroy, Name | ✅ COMPLETE |
| 17 | Ring[T] | NewRing, OpenRing | Push, Pop, Peek, Size, Capacity, Empty, Full, FreeSpace, Reset, Destroy, Name | ✅ COMPLETE |
| 18 | Pool[T] | NewPool, OpenPool | Allocate, Free, Get, Set, Size, Capacity, Available, Empty, Full, Destroy, Name | ✅ COMPLETE |
| 19 | Channel[T] | NewChannel, OpenChannel | Send, TrySend, SendTimeout, Receive, TryReceive, ReceiveTimeout, Close, IsClosed, Capacity, IsBuffered, Destroy, Name | ✅ COMPLETE |
| 20 | Future[T] | NewFuture, OpenFuture | SetValue, SetError, Get, TryGet, GetTimeout, IsReady, State, Waiters, Destroy, Name | ✅ COMPLETE |
| 21 | Lazy[T] | NewLazy, NewLazyWithInitialValue, OpenLazy | Force, Get, Peek, IsComputed, State, ComputeCount, Reset, Destroy, Name | ✅ COMPLETE |
| 22 | Stream[T] | NewStream, OpenStream | Emit, Read, ReadTimeout, Close, IsClosed, Capacity, Size, Version, Subscribers, Subscribe, Destroy, Name | ✅ COMPLETE |

## Checkpoint Details

### Checkpoint 1: Memory (COMPLETE)
**Date:** 2026-11-22
**Tests added:**
- `TestMemoryAccessors` — Size, Name, IsOwner, Data, At (valid), At (invalid bounds)
- `TestMemoryAtBounds` — At bounds checking (valid offset, out-of-bounds panic)
- `TestMemoryDump` — Dump to file and verify content
- `TestMemoryCloseAndUnlink` — Close, Unlink, and verify Open fails after unlink
- `TestMemoryIsOwnerAfterOpen` — IsOwner returns true for creator, false for opener

**Tests already existing:**
- `TestMemoryCreateAndOpen` — NewMemory, OpenMemory, Data, Table, Size

**Next:** Signal interface (Phase 1) - ✅ COMPLETE

### Checkpoint 22: Stream[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (stream_test.go):**
- `TestStreamName` — Name() returns correct value
- `TestStreamNewOpen` — NewStream and OpenStream work correctly
- `TestStreamEmitRead` — Basic emit and read
- `TestStreamEmitMultiple` — Emit multiple values, read in order
- `TestStreamReadEmpty` — Read from empty stream returns false
- `TestStreamCapacity` — Capacity() returns correct value
- `TestStreamSize` — Size() tracks items correctly
- `TestStreamVersion` — Version() increments on emit
- `TestStreamBufferFull` — Emit fails when buffer is full
- `TestStreamClose` — Close prevents new emits, allows drain
- `TestStreamIsClosed` — IsClosed() state tracking
- `TestStreamReadTimeoutSuccess` — ReadTimeout succeeds when data available
- `TestStreamReadTimeoutFail` — ReadTimeout fails when no data
- `TestStreamReadTimeoutClosed` — ReadTimeout fails on closed stream
- `TestStreamSubscribe` — Subscribe increments/decrements count
- `TestStreamNewNameTooLong` — Name too long fails
- `TestStreamNewNameTooShort` — Name too short fails
- `TestStreamNewAlreadyExists` — Duplicate name fails
- `TestStreamOpenNotExists` — Open non-existent name fails
- `TestStreamOpenCrossProcess` — Open sees creator's data
- `TestStreamInt64` — Stream with int64 values
- `TestStreamFloat64` — Stream with float64 values
- `TestStreamDestroyAndReuse` — Destroy removes entries, name reusable
- `TestStreamCloseThenDrain` — After close, drain remaining items
- `TestStreamConcurrentProducers` — Multiple producers with backpressure

**Total Stream tests:** 25 — ALL PASSING

**Implementation notes:**
- Built on Ring[T] for backpressure handling
- Binary layout: header(56 bytes: sequence, subscribers, closed, capacity, padding, transform_name) + Ring buffer
- Sequence number tracks total items ever emitted
- Subscribers count tracks active readers
- Close allows draining remaining items then stops

**Fixed in implementation:**
- Ring.Push returns error (not bool) — Emit now checks for error
- Ring.Pop returns (T, error) — Read and ReadTimeout handle error properly
- OpenMemory requires maxEntries parameter

### Checkpoint 21: Lazy[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (lazy_test.go):**
- `TestLazyName` — Name() returns correct value
- `TestLazyInitialState` — NOT_COMPUTED state, compute count 0
- `TestLazyForceCompute` — Force computes value, state becomes COMPUTED
- `TestLazyForceOnce` — Second Force does not recompute, uses cached value
- `TestLazyForceError` — Force with error sets ERROR state, subsequent forces return error
- `TestLazyGet` — Get blocks until Force completes in another goroutine
- `TestLazyPeekNotComputed` — Peek returns LazyNotReady before computation
- `TestLazyPeekComputed` — Peek returns cached value after computation
- `TestLazyReset` — Reset allows recomputation with new value
- `TestLazyConcurrentForce` — 8 concurrent forces, only 1 computes, all get same value
- `TestLazyOpenCrossProcess` — OpenLazy sees creator's computed value
- `TestLazyOpenNotFound` — OpenLazy non-existent name fails
- `TestLazyNewNameTooLong` — NewLazy name too long fails
- `TestLazyNewNameTooShort` — NewLazy name too short fails
- `TestLazyNewAlreadyExists` — NewLazy duplicate name fails
- `TestLazyOpenTypeMismatch` — OpenLazy with wrong type size fails
- `TestLazyInt64` — Lazy with int64 values
- `TestLazyFloat64` — Lazy with float64 values
- `TestLazyWithInitialValue` — NewLazyWithInitialValue starts COMPUTED
- `TestLazyDestroyAndReuse` — Destroy removes table entry, name reusable

**Total Lazy tests:** 20 — ALL PASSING

**Implementation notes:**
- CAS-based single-computation guarantee (NOT_COMPUTED → COMPUTING → COMPUTED)
- Binary layout: header(16 bytes: state, compute_count, completion_time) + value(T) + error_msg[256]
- First caller to Force() computes; others spin-wait for result

### Checkpoint 20: Future[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (future_test.go):**
- `TestFutureName` — Name() returns correct value
- `TestFutureInitialState` — PENDING state, not ready
- `TestFutureSetValueGet` — SetValue then Get returns value
- `TestFutureTryGetNotReady` — TryGet returns FutureNotReady before set
- `TestFutureTryGetReady` — TryGet returns value after set
- `TestFutureSetValueOnce` — Second SetValue fails, first value preserved
- `TestFutureSetError` — SetError sets ERROR state, Get returns error
- `TestFutureGetTimeoutSuccess` — GetTimeout succeeds when value set within timeout
- `TestFutureGetTimeoutFail` — GetTimeout fails when value not set within timeout
- `TestFutureGetBlocks` — Get blocks until SetValue in another goroutine
- `TestFutureOpenCrossProcess` — OpenFuture sees creator's value
- `TestFutureOpenNotFound` — OpenFuture non-existent name fails
- `TestFutureNewNameTooLong` — NewFuture name too long fails
- `TestFutureNewNameTooShort` — NewFuture name too short fails
- `TestFutureNewAlreadyExists` — NewFuture duplicate name fails
- `TestFutureOpenTypeMismatch` — OpenFuture with wrong type size fails
- `TestFutureMultipleWaiters` — 4 concurrent Get calls all receive same value
- `TestFutureInt64` — Future with int64 values
- `TestFutureFloat64` — Future with float64 values
- `TestFutureDestroyAndReuse` — Destroy removes table entry, name reusable

**Total Future tests:** 20 — ALL PASSING

**Implementation notes:**
- State-based spin-wait (PENDING/COMPUTING/READY/ERROR)
- Binary layout: header(16 bytes: state, waiters, completion_time) + value(T) + error_msg[256]
- Multiple consumers can await the same result

### Checkpoint 19: Channel[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (channel_test.go):**
- `TestChannelBufferedName` — Name() returns correct value
- `TestChannelBufferedCreate` — Buffered channel with correct capacity
- `TestChannelBufferedSendReceive` — Send/Receive basic operations
- `TestChannelBufferedFIFOOrder` — Elements received in FIFO order
- `TestChannelBufferedTrySendFull` — TrySend returns ChannelFull when buffer full
- `TestChannelBufferedTryReceiveEmpty` — TryReceive returns ChannelEmpty when empty
- `TestChannelBufferedReceiveTimeout` — ReceiveTimeout succeeds/fails correctly
- `TestChannelBufferedSendTimeout` — SendTimeout fails when buffer full
- `TestChannelBufferedClose` — Close prevents sends, allows drain
- `TestChannelBufferedOpenCrossProcess` — OpenChannel sees creator's messages
- `TestChannelBufferedOpenNotFound` — OpenChannel non-existent name fails
- `TestChannelNewNameTooLong` — NewChannel name too long fails
- `TestChannelNewNameTooShort` — NewChannel name too short fails
- `TestChannelNewAlreadyExists` — NewChannel duplicate name fails
- `TestChannelBufferedCapacityRounding` — Capacity rounded to next power of 2
- `TestChannelBufferedMPMC` — 4 senders × 4 receivers, all messages received
- `TestChannelUnbufferedCreate` — Unbuffered channel (capacity 0)
- `TestChannelUnbufferedRendezvous` — Synchronous send/receive rendezvous
- `TestChannelUnbufferedTrySendNoReceiver` — TrySend returns ChannelFull with no receiver
- `TestChannelUnbufferedTryReceiveNoSender` — TryReceive returns ChannelEmpty with no sender
- `TestChannelUnbufferedCloseUnblocks` — Close unblocks waiting receiver
- `TestChannelInt64` — Channel with int64 messages
- `TestChannelFloat64` — Channel with float64 messages
- `TestChannelDestroyAndReuse` — Destroy removes table entries, name reusable

**Total Channel tests:** 24 — ALL PASSING

**Implementation notes:**
- Buffered channels: Queue[T]-backed (MPMC-safe)
- Unbuffered channels: Rendezvous slot with ready/consumed flags (SPSC)
- Binary layout: header(32 bytes: capacity, senders, receivers, closed, send_seq, recv_seq) + buffer/slot

### Checkpoint 18: Pool[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (pool_test.go):**
- `TestPoolName` — Name() returns correct value
- `TestPoolCreateEmpty` — Empty pool has size 0, correct capacity and available slots
- `TestPoolAllocateFree` — Allocate/Free cycle updates size and available correctly
- `TestPoolAllocateExhausted` — Allocate all slots, pool becomes full, next allocate returns PoolExhausted, all slots unique
- `TestPoolFreeAndReallocate` — Free a slot, reallocate reuses freed slot
- `TestPoolGetSet` — Set/Get basic operations, value update
- `TestPoolInvalidSlotGet` — Get with invalid index returns PoolInvalidSlot
- `TestPoolInvalidSlotSet` — Set with invalid index returns PoolInvalidSlot
- `TestPoolInvalidSlotFree` — Free with invalid index returns PoolInvalidSlot
- `TestPoolMultipleSlots` — Allocate multiple slots, set different values, verify all retrievable
- `TestPoolOpenCrossProcess` — OpenPool sees creator's values and can allocate from second handle
- `TestPoolOpenNotFound` — OpenPool non-existent name fails
- `TestPoolNewNameTooLong` — NewPool name too long fails
- `TestPoolNewNameTooShort` — NewPool name too short fails
- `TestPoolNewAlreadyExists` — NewPool duplicate name fails
- `TestPoolNewInvalidCapacity` — NewPool zero capacity fails
- `TestPoolOpenTypeMismatch` — OpenPool with wrong type size fails
- `TestPoolConcurrentAllocate` — 8 goroutines × 4 allocates, no duplicate slots, size matches
- `TestPoolInt64` — Pool with int64 elements (large positive)
- `TestPoolFloat64` — Pool with float64 elements
- `TestPoolUint8` — Pool with uint8 elements (max value 255)
- `TestPoolFreeMultipleTimes` — Free same slot twice does not crash

**Total Pool tests:** 22 — ALL PASSING

**Implementation notes:**
- Lock-free fixed-size object pool with free-list
- Uses tagged pointer CAS (generation + index) to prevent ABA problems
- Binary layout: header(24 bytes: free_head, allocated, padding_, capacity, elem_size) + nodes(capacity × (data(T) + next(uint32)))
- Node data aligned to 8 bytes, followed by 4-byte next pointer
- Allocate: CAS free_head with bumped generation; Free: push node onto free list with bumped generation

### Checkpoint 17: Ring[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (ring_test.go):**
- `TestRingName` — Name() returns correct value
- `TestRingCreateEmpty` — Empty ring has size 0, correct capacity and free space
- `TestRingPushAndPop` — Push/Pop basic operations
- `TestRingPopEmpty` — Pop from empty ring returns RingEmpty
- `TestRingPushFull` — Push to full ring returns RingFull
- `TestRingPeek` — Peek reads without removing, size unchanged
- `TestRingPeekEmpty` — Peek from empty ring returns RingEmpty
- `TestRingFIFOOrder` — Elements popped in FIFO order
- `TestRingWrapAround` — Ring wraps around after fill/drain cycle
- `TestRingElementSplitAcrossBoundary` — 8-byte element split across buffer boundary
- `TestRingReset` — Reset clears all elements
- `TestRingOpenCrossProcess` — OpenRing sees creator's elements
- `TestRingOpenNotFound` — OpenRing non-existent name fails
- `TestRingNewNameTooLong` — NewRing name too long fails
- `TestRingNewNameTooShort` — NewRing name too short fails
- `TestRingNewAlreadyExists` — NewRing duplicate name fails
- `TestRingNewInvalidCapacity` — NewRing zero capacity fails
- `TestRingOpenTypeMismatch` — OpenRing with wrong type size fails
- `TestRingSPSC` — Single producer / single consumer concurrent access, FIFO order preserved
- `TestRingInt64` — Ring with int64 elements (large positive/negative)
- `TestRingFloat64` — Ring with float64 elements
- `TestRingInt8` — Ring with int8 elements (min/max values)
- `TestRingDestroyAndReuse` — Destroy removes table entry, name reusable
- `TestRingCapacityRounding` — Capacity in elements correctly allocated

**Total Ring tests:** 24 — ALL PASSING

**Implementation notes:**
- SPSC (single-producer / single-consumer) lock-free ring buffer
- Byte-based positions (write_pos/read_pos as uint64) with modulo for wrap-around
- Binary layout: header(24 bytes: write_pos, read_pos, capacity, elem_size) + buffer(capacity bytes)
- Supports split read/write when element crosses buffer boundary
- Push uses load on write_pos + acquire on read_pos; Pop uses load on read_pos + acquire on write_pos

### Checkpoint 16: Map[K,V] (COMPLETE)
**Date:** 2026-11-22
**Tests added (map_test.go):**
- `TestMapName` — Name() returns correct value
- `TestMapCreateEmpty` — Empty map has size 0, correct capacity
- `TestMapSetAndGet` — Set/Get basic operations
- `TestMapGetNotFound` — Get non-existent key returns MapNotFound
- `TestMapSetUpdate` — Update existing key, value changes, size unchanged
- `TestMapSetMultiple` — Set 10 key-value pairs, all retrievable
- `TestMapDelete` — Delete removes key, size decrements
- `TestMapDeleteNotFound` — Delete non-existent returns MapNotFound
- `TestMapContains` — Contains true for existing key, false for missing
- `TestMapFull` — Set beyond capacity returns MapFull
- `TestMapClear` — Clear removes all elements, size becomes 0
- `TestMapRange` — Range iterates over occupied entries with key-value pairs
- `TestMapRangeEarlyStop` — Range stops early when fn returns false
- `TestMapKeys` — Keys returns all existing keys
- `TestMapEntries` — Entries returns all slots with state information
- `TestMapOpenCrossProcess` — OpenMap sees creator's key-value pairs
- `TestMapOpenNotFound` — OpenMap non-existent name fails
- `TestMapNewNameTooLong` — NewMap name too long fails
- `TestMapNewNameTooShort` — NewMap name too short fails
- `TestMapNewAlreadyExists` — NewMap duplicate name fails
- `TestMapNewInvalidCapacity` — NewMap zero capacity fails
- `TestMapOpenTypeMismatch` — OpenMap with wrong type sizes fails
- `TestMapConcurrentSet` — 8 workers × 8 items, all present with correct values
- `TestMapConcurrentSetDelete` — Concurrent set and delete, correct final size
- `TestMapInt64Values` — Map with int64 values (large positive/negative)
- `TestMapFloat64Values` — Map with float64 values
- `TestMapUint64Keys` — Map with uint64 keys (max value + 0)
- `TestMapDestroyAndReuse` — Destroy removes table entry, name reusable
- `TestMapDeleteAndReinsert` — Delete creates tombstone, reinsert reuses slot
- `TestMapDeleteThroughTombstones` — Get continues probing through tombstones

**Total Map tests:** 30 — ALL PASSING

**Implementation notes:**
- Uses 4-state CAS (EMPTY/OCCUPIED/DELETED/INSERTING) with linear probing
- Lock-free concurrent set/delete/update via atomic CAS on slot state
- Binary layout: header(16 bytes: size, capacity, key_size, value_size) + entries(capacity × (state(4) + key(K) + value(V), 8-aligned))
- Supports mixed key/value types (e.g., int32 keys with int64 values, uint64 keys with int32 values)
- mapHash uses multiplicative hash for integers, FNV for floats

### Checkpoint 15: Set[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (set_test.go):**
- `TestSetName` — Name() returns correct value
- `TestSetCreateEmpty` — Empty set has size 0, correct capacity
- `TestSetInsertAndContains` — Insert/Contains basic operations
- `TestSetInsertDuplicate` — Duplicate insert is idempotent, size unchanged
- `TestSetInsertMultiple` — Insert 10 distinct values, all present
- `TestSetErase` — Erase removes element, size decrements
- `TestSetEraseNotFound` — Erase non-existent returns SetNotFound
- `TestSetFull` — Insert beyond capacity returns SetFull
- `TestSetClear` — Clear removes all elements, size becomes 0
- `TestSetRange` — Range iterates over occupied entries
- `TestSetEntries` — Entries returns all slots with state information
- `TestSetOpenCrossProcess` — OpenSet sees creator's elements
- `TestSetOpenNotFound` — OpenSet non-existent name fails
- `TestSetNewNameTooLong` — NewSet name too long fails
- `TestSetNewNameTooShort` — NewSet name too short fails
- `TestSetNewAlreadyExists` — NewSet duplicate name fails
- `TestSetNewInvalidCapacity` — NewSet zero capacity fails
- `TestSetOpenTypeMismatch` — OpenSet with wrong type size fails
- `TestSetConcurrentInsert` — 8 workers × 32 items, all present
- `TestSetConcurrentInsertErase` — Concurrent insert and erase, correct final size
- `TestSetUint64` — Set with uint64 elements (max value + 0)
- `TestSetFloat64` — Set with float64 elements
- `TestSetDestroyAndReuse` — Destroy removes table entry, name reusable
- `TestSetEraseAfterInsertProbing` — Erase through tombstones, re-insert reuses slot

**Total Set tests:** 24 — ALL PASSING

**Implementation notes:**
- Uses 4-state CAS (EMPTY/OCCUPIED/DELETED/INSERTING) with linear probing
- Lock-free concurrent insert/erase via atomic CAS on slot state
- Binary layout: header(16 bytes) + entries(capacity × (state(4) + value(T), 8-aligned))
- trivialHash uses multiplicative hash for integers, FNV for floats

### Checkpoint 14: Monitor (COMPLETE)
**Date:** 2026-11-22
**Tests added (monitor_test.go):**
- `TestMonitorName` — Name() returns correct value
- `TestMonitorLockUnlock` — Lock/Unlock cycle
- `TestMonitorTryLock` — TryLock success and failure cases
- `TestMonitorNotifyOne` — NotifyOne wakes a single waiting goroutine
- `TestMonitorNotifyAll` — NotifyAll wakes all waiting goroutines
- `TestMonitorWaitTimeout` — WaitTimeout fails when predicate never becomes true
- `TestMonitorWaitTimeoutSuccess` — WaitTimeout succeeds when predicate becomes true
- `TestMonitorNewNameTooLong` — NewMonitor name too long fails
- `TestMonitorNewAlreadyExists` — NewMonitor duplicate name fails
- `TestMonitorOpenNotExists` — OpenMonitor non-existent name fails
- `TestMonitorOpenCrossProcess` — OpenMonitor sees creator's lock state
- `TestMonitorCrossGoroutine` — Wait/Notify across goroutines with predicate

### Checkpoint 13: RWLock (COMPLETE)
**Date:** 2026-11-22
**Tests added (rwlock_test.go):**
- `TestRWLockName` — Name() returns correct value
- `TestRWLockReaderLock` — Multiple concurrent reader locks
- `TestRWLockWriterLock` — Writer lock/unlock cycle
- `TestRWLockWriterBlocksReaders` — Writer blocks new readers
- `TestRWLockReaderBlocksWriter` — Readers block writer
- `TestRWLockTryReaderLock` — TryReaderLock success (concurrent readers allowed)
- `TestRWLockTryWriterLock` — TryWriterLock exclusive access
- `TestRWLockConcurrentReaders` — 10 concurrent readers
- `TestRWLockWriterExclusive` — Writers exclusive with concurrent readers
- `TestRWLockNewNameTooLong` — NewRWLock name too long fails
- `TestRWLockNewAlreadyExists` — NewRWLock duplicate name fails
- `TestRWLockOpenNotExists` — OpenRWLock non-existent name fails
- `TestRWLockOpenCrossProcess` — OpenRWLock sees creator's lock state

### Checkpoint 12: Event (COMPLETE)
**Date:** 2026-11-22
**Tests added (event_test.go):**
- `TestEventName` — Name() returns correct value
- `TestEventIsSignaled` — IsSignaled() state tracking through Signal/Reset
- `TestEventAutoReset` — AutoReset event: Wait returns after Signal, event auto-resets
- `TestEventManualReset` — ManualReset event: Signal wakes all waiters, stays signaled
- `TestEventWaitTimeoutSuccess` — WaitTimeout succeeds when event is signaled
- `TestEventWaitTimeoutFail` — WaitTimeout fails on timeout
- `TestEventPulse` — Pulse wakes waiters then resets
- `TestEventNewNameTooLong` — NewEvent name too long fails
- `TestEventNewAlreadyExists` — NewEvent duplicate name fails
- `TestEventOpenNotExists` — OpenEvent non-existent name fails
- `TestEventOpenCrossProcess` — OpenEvent sees creator's signal state
- `TestEventConcurrentWaiters` — 5 concurrent waiters all wake on single Signal

### Checkpoint 11: Signal[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (signal_test.go):**
- `TestSignalName` — Name() returns correct value
- `TestSignalGetSet` — Get/Set operations with initial value
- `TestSignalUpdate` — Update with function (atomic transform)
- `TestSignalVersion` — Version increments on each Set/Update
- `TestSignalHasChanged` — HasChanged detects version changes
- `TestSignalWaitForChange` — WaitForChange returns true when value changes
- `TestSignalWaitForChangeTimeout` — WaitForChange returns false on timeout
- `TestSignalNewNameTooLong` — NewSignal name too long fails
- `TestSignalNewAlreadyExists` — NewSignal duplicate name fails
- `TestSignalOpenNotExists` — OpenSignal non-existent name fails
- `TestSignalOpenCrossProcess` — OpenSignal sees creator's value and version
- `TestSignalConcurrentAccess` — 5 concurrent goroutines, atomic Update, correct final value and version
- `TestSignalFloat64` — Signal with float64 type

### Checkpoint 10: Once (COMPLETE)
**Date:** 2026-11-22
**Tests added (once_test.go):**
- `TestOnceName` — Name() returns correct value
- `TestOnceIsCalled` — IsCalled() state tracking before and after Do()
- `TestOnceCall` — Call() alias works same as Do(), executes fn exactly once
- `TestOnceAlreadyCalled` — AlreadyCalled() alias works same as IsCalled()
- `TestOnceResetUnsafe` — ResetUnsafe() allows Do() to execute fn again
- `TestOnceNewNameTooLong` — NewOnce name too long fails
- `TestOnceNewAlreadyExists` — NewOnce duplicate name fails
- `TestOnceOpenNotExists` — OpenOnce non-existent name fails
- `TestOnceOpenCrossProcess` — OpenOnce sees creator's state, second Do() doesn't re-execute
- `TestOnceDoPanic` — Do() marks done even if fn panics (std::call_once semantics)
- `TestOnceConcurrentCrossProcess` — Concurrent Do from NewOnce and OpenOnce instances, fn executes exactly once

**Tests already existing (structures_test.go):**
- `TestOnceBasic` — NewOnce, IsCalled, Do (executes once, second Do doesn't execute)
- `TestOnceConcurrent` — 8 goroutines, fn executes exactly once, all wait for completion (3-state protocol)

### Checkpoint 9: Latch (COMPLETE)
**Date:** 2026-11-22
**Tests added (latch_test.go):**
- `TestLatchName` — Name() returns correct value
- `TestLatchCount` — Count() tracks countdown correctly, saturation at 0
- `TestLatchInitialCount` — InitialCount() returns immutable initial value
- `TestLatchCountDown` — CountDown() decrements correctly, saturates at 0, rejects non-positive values
- `TestLatchTryWait` — TryWait() non-blocking check before and after countdown
- `TestLatchWait` — Wait() blocks until count reaches 0
- `TestLatchWaitAlreadyZero` — Wait() returns immediately when count is 0
- `TestLatchWaitTimeoutSuccess` — WaitTimeout succeeds when count reaches 0
- `TestLatchWaitTimeoutFail` — WaitTimeout fails on timeout
- `TestLatchReset` — Reset() restores initial count
- `TestLatchNewNameTooLong` — NewLatch name too long fails
- `TestLatchNewNegativeCount` — NewLatch negative count fails
- `TestLatchNewAlreadyExists` — NewLatch duplicate name fails
- `TestLatchOpenNotExists` — OpenLatch non-existent name fails
- `TestLatchOpenCrossProcess` — OpenLatch sees creator's state bidirectionally
- `TestLatchConcurrentCountDown` — Concurrent CountDown and Wait from multiple goroutines
- `TestLatchCompletionPattern` — Completion use case (count=N workers)
- `TestLatchStartGatePattern` — Start gate use case (count=1)

**Tests already existing (structures_test.go):**
- `TestLatchBasic` — CountDown, Wait

### Checkpoint 8: Barrier (COMPLETE)
**Date:** 2026-11-22
**Tests added (barrier_test.go):**
- `TestBarrierName` — Name() returns correct value
- `TestBarrierNumParticipants` — NumParticipants() returns correct value
- `TestBarrierArrived` — Arrived() tracks participants waiting
- `TestBarrierGeneration` — Generation() increments each cycle
- `TestBarrierWaitTimeoutSuccess` — WaitTimeout succeeds when all arrive
- `TestBarrierWaitTimeoutFail` — WaitTimeout fails on timeout, resets Arrived
- `TestBarrierNewNameTooLong` — NewBarrier name too long fails
- `TestBarrierNewZeroParticipants` — NewBarrier zero/negative participants fails
- `TestBarrierNewAlreadyExists` — NewBarrier duplicate name fails
- `TestBarrierOpenNotExists` — OpenBarrier non-existent name fails
- `TestBarrierOpenCrossProcess` — OpenBarrier sees creator's state, both release together
- `TestBarrierConcurrent` — 5 participants synchronize at barrier
- `TestBarrierReusable` — Barrier reused across 3 cycles with generation tracking

**Tests already existing (structures_test.go):**
- `TestBarrierBasic` — Wait with 2 participants

### Checkpoint 7: Mutex (COMPLETE)
**Date:** 2026-11-22
**Tests added (mutex_test.go):**
- `TestMutexName` — Name() returns correct value
- `TestMutexTryLock` — TryLock success and failure cases
- `TestMutexIsLocked` — IsLocked() state tracking
- `TestMutexLockTimeoutSuccess` — LockTimeout succeeds when unlocked
- `TestMutexLockTimeoutFail` — LockTimeout fails on timeout
- `TestMutexNewNameTooLong` — NewMutex name too long fails
- `TestMutexNewAlreadyExists` — NewMutex duplicate name fails
- `TestMutexOpenNotExists` — OpenMutex non-existent name fails
- `TestMutexOpenNotBinary` — OpenMutex fails on non-binary semaphore
- `TestMutexOpenCrossProcess` — OpenMutex sees creator's state bidirectionally
- `TestMutexConcurrentLock` — Concurrent Lock/Unlock protects shared counter
- `TestMutexLockUnlockSequence` — Multiple lock/unlock cycles

**Tests already existing (structures_test.go):**
- `TestMutexBasic` — Lock, Unlock

### Checkpoint 6: Semaphore (COMPLETE)
**Date:** 2026-11-22
**Tests added (semaphore_test.go):**
- `TestSemaphoreName` — Name() returns correct value
- `TestSemaphoreMaxCount` — MaxCount() for bounded and unbounded semaphores
- `TestSemaphoreWaiting` — Waiting() tracks blocked goroutines
- `TestSemaphoreAcquire` — Blocking Acquire, immediate and waiting
- `TestSemaphoreAcquireTimeoutSuccess` — AcquireTimeout succeeds when permit available
- `TestSemaphoreAcquireTimeoutFail` — AcquireTimeout fails on timeout
- `TestSemaphoreReleaseExceedsMax` — Release fails when count would exceed max
- `TestSemaphoreNewNameTooLong` — NewSemaphore name too long fails
- `TestSemaphoreNewNegativeInitial` — NewSemaphore negative initial count fails
- `TestSemaphoreNewNegativeMax` — NewSemaphore negative max count fails
- `TestSemaphoreNewInitialExceedsMax` — NewSemaphore initial > max fails
- `TestSemaphoreNewAlreadyExists` — NewSemaphore duplicate name fails
- `TestSemaphoreOpenNotExists` — OpenSemaphore non-existent name fails
- `TestSemaphoreOpenCrossProcess` — OpenSemaphore sees creator's state bidirectionally
- `TestSemaphoreBinaryMode` — Binary semaphore (max=1) behavior
- `TestSemaphoreUnboundedMode` — Unbounded semaphore (max=0) allows unlimited releases
- `TestSemaphoreConcurrentAcquire` — Concurrent TryAcquire with limited permits

**Tests already existing (structures_test.go):**
- `TestSemaphoreBasic` — TryAcquire, Release, Count
- `TestSemaphoreConcurrentRelease` — Concurrent Release with max count enforcement

### Checkpoint 5: Stack[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (stack_test.go):**
- `TestStackName` — Name() returns correct value
- `TestStackTop` — Top() returns top element without modifying stack, fails on empty
- `TestStackSize` — Size() tracks elements correctly through push/pop
- `TestStackNewNameTooLong` — NewStack name too long fails
- `TestStackNewCapacityZero` — NewStack zero/negative capacity fails
- `TestStackOpenNotExists` — OpenStack non-existent name fails
- `TestStackOpenSizeMismatch` — OpenStack wrong element type fails
- `TestStackNewAlreadyExists` — NewStack duplicate name fails
- `TestStackOpenCrossProcess` — OpenStack sees creator's data bidirectionally (LIFO)
- `TestStackCapacityExact` — Capacity used as-is (no power-of-two rounding like Queue)
- `TestStackLIFOOrder` — LIFO ordering preserved

**Tests already existing (structures_test.go, crash_test.go):**
- `TestStackBasic` — Push, Pop, Empty, Full, Capacity
- `TestStackConcurrentMPMC` — MPMC concurrent access
- `TestStackCrashedPeerBoundedSpin` — Crash recovery with bounded spin

### Checkpoint 4: Queue[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (queue_test.go):**
- `TestQueueName` — Name() returns correct value
- `TestQueueTryPushTryPop` — TryPush/TryPop aliases, success and failure cases
- `TestQueueSize` — Size() tracks elements correctly through push/pop
- `TestQueueNewNameTooLong` — NewQueue name too long fails
- `TestQueueNewCapacityZero` — NewQueue zero/negative capacity fails
- `TestQueueOpenNotExists` — OpenQueue non-existent name fails
- `TestQueueOpenSizeMismatch` — OpenQueue wrong element type fails
- `TestQueueNewAlreadyExists` — NewQueue duplicate name fails
- `TestQueueOpenCrossProcess` — OpenQueue sees creator's data bidirectionally
- `TestQueueCapacityRounding` — Capacity rounds up to next power of two
- `TestQueueFIFOOrder` — FIFO ordering preserved

**Tests already existing (structures_test.go, wraparound_test.go):**
- `TestQueueBasic` — Push, Pop, Empty, Full, Capacity
- `TestQueueConcurrent` — MPMC concurrent access
- `TestQueueWraparoundAt2To32` — Counter wraparound handling

### Checkpoint 3: Array[T] (COMPLETE)
**Date:** 2026-11-22
**Tests added (array_test.go):**
- `TestArrayName` — Name() returns correct value
- `TestArrayFill` — Fill() sets all elements
- `TestArraySlice` — Slice() returns copy, modifications don't affect array
- `TestArrayCopyFrom` — CopyFrom() with equal, smaller, and larger slices
- `TestArrayRange` — Range() full iteration and early termination
- `TestArrayGetSetBounds` — Get/Set bounds checking (panic on invalid index)
- `TestArrayNewNameTooLong` — NewArray name too long fails
- `TestArrayNewCapacityZero` — NewArray zero/negative capacity fails
- `TestArrayOpenNotExists` — OpenArray non-existent name fails
- `TestArrayOpenSizeMismatch` — OpenArray wrong element type fails
- `TestArrayNewAlreadyExists` — NewArray duplicate name fails
- `TestArrayOpenCrossProcess` — OpenArray sees creator's data and vice versa

**Tests already existing (memory_test.go):**
- `TestArrayCreateAndAccess` — NewArray, OpenArray, Get, Set, Capacity
- `TestArrayInt32` — Array with int32 type

### Checkpoint 2: Table (COMPLETE)
**Date:** 2026-11-22
**Tests added (table_test.go):**
- `TestTableNewCreate` — NewTable in create mode, verifies header fields
- `TestTableNewOpen` — NewTable in validate mode, verifies entry persistence
- `TestTableHeader` — Header() method returns correct values
- `TestTableEntry` — Entry() with valid index and invalid indices (panic)
- `TestTableFind` — Find() existing and non-existing entries
- `TestTableAdd` — Add() valid entries and table-full condition
- `TestTableAddDuplicate` — Add() duplicate name fails
- `TestTableAddLongName` — Add() name too long fails
- `TestTableAllocate` — Allocate() offsets and NextOffset updates
- `TestTableAllocateAlignment` — Allocate() alignment handling
- `TestTableAllocateOverflow` — Allocate() exceeding memory size panics
- `TestTableEntryCount` — EntryCount() increments correctly
- `TestTableMaxEntries` — MaxEntries() returns correct value
- `TestTableNextOffset` — NextOffset() updates after allocation
- `TestTableEntries` — Entries() returns correct slice
- `TestCalculateTableSize` — CalculateTableSize() for various maxEntries
- `TestEntryNameString` — Entry.NameString() for normal and empty names
- `TestTableValidateBadMagic` — NewTable validate with bad magic panics
- `TestTableValidateBadVersion` — NewTable validate with bad version panics

**Tests already existing (long_names_test.go, layout_test.go):**
- `TestLongNames`, `TestHashVerification`, `TestHashCollisionResolution` — hash/name logic
- `TestSectionAlignmentLayout` — alignment layout

---

## Phase 1 Summary

**Phase 1 (Core Sync Primitives) — COMPLETE**

All 4 Phase 1 primitives have been implemented and tested:
- **Signal[T]** — Reactive signal with version tracking (12 tests)
- **Event** — AutoReset/ManualReset event (12 tests)
- **RWLock** — Reader-writer lock (13 tests)
- **Monitor** — Condition variable with predicate waiting (12 tests)

**Total Phase 1 tests:** 49

**Supporting infrastructure added:**
- `Table.Remove(name)` — removes a table entry by name, shifts subsequent entries
- `Mutex.Destroy()` / `Semaphore.Destroy()` — removes primitives from shared memory

**Status:** ✅ Phase 1 COMPLETE — Awaiting authorization to begin Phase 2 (Data Structures: Set, Map, Ring, Pool)
