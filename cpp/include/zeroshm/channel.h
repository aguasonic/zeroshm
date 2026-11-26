#pragma once

#include <atomic>
#include <bit>
#include <chrono>
#include <cstddef>
#include <expected>
#include <format>
#include <optional>

#include <zeroshm/constants_generated.h>

#include "error.h"
#include "memory.h"
#include "queue.h"

namespace zeroshm {

/**
 * @brief CSP-style channel for synchronous message passing between processes
 *
 * Channel<T> implements Communicating Sequential Processes (CSP) semantics,
 * where processes communicate through synchronous message passing. Unlike
 * queues which are asynchronous, channels provide rendezvous semantics where
 * send blocks until receive, creating a synchronization point.
 *
 * @motivation
 * Channels solve coordination problems in concurrent systems:
 * - Synchronization: Sender and receiver meet at the channel
 * - Deadlock prevention: Can use select() across multiple channels
 * - Composability: Channels can be passed as values
 * - Type safety: Strongly typed communication
 * - Back-pressure: Natural flow control as senders block
 *
 * @theory
 * Based on Tony Hoare's CSP and implemented in languages like Go and Rust,
 * channels provide a higher-level abstraction than shared memory. They
 * enforce "Don't communicate by sharing memory; share memory by communicating."
 *
 * In our shared memory context, we implement both:
 * - Unbuffered channels: Pure synchronous rendezvous
 * - Buffered channels: Asynchronous up to buffer capacity
 *
 * @warning The unbuffered rendezvous supports exactly ONE sender and ONE
 * receiver at a time. The slot claim uses plain flag reads, so concurrent
 * senders can overwrite each other's value and concurrent receivers can
 * both take the same value. Buffered channels (queue-backed) are MPMC-safe.
 *
 * @example
 * ```cpp
 * // Unbuffered channel for synchronization
 * Memory mem("/comms", 1024*1024);
 * Channel<int> ch(mem, "sync_channel");  // capacity=0 means unbuffered
 *
 * // Process A: Send (blocks until received)
 * ch.send(42);
 *
 * // Process B: Receive (blocks until sent)
 * if (auto val = ch.recv()) {
 *     process(*val);
 * }
 *
 * // Buffered channel for async communication
 * Channel<Message> msgs(mem, "msg_channel", 100);  // buffer 100 messages
 *
 * // Non-blocking try operations
 * if (msgs.try_send(msg)) {
 *     // Sent successfully
 * }
 *
 * // Timeout operations
 * if (auto msg = msgs.recv_timeout(500ms)) {
 *     handle(*msg);
 * }
 *
 * // Select across multiple channels (Go-style)
 * select(
 *     ch1.on_recv([](int x) { handle_int(x); }),
 *     ch2.on_recv([](double y) { handle_double(y); }),
 *     timeout(1s, []() { handle_timeout(); })
 * );
 * ```
 *
 * @tparam T Type of channel messages (must be trivially copyable)
 */
template <typename T> class Channel {
 public:
   static_assert(std::is_trivially_copyable_v<T>,
                 "T must be trivially copyable for shared memory");

   struct Header {
      std::atomic<uint32_t> capacity;    // 0 = unbuffered
      std::atomic<uint32_t> senders;     // Waiting senders
      std::atomic<uint32_t> receivers;   // Waiting receivers
      std::atomic<bool> closed;          // Channel closed
      std::atomic<uint64_t> send_seq;    // Send sequence for ordering
      std::atomic<uint64_t> recv_seq;    // Receive sequence for ordering
   };

   /// Compile-time validation: Channel::Header must be exactly 32 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == 32,
                 "Channel::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, capacity) == CHANNEL_CAPACITY_OFFSET,
                 "Channel::Header.capacity must be at CHANNEL_CAPACITY_OFFSET");
   static_assert(offsetof(Header, senders) == CHANNEL_SENDERS_OFFSET,
                 "Channel::Header.senders must be at CHANNEL_SENDERS_OFFSET");
   static_assert(offsetof(Header, receivers) == CHANNEL_RECEIVERS_OFFSET,
                 "Channel::Header.receivers must be at CHANNEL_RECEIVERS_OFFSET");
   static_assert(offsetof(Header, closed) == CHANNEL_CLOSED_OFFSET,
                 "Channel::Header.closed must be at CHANNEL_CLOSED_OFFSET");
   static_assert(offsetof(Header, send_seq) == CHANNEL_SEND_SEQ_OFFSET,
                 "Channel::Header.send_seq must be at CHANNEL_SEND_SEQ_OFFSET");
   static_assert(offsetof(Header, recv_seq) == CHANNEL_RECV_SEQ_OFFSET,
                 "Channel::Header.recv_seq must be at CHANNEL_RECV_SEQ_OFFSET");

