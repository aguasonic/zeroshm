#include <atomic>
#include <thread>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/memory.h>
#include <zeroshm/queue.h>
#include <zeroshm/raii.h>

#include "test_config.h"

using namespace zeroshm;
using namespace zeroshm::test;

class QueueTest : public SharedMemoryTestBase {};

TEST_F(QueueTest, CreateAndBasicOps) {
   Memory mem(shm_name_, 1024 * 1024);
   Queue<int> queue(mem, "int_queue", 100);

   EXPECT_TRUE(queue.empty());
   EXPECT_FALSE(queue.full());
   EXPECT_EQ(queue.size(), 0);
   // Requested 100, rounded up to the next power of two (wrap-safety)
   EXPECT_EQ(queue.capacity(), 128);

   // Push some values
   EXPECT_TRUE(queue.push(10).has_value());
   EXPECT_TRUE(queue.push(20).has_value());
   EXPECT_TRUE(queue.push(30).has_value());

   EXPECT_FALSE(queue.empty());
   EXPECT_EQ(queue.size(), 3);

   // Pop values
   auto val = queue.pop();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 10);

   val = queue.pop();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 20);

   val = queue.pop();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 30);

   EXPECT_TRUE(queue.empty());
   EXPECT_FALSE(queue.pop().has_value());
}

TEST_F(QueueTest, FullQueue) {
   Memory mem(shm_name_, 1024 * 1024);
   // Request 3, get 4 (power-of-two rounding for wrap-safety)
   Queue<int> queue(mem, "small_queue", 3);
   EXPECT_EQ(queue.capacity(), 4);

   // Vyukov-style queue uses all capacity slots (no wasted slot)
   EXPECT_TRUE(queue.push(1).has_value());
   EXPECT_TRUE(queue.push(2).has_value());
   EXPECT_TRUE(queue.push(3).has_value());
   EXPECT_TRUE(queue.push(4).has_value());    // All 4 slots usable
   EXPECT_FALSE(queue.push(5).has_value());   // Now full

   EXPECT_TRUE(queue.full());

   ASSERT_TRUE(queue.pop().has_value());
   EXPECT_FALSE(queue.full());
   EXPECT_TRUE(queue.push(5).has_value());
}

TEST_F(QueueTest, CapacityRoundsUpToPowerOfTwo) {
   Memory mem(shm_name_, 1024 * 1024);

   Queue<int> q1(mem, "q_p2_1", 1);
   EXPECT_EQ(q1.capacity(), 1);
   Queue<int> q2(mem, "q_p2_2", 2);
   EXPECT_EQ(q2.capacity(), 2);
   Queue<int> q5(mem, "q_p2_5", 5);
   EXPECT_EQ(q5.capacity(), 8);
   Queue<int> q1000(mem, "q_p2_1000", 1000);
   EXPECT_EQ(q1000.capacity(), 1024);
}

// Regression for the 2^32 counter wraparound. head/tail increase
// monotonically and wrap; with a power-of-two capacity the slot mapping
// counter % capacity is continuous across the wrap. Seed the counters just
// below UINT32_MAX (with matching per-slot sequence numbers, seq[pos % cap]
// = pos) and stream elements across the boundary in FIFO order.
TEST_F(QueueTest, WraparoundAt2To32) {
   Memory mem(shm_name_, 1024 * 1024);
   constexpr uint32_t CAP = 8;
   Queue<uint32_t> queue(mem, "wrap32_queue", CAP);
   ASSERT_EQ(queue.capacity(), CAP);

   size_t offset = 0, size = 0;
   ASSERT_TRUE(mem.find("wrap32_queue", offset, size));
   char *base = static_cast<char *>(mem.base()) + offset;
   auto *head = reinterpret_cast<std::atomic<uint32_t> *>(base);
   auto *tail = reinterpret_cast<std::atomic<uint32_t> *>(base + 4);
   auto *seq = reinterpret_cast<std::atomic<uint32_t> *>(
       base + 16 + align_up(sizeof(uint32_t) * CAP, 8));

   // Position both counters 4 increments before the wrap.
   const uint32_t T0 = 0xFFFFFFFCu;
   head->store(T0);
   tail->store(T0);
   for (uint32_t k = 0; k < CAP; ++k) {
      uint32_t pos = T0 + k;   // wraps through 0
      seq[pos % CAP].store(pos);
   }

   // Stream 3 full generations through the queue, crossing the wrap.
   uint32_t next_in = 0, next_out = 0;
   for (int round = 0; round < 3; ++round) {
      for (uint32_t i = 0; i < CAP; ++i) {
         ASSERT_TRUE(queue.push(next_in).has_value()) << "push " << next_in;
         ++next_in;
      }
      EXPECT_TRUE(queue.full());
      for (uint32_t i = 0; i < CAP; ++i) {
         auto v = queue.pop();
         ASSERT_TRUE(v.has_value()) << "pop " << next_out;
         EXPECT_EQ(*v, next_out) << "FIFO order broken at wrap";
         ++next_out;
      }
      EXPECT_TRUE(queue.empty());
   }
   // The counters have wrapped past zero.
   EXPECT_LT(tail->load(), T0);
}

