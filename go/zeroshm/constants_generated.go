package zeroshm

// Code generated from constants.pkl - DO NOT EDIT
// Run 'make generate' to regenerate.

const (
	// Barrier header: arrived field offset
	BarrierArrivedOffset = 0
	// Barrier header: generation field offset
	BarrierGenerationOffset = 4
	// Barrier header: num participants field offset
	BarrierNumParticipantsOffset = 8
	// Barrier header: padding field offset
	BarrierPaddingOffset = 12
	// Channel header: capacity field offset
	ChannelCapacityOffset = 0
	// Channel header: closed field offset
	ChannelClosedOffset = 12
	// Channel header: receivers field offset
	ChannelReceiversOffset = 8
	// Channel header: receive sequence field offset
	ChannelRecvSeqOffset = 24
	// Channel header: send sequence field offset
	ChannelSendSeqOffset = 16
	// Channel header: senders field offset
	ChannelSendersOffset = 4
	// Size of each table entry in bytes
	EntrySize = 384
	// Event state: mode field offset
	EventModeOffset = 4
	// Event state: signaled field offset
	EventSignaledOffset = 0
	// Event state: waiting field offset
	EventWaitingOffset = 8
	// Future header: completion time field offset
	FutureCompletionTimeOffset = 8
	// Future header: state field offset
	FutureStateOffset = 0
	// Future header: waiters field offset
	FutureWaitersOffset = 4
	// Size of the table header in bytes
	HeaderSize = 32
	// Latch header: count field offset
	LatchCountOffset = 0
	// Latch header: initial count field offset
	LatchInitialCountOffset = 4
	// Latch header: padding 0 field offset
	LatchPadding0Offset = 8
	// Latch header: padding 1 field offset
	LatchPadding1Offset = 12
	// Lazy header: completion time field offset
	LazyCompletionTimeOffset = 8
	// Lazy header: compute count field offset
	LazyComputeCountOffset = 4
	// Lazy header: state field offset
	LazyStateOffset = 0
	// Magic number identifying a ZeroSHM table header ('ZSHM')
	TableMagic uint32 = 0x5A53484D
	// Map header: capacity field offset
	MapCapacityOffset = 4
	// Map header: key size field offset
	MapKeySizeOffset = 8
	// Map header: size field offset
	MapSizeOffset = 0
	// Map header: value size field offset
	MapValueSizeOffset = 12
	// Name field size in table entries (includes null terminator + padding)
	NameFieldSize = 256
	// Once flag: state field offset
	OnceStateOffset = 0
	// Pool header: allocated field offset
	PoolAllocatedOffset = 8
	// Pool header: capacity field offset
	PoolCapacityOffset = 16
	// Pool header: element size field offset
	PoolElemSizeOffset = 20
	// Pool header: free head field offset
	PoolFreeHeadOffset = 0
	// Pool header: padding field offset
	PoolPaddingOffset = 12
	// Queue header: capacity field offset
	QueueCapacityOffset = 8
	// Queue header: element size field offset
	QueueElemSizeOffset = 12
	// Queue header: head field offset
	QueueHeadOffset = 0
	// Queue header: tail field offset
	QueueTailOffset = 4
	// Ring header: capacity field offset
	RingCapacityOffset = 16
	// Ring header: element size field offset
	RingElemSizeOffset = 20
	// Ring header: read position field offset
	RingReadPosOffset = 8
	// Ring header: write position field offset
	RingWritePosOffset = 0
	// RWLock state: readers field offset
	RwlockReadersOffset = 0
	// RWLock state: writer active field offset
	RwlockWriterActiveOffset = 4
	// Section alignment (8-byte boundaries for all structure sections)
	SectionAlign = 8
	// Semaphore header: count field offset
	SemaphoreCountOffset = 0
	// Semaphore header: max count field offset
	SemaphoreMaxCountOffset = 8
	// Semaphore header: padding field offset
	SemaphorePaddingOffset = 12
	// Semaphore header: waiting field offset
	SemaphoreWaitingOffset = 4
	// Set header: capacity field offset
	SetCapacityOffset = 4
	// Set header: element size field offset
	SetElemSizeOffset = 8
	// Set header: reserved field offset
	SetReservedOffset = 12
	// Set header: size field offset
	SetSizeOffset = 0
	// Stack header: capacity field offset
	StackCapacityOffset = 4
	// Stack header: element size field offset
	StackElemSizeOffset = 8
	// Stack header: reserved field offset
	StackReservedOffset = 12
	// Stack header: top field offset
	StackTopOffset = 0
	// Stream header: buffer capacity field offset
	StreamBufferCapacityOffset = 16
	// Stream header: closed field offset
	StreamClosedOffset = 12
	// Stream header: padding field offset
	StreamPaddingOffset = 20
	// Stream header: sequence field offset
	StreamSequenceOffset = 0
	// Stream header: subscribers field offset
	StreamSubscribersOffset = 8
	// Stream header: transform name field offset
	StreamTransformNameOffset = 24
	// Table header: entry count field offset
	TableEntryCountOffset = 8
	// Table header: magic field offset
	TableMagicOffset = 0
	// Table header: max entries field offset
	TableMaxEntriesOffset = 12
	// Table header: memory size field offset
	TableMemorySizeOffset = 16
	// Table header: next offset field
	TableNextOffsetOffset = 24
	// Table header: version field offset
	TableVersionOffset = 4
	// Size of u32 primitive
	U32Size = 4
	// Size of u64 primitive
	U64Size = 8
	// Current table format version
	TableVersion uint32 = 3
	// Seed value for xxhash3-64 name hashing
	XXHashSeed uint64 = 0x2FE7
)