   struct RendezvousSlot {
      std::atomic<bool> ready;      // Data is ready
      std::atomic<bool> consumed;   // Data was consumed
      T data;                       // The actual data
   };

 private:
   struct FactoryTag {};

   /// Private constructor used by factory functions and delegating
   /// constructors. Leaves pointers uninitialized; callers must call
   /// init_create() or init_open().
   Channel(Memory &memory, std::string_view name, FactoryTag)
       : memory_(memory), name_(name), header_(nullptr), slot_(nullptr),
         buffer_(nullptr), capacity_(0) {
   }

   /// Core initialization logic for creating a new channel.
   std::expected<void, Error> init_create(size_t capacity) {
      // The buffer is a Queue, whose capacity rounds up to a power of two
      // (wrap-safety). Round here too so capacity() and the buffer agree.
      if (capacity > 0) {
         capacity = std::bit_ceil(capacity);
      }
      capacity_ = capacity;

      size_t header_size = sizeof(Header);
      std::string header_name = std::string(name_) + "_header";
      auto header_alloc = memory_.allocate(header_name, header_size);
      if (!header_alloc) {
         return std::unexpected(header_alloc.error());
      }
      size_t offset = *header_alloc;

      header_ = memory_.ptr_at<Header>(offset);

      // Initialize header
      header_->capacity.store(capacity, std::memory_order_relaxed);
      header_->senders.store(0, std::memory_order_relaxed);
      header_->receivers.store(0, std::memory_order_relaxed);
      header_->closed.store(false, std::memory_order_relaxed);
      header_->send_seq.store(0, std::memory_order_relaxed);
      header_->recv_seq.store(0, std::memory_order_relaxed);

      if (capacity > 0) {
         // Buffered channel uses queue (Vyukov queue uses all N slots)
         auto buffer_result = Queue<T>::create(
             memory_, std::string(name_) + "_buffer", capacity);
         if (!buffer_result) {
            // Clean up header on failure
            (void)memory_.deallocate(header_name);
            return std::unexpected(buffer_result.error());
         }
         buffer_ = std::make_unique<Queue<T>>(std::move(*buffer_result));
      } else {
         // Unbuffered uses rendezvous slot
         size_t slot_size = sizeof(RendezvousSlot);
         std::string slot_name = std::string(name_) + "_slot";
         auto slot_alloc = memory_.allocate(slot_name, slot_size);
         if (!slot_alloc) {
            // Clean up header on failure
            (void)memory_.deallocate(header_name);
            return std::unexpected(slot_alloc.error());
         }
         size_t slot_offset = *slot_alloc;

         slot_ = memory_.ptr_at<RendezvousSlot>(slot_offset);

         slot_->ready.store(false, std::memory_order_relaxed);
         slot_->consumed.store(false, std::memory_order_relaxed);
      }

      return {};
   }

   /// Core initialization logic for opening an existing channel.
   std::expected<void, Error> init_open() {
      size_t offset, size;
      std::string header_name = std::string(name_) + "_header";
      if (!memory_.find(header_name, offset, size)) {
         return std::unexpected(Error::NotFound);
      }

      header_ = memory_.ptr_at<Header>(offset);

      capacity_ = header_->capacity.load(std::memory_order_relaxed);

      if (capacity_ > 0) {
         // Open existing queue (already has correct size)
         auto buffer_result =
             Queue<T>::open(memory_, std::string(name_) + "_buffer");
         if (!buffer_result) {
            return std::unexpected(buffer_result.error());
         }
         buffer_ = std::make_unique<Queue<T>>(std::move(*buffer_result));
      } else {
         std::string slot_name = std::string(name_) + "_slot";
         if (!memory_.find(slot_name, offset, size)) {
            return std::unexpected(Error::NotFound);
         }
         slot_ = memory_.ptr_at<RendezvousSlot>(offset);
      }

      return {};
   }

   Memory &memory_;
   std::string name_;
   Header *header_ = nullptr;
   RendezvousSlot *slot_ = nullptr;
   std::unique_ptr<Queue<T>> buffer_;
   size_t capacity_;

   template <typename Predicate>
   bool wait_while(Predicate pred, std::atomic<uint32_t> &counter) {
      while (pred()) {
         if (header_->closed.load(std::memory_order_acquire)) {
            counter.fetch_sub(1, std::memory_order_acq_rel);
            return false;
         }
         std::this_thread::yield();
      }
      return true;
   }