TEST_F(QueueTest, CircularWrap) {
   Memory mem(shm_name_, 1024 * 1024);
   Queue<int> queue(mem, "wrap_queue", 5);

   // Fill queue
   for (int i = 0; i < 4; i++) {
      EXPECT_TRUE(queue.push(i).has_value());
   }

   // Pop some
   auto p0 = queue.pop();
   ASSERT_TRUE(p0.has_value());
   EXPECT_EQ(*p0, 0);
   auto p1 = queue.pop();
   ASSERT_TRUE(p1.has_value());
   EXPECT_EQ(*p1, 1);

   // Push more (wrapping around)
   EXPECT_TRUE(queue.push(4).has_value());
   EXPECT_TRUE(queue.push(5).has_value());

   // Verify order
   auto p2 = queue.pop();
   ASSERT_TRUE(p2.has_value());
   EXPECT_EQ(*p2, 2);
   auto p3 = queue.pop();
   ASSERT_TRUE(p3.has_value());
   EXPECT_EQ(*p3, 3);
   auto p4 = queue.pop();
   ASSERT_TRUE(p4.has_value());
   EXPECT_EQ(*p4, 4);
   auto p5 = queue.pop();
   ASSERT_TRUE(p5.has_value());
   EXPECT_EQ(*p5, 5);
   EXPECT_TRUE(queue.empty());
}

TEST_F(QueueTest, OpenExisting) {
   Memory mem(shm_name_, 1024 * 1024);

   {
      Queue<float> queue1(mem, "float_queue", 50);
      ASSERT_TRUE(queue1.push(3.14f).has_value());
      ASSERT_TRUE(queue1.push(2.71f).has_value());
   }

   Queue<float> queue2(mem, "float_queue");
   EXPECT_EQ(queue2.capacity(), 64);   // 50 rounded up at creation
   EXPECT_EQ(queue2.size(), 2);

   auto val = queue2.pop();
   ASSERT_TRUE(val.has_value());
   EXPECT_FLOAT_EQ(*val, 3.14f);
}

TEST_F(QueueTest, ConcurrentProducerConsumer) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Queue<int> queue(mem, "concurrent_queue", 1000);

   const int num_items = 10000;
   std::atomic<int> sum_produced{0};
   std::atomic<int> sum_consumed{0};

   // Producer thread
   std::thread producer([&]() {
      for (int i = 0; i < num_items; i++) {
         while (!queue.push(i).has_value()) {
            std::this_thread::yield();
         }
         sum_produced += i;
      }
   });

   // Consumer thread
   std::thread consumer([&]() {
      int count = 0;
      while (count < num_items) {
         auto val = queue.pop();
         if (val) {
            sum_consumed += *val;
            count++;
         } else {
            std::this_thread::yield();
         }
      }
   });

   producer.join();
   consumer.join();

   EXPECT_EQ(sum_produced.load(), sum_consumed.load());
   EXPECT_TRUE(queue.empty());
}

TEST_F(QueueTest, MultipleProducersConsumers) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Queue<int> queue(mem, "mpmc_queue", 1000);

   const int num_producers = 4;
   const int num_consumers = 4;
   const int items_per_producer = 1000;

   std::atomic<int> total_produced{0};
   std::atomic<int> total_consumed{0};

   std::vector<std::thread> producers;
   std::vector<std::thread> consumers;

   // Start producers
   for (int p = 0; p < num_producers; p++) {
      producers.emplace_back([&, p]() {
         for (int i = 0; i < items_per_producer; i++) {
            int value = p * items_per_producer + i;
            while (!queue.push(value).has_value()) {
               std::this_thread::yield();
            }
            total_produced++;
         }
      });
   }

   // Start consumers
   for (int c = 0; c < num_consumers; c++) {
      consumers.emplace_back([&]() {
         while (total_consumed < num_producers * items_per_producer) {
            auto val = queue.pop();
            if (val) {
               total_consumed++;
            } else {
               std::this_thread::yield();
            }
         }
      });
   }

   // Wait for completion
   for (auto &t : producers)
      t.join();
   for (auto &t : consumers)
      t.join();

   EXPECT_EQ(total_produced.load(), num_producers * items_per_producer);
   EXPECT_EQ(total_consumed.load(), num_producers * items_per_producer);
   EXPECT_TRUE(queue.empty());
}

TEST_F(QueueTest, SpanAccess) {
   Memory mem(shm_name_, 64 * 1024);
   Queue<int> queue(mem, "span_queue", 8);

   EXPECT_TRUE(queue.push(1).has_value());
   EXPECT_TRUE(queue.push(2).has_value());
   EXPECT_TRUE(queue.push(3).has_value());

   auto span = queue.data();
   EXPECT_EQ(span.size(), 8);   // entire capacity
   EXPECT_EQ(span[0], 1);
   EXPECT_EQ(span[1], 2);
   EXPECT_EQ(span[2], 3);

   // Raw pointer compatibility
   EXPECT_EQ(queue.raw_data()[0], 1);
   EXPECT_EQ(queue.raw_data()[1], 2);

   // Const span
   const Queue<int> &cqueue = queue;
   auto cspan = cqueue.data();
   EXPECT_EQ(cspan.size(), 8);
   EXPECT_EQ(cspan[0], 1);
}