   bool rendezvous_send(const T &value) {
      // Indicate we're a waiting sender
      header_->senders.fetch_add(1, std::memory_order_acq_rel);

      // Wait for slot to be free
      if (!wait_while([this] { return slot_->ready.load(std::memory_order_acquire); },
                      header_->senders)) {
         return false;
      }

      // Place data
      slot_->data = value;
      slot_->consumed.store(false, std::memory_order_relaxed);
      slot_->ready.store(true, std::memory_order_release);

      // Wait for receiver to consume
      if (!wait_while([this] { return !slot_->consumed.load(std::memory_order_acquire); },
                      header_->senders)) {
         return false;
      }

      header_->senders.fetch_sub(1, std::memory_order_acq_rel);
      return true;
   }

   std::optional<T> rendezvous_recv() {
      // Indicate we're a waiting receiver
      header_->receivers.fetch_add(1, std::memory_order_acq_rel);

      // Wait for data
      if (!wait_while([this] { return !slot_->ready.load(std::memory_order_acquire); },
                      header_->receivers)) {
         return std::nullopt;
      }

      // Get data
      T value = slot_->data;

      // Mark as consumed
      slot_->consumed.store(true, std::memory_order_release);
      slot_->ready.store(false, std::memory_order_release);

      header_->receivers.fetch_sub(1, std::memory_order_acq_rel);
      return value;
   }

   bool has_data() const {
      if (capacity_ > 0 && buffer_) {
         return !buffer_->empty();
      }
      return slot_->ready.load(std::memory_order_acquire);
   }

 public:
   /**
    * @brief Create unbuffered synchronous channel
    */
   Channel(Memory &memory, std::string_view name)
       : Channel(memory, name, FactoryTag{}) {
      auto result = init_create(0);
      if (!result) {
         throw std::runtime_error(std::format("Failed to create channel: {}",
                                              error_message(result.error())));
      }
   }

   /**
    * @brief Create buffered channel with specified capacity
    * @param capacity Buffer size (0 for unbuffered/synchronous)
    */
   Channel(Memory &memory, std::string_view name, size_t capacity)
       : Channel(memory, name, FactoryTag{}) {
      auto result = init_create(capacity);
      if (!result) {
         throw std::runtime_error(std::format("Failed to create channel: {}",
                                              error_message(result.error())));
      }
   }

   /**
    * @brief Open existing channel
    */
   Channel(Memory &memory, std::string_view name,
           [[maybe_unused]] bool open_existing)
       : Channel(memory, name, FactoryTag{}) {
      auto result = init_open();
      if (!result) {
         switch (result.error()) {
         case Error::NotFound:
            throw std::runtime_error(
                std::format("Channel not found: {}", name));
         case Error::TypeMismatch:
            throw std::runtime_error("Channel type size mismatch");
         default:
            throw std::runtime_error(std::format(
                "Failed to open channel: {}", error_message(result.error())));
         }
      }
   }

   /// Create a new channel in shared memory.
   /// Returns Error::AllocationOverflow or Error::BoundsExceeded if allocation
   /// fails, Error::NameAlreadyExists if the name is already in use.
   static std::expected<Channel, Error>
   create(Memory &memory, std::string_view name, size_t capacity) {
      Channel ch(memory, name, FactoryTag{});
      auto result = ch.init_create(capacity);
      if (!result)
         return std::unexpected(result.error());
      return ch;
   }

   /// Open an existing channel in shared memory.
   /// Returns Error::NotFound if the name doesn't exist,
   /// Error::TypeMismatch if the element type size doesn't match.
   static std::expected<Channel, Error> open(Memory &memory,
                                             std::string_view name) {
      Channel ch(memory, name, FactoryTag{});
      auto result = ch.init_open();
      if (!result)
         return std::unexpected(result.error());
      return ch;
   }

   /**
    * @brief Send value (blocks until received or buffered)
    */
   [[nodiscard]] bool send(const T &value) {
      if (header_->closed.load(std::memory_order_acquire)) {
         return false;
      }

      if (capacity_ > 0) {
         // Buffered send
         return buffer_->push(value).has_value();
      } else {
         // Unbuffered: synchronous rendezvous
         return rendezvous_send(value);
      }
   }

   /**
    * @brief Try to send without blocking
    * @return expected<void, Error> -- success if sent, ChannelClosed if closed,
    * ChannelFull if buffer full or no receiver
    */
   [[nodiscard]] std::expected<void, Error> try_send(const T &value) {
      if (header_->closed.load(std::memory_order_acquire)) {
         return std::unexpected(Error::ChannelClosed);
      }

      if (capacity_ > 0 && buffer_) {
         auto result = buffer_->push(value);
         if (!result) {
            return std::unexpected(Error::ChannelFull);
         }
         return {};
      } else {
         // For unbuffered, check if receiver is waiting
         if (header_->receivers.load(std::memory_order_acquire) > 0) {
            if (rendezvous_send(value)) {
               return {};
            }
         }
         return std::unexpected(Error::ChannelFull);
      }
   }

   /**
    * @brief Send with timeout
    */
   [[nodiscard]] bool send_timeout(const T &value,
                                   std::chrono::milliseconds timeout) {
      auto deadline = std::chrono::steady_clock::now() + timeout;

      while (std::chrono::steady_clock::now() < deadline) {
         if (try_send(value)) {
            return true;
         }
         if (header_->closed.load(std::memory_order_acquire)) {
            return false;
         }
         std::this_thread::yield();
      }

      return false;
   }

   /**
    * @brief Receive value.
    *
    * Buffered channels: NON-blocking; returns nullopt when the buffer is
    * empty (use recv_timeout for a bounded wait). Unbuffered channels:
    * blocks in the rendezvous until a sender arrives.
    */
   [[nodiscard]] std::optional<T> recv() {
      if (capacity_ > 0 && buffer_) {
         // Buffered receive
         // For buffered channels, pop() is non-blocking
         auto result = buffer_->pop();
         return result ? std::optional<T>(*result) : std::nullopt;
      } else {
         // Unbuffered: synchronous rendezvous
         return rendezvous_recv();
      }
   }

   /**
    * @brief Try to receive without blocking
    * @return expected<T, Error> -- value if available, ChannelClosed if closed
    * and empty, ChannelEmpty if no data
    */
   [[nodiscard]] std::expected<T, Error> try_recv() {
      if (capacity_ > 0 && buffer_) {
         auto result = buffer_->pop();
         if (result) {
            return *result;
         }
         if (header_->closed.load(std::memory_order_acquire)) {
            return std::unexpected(Error::ChannelClosed);
         }
         return std::unexpected(Error::ChannelEmpty);
      } else {
         // For unbuffered, check if sender is waiting
         if (slot_->ready.load(std::memory_order_acquire)) {
            auto result = rendezvous_recv();
            if (result) {
               return *result;
            }
            return std::unexpected(Error::ChannelClosed);
         }
         if (header_->closed.load(std::memory_order_acquire)) {
            return std::unexpected(Error::ChannelClosed);
         }
         return std::unexpected(Error::ChannelEmpty);
      }
   }

   /**
    * @brief Receive with timeout
    */
   [[nodiscard]] std::optional<T>
   recv_timeout(std::chrono::milliseconds timeout) {

      auto deadline = std::chrono::steady_clock::now() + timeout;

      while (std::chrono::steady_clock::now() < deadline) {
         if (auto val = try_recv()) {
            return std::optional<T>(*val);
         }
         if (header_->closed.load(std::memory_order_acquire) && !has_data()) {
            return std::nullopt;
         }
         std::this_thread::yield();
      }

      return std::nullopt;
   }

   /**
    * @brief Close the channel
    */
   void close() {
      header_->closed.store(true, std::memory_order_release);
      // Wake all waiting senders and receivers
   }

   /**
    * @brief Check if channel is closed
    */
   [[nodiscard]] bool is_closed() const {
      return header_->closed.load(std::memory_order_acquire);
   }

   /**
    * @brief Get channel capacity (0 for unbuffered)
    */
   [[nodiscard]] size_t capacity() const {
      return capacity_;
   }

   /**
    * @brief Check if channel is buffered
    */
   [[nodiscard]] bool is_buffered() const {
      return capacity_ > 0;
   }

   /**
    * @brief Iterator support for range-based for loops
    */
   class iterator {
    private:
      Channel *channel_;
      std::optional<T> current_;

    public:
      iterator(Channel *ch) : channel_(ch) {
         if (ch) {
            current_ = ch->recv();
         }
      }

      iterator &operator++() {
         if (channel_) {
            current_ = channel_->recv();
         }
         return *this;
      }

      T operator*() const {
         return *current_;
      }

      bool operator!=(const iterator &other) const {
         return current_.has_value() != other.current_.has_value();
      }
   };

   iterator begin() {
      return iterator(this);
   }
   iterator end() {
      return iterator(nullptr);
   }

   /**
    * @brief Destroy the channel and remove it from the shared memory table.
    *
    * Destroys the backing buffer (Queue) or rendezvous slot, and removes
    * the header table entry so the names can be reused.
    */
   void destroy() {
      if (buffer_) {
         buffer_->destroy();
      } else if (slot_) {
         (void)memory_.deallocate(name_ + "_slot");
      }
      (void)memory_.deallocate(name_ + "_header");
   }
};

}   // namespace zeroshm