// Factory function tests (Phase 4.3)

TEST_F(QueueTest, CreateWithExpected) {
   Memory mem(shm_name_, 64 * 1024);
   auto result = Queue<int>::create(mem, "qqq", 8);
   ASSERT_TRUE(result.has_value());
   EXPECT_EQ(result->capacity(), 8);
   EXPECT_TRUE(result->push(42).has_value());
   auto val = result->pop();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 42);
   result->destroy();
}

TEST_F(QueueTest, OpenWithExpected) {
   Memory mem(shm_name_, 64 * 1024);

   // Create using factory
   auto create_result = Queue<float>::create(mem, "open_q", 16);
   ASSERT_TRUE(create_result.has_value());
   EXPECT_TRUE(create_result->push(3.14f).has_value());
   create_result->destroy();

   // Recreate (name was destroyed, so create works again)
   auto recreate = Queue<float>::create(mem, "open_q", 16);
   ASSERT_TRUE(recreate.has_value());
   EXPECT_TRUE(recreate->push(2.71f).has_value());
   recreate->destroy();

   // Create again and open
   auto create2 = Queue<float>::create(mem, "open_q", 16);
   ASSERT_TRUE(create2.has_value());
   EXPECT_TRUE(create2->push(1.41f).has_value());

   // Open existing queue
   auto open_result = Queue<float>::open(mem, "open_q");
   ASSERT_TRUE(open_result.has_value());
   EXPECT_EQ(open_result->size(), 1);
   auto val = open_result->pop();
   ASSERT_TRUE(val.has_value());
   EXPECT_FLOAT_EQ(*val, 1.41f);
}

TEST_F(QueueTest, CreateInvalidCapacity) {
   Memory mem(shm_name_, 64 * 1024);
   auto result = Queue<int>::create(mem, "qqq", 0);
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::InvalidCapacity);
}

TEST_F(QueueTest, CreateDuplicateName) {
   Memory mem(shm_name_, 64 * 1024);

   auto result1 = Queue<int>::create(mem, "dup_q", 8);
   ASSERT_TRUE(result1.has_value());

   auto result2 = Queue<int>::create(mem, "dup_q", 8);
   EXPECT_FALSE(result2.has_value());
   EXPECT_EQ(result2.error(), Error::NameAlreadyExists);
}

TEST_F(QueueTest, OpenNotFound) {
   Memory mem(shm_name_, 64 * 1024);
   auto result = Queue<int>::open(mem, "nonexistent");
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NotFound);
}

TEST_F(QueueTest, OpenTypeMismatch) {
   Memory mem(shm_name_, 64 * 1024);

   auto create_result = Queue<int>::create(mem, "type_q", 8);
   ASSERT_TRUE(create_result.has_value());

   // Try to open as double (different element size: 4 vs 8 bytes)
   auto open_result = Queue<double>::open(mem, "type_q");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::TypeMismatch);
}

TEST_F(QueueTest, DestroyAndReuse) {
   Memory mem(shm_name_, 64 * 1024);

   // Create and use
   auto result1 = Queue<int>::create(mem, "reuse_q", 8);
   ASSERT_TRUE(result1.has_value());
   EXPECT_TRUE(result1->push(42).has_value());

   // Destroy removes table entry
   result1->destroy();

   // Open fails after destroy (name removed from table)
   auto open_result = Queue<int>::open(mem, "reuse_q");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);

   // Name can be reused after destroy
   auto result2 = Queue<int>::create(mem, "reuse_q", 8);
   ASSERT_TRUE(result2.has_value());
   EXPECT_TRUE(result2->empty());

   // Destroy result2
   result2->destroy();

   // Open fails again
   auto open_result2 = Queue<int>::open(mem, "reuse_q");
   EXPECT_FALSE(open_result2.has_value());
   EXPECT_EQ(open_result2.error(), Error::NotFound);
}

TEST_F(QueueTest, RAIIWithGuard) {
   Memory mem(shm_name_, 64 * 1024);

   auto result = Queue<int>::create(mem, "guard_q", 8);
   ASSERT_TRUE(result.has_value());
   {
      SharedMemoryGuard guard(*result);
      EXPECT_TRUE(guard->push(42).has_value());
      EXPECT_EQ(guard->size(), 1);
      // guard automatically calls destroy() when it goes out of scope
   }

   // After guard is destroyed, the name should be removed from the table
   auto open_result = Queue<int>::open(mem, "guard_q");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);

   // Name can be reused
   auto result2 = Queue<int>::create(mem, "guard_q", 8);
   ASSERT_TRUE(result2.has_value());
   EXPECT_TRUE(result2->empty());
}
