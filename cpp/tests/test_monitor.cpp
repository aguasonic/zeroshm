#include <atomic>
#include <thread>
#include <unistd.h>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/array.h>
#include <zeroshm/memory.h>
#include <zeroshm/monitor.h>
#include <zeroshm/raii.h>

#include "test_config.h"

using namespace zeroshm;
using namespace zeroshm::test;

class MonitorTest : public SharedMemoryTestBase {};

TEST_F(MonitorTest, BasicNotify) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> ready(mem, "ready", 1);
   ready[0] = 0;

   std::thread waiter([&]() {
      mon.lock();
      while (ready[0] == 0) {
         mon.wait();
      }
      mon.unlock();
   });

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);

   mon.lock();
   ready[0] = 1;
   mon.notify_one();
   mon.unlock();

   waiter.join();
   EXPECT_EQ(ready[0], 1);
}

TEST_F(MonitorTest, NotifyAll) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> counter(mem, "counter", 1);
   counter[0] = 0;

   std::atomic<int> woken{0};

   std::vector<std::thread> waiters;
   for (int i = 0; i < 4; i++) {
      waiters.emplace_back([&]() {
         mon.lock();
         while (counter[0] < 10) {
            mon.wait();
         }
         woken++;
         mon.unlock();
      });
   }

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);

   mon.lock();
   counter[0] = 10;
   mon.notify_all();
   mon.unlock();

   for (auto &t : waiters) {
      t.join();
   }

   EXPECT_EQ(woken.load(), 4);
}

TEST_F(MonitorTest, PredicateWait) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> value(mem, "value", 1);
   value[0] = 0;

   std::thread waiter([&]() {
      mon.lock();
      mon.wait([&]() { return value[0] >= 5; });
      mon.unlock();
   });

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);

   // Increment and notify multiple times
   for (int i = 1; i <= 5; i++) {
      mon.lock();
      value[0] = i;
      mon.notify_one();
      mon.unlock();
      std::this_thread::sleep_for(std::chrono::milliseconds(1));
   }

   waiter.join();
   EXPECT_GE(value[0], 5);
}

TEST_F(MonitorTest, ProducerConsumer) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> buffer(mem, "buffer", 10);
   zeroshm::Array<int> count(mem, "count", 1);
   count[0] = 0;

   std::atomic<int> consumed{0};
   std::atomic<int> produced{0};
   const int NUM_ITEMS = 5;

   // Consumer
   std::thread consumer([&]() {
      for (int i = 0; i < NUM_ITEMS; i++) {
         mon.lock();
         mon.wait([&]() { return count[0] > 0; });

         int idx = --count[0];
         int value = buffer[idx];
         consumed += value;

         mon.notify_one();
         mon.unlock();
         std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
   });

   // Producer
   std::thread producer([&]() {
      for (int i = 0; i < NUM_ITEMS; i++) {
         mon.lock();
         mon.wait([&]() { return count[0] < 3; });

         int idx = count[0]++;
         buffer[idx] = i + 1;
         produced++;

         mon.notify_one();
         mon.unlock();
         std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
   });

   producer.join();
   consumer.join();

   // Sum of 1+2+...+5 = 15
   EXPECT_EQ(consumed.load(), 15);
   EXPECT_EQ(produced.load(), 5);
}

TEST_F(MonitorTest, WaitFor) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> ready(mem, "ready", 1);
   ready[0] = 0;

   // Should timeout
   mon.lock();
   bool result = mon.wait_for(std::chrono::milliseconds(10),
                              [&]() { return ready[0] == 1; });
   mon.unlock();

   EXPECT_FALSE(result);

   // Now signal
   std::thread signaler([&]() {
      std::this_thread::sleep_for(std::chrono::milliseconds(5));
      mon.lock();
      ready[0] = 1;
      mon.notify_one();
      mon.unlock();
   });

   mon.lock();
   result = mon.wait_for(std::chrono::milliseconds(100),
                         [&]() { return ready[0] == 1; });
   mon.unlock();

   signaler.join();
   EXPECT_TRUE(result);
}

TEST_F(MonitorTest, CrossProcess) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> data(mem, "data", 1);
   data[0] = 0;

   pid_t pid = fork();
   ASSERT_NE(pid, -1);

   if (pid == 0) {
      // Child - consumer
      zeroshm::Memory child_mem(shm_name_);
      zeroshm::Monitor child_mon(child_mem, "test_mon");
      zeroshm::Array<int> child_data(child_mem, "data");

      child_mon.lock();
      child_mon.wait([&]() { return child_data[0] == 42; });
      child_mon.unlock();

      exit(child_data[0] == 42 ? 0 : 1);
   } else {
      // Parent - producer
      std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);

      mon.lock();
      data[0] = 42;
      mon.notify_all();
      mon.unlock();

      int status;
      waitpid(pid, &status, 0);
      EXPECT_EQ(WEXITSTATUS(status), 0);
   }
}

TEST_F(MonitorTest, SpuriousWakeupHandling) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Monitor mon(mem, "test_mon");
   zeroshm::Array<int> value(mem, "value", 1);
   value[0] = 0;

   std::atomic<int> checks{0};

   std::thread waiter([&]() {
      mon.lock();
      mon.wait([&]() {
         checks++;
         return value[0] == 10;
      });
      mon.unlock();
   });

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);

   // Send multiple notifications before condition is true
   for (int i = 1; i <= 10; i++) {
      mon.lock();
      value[0] = i;
      mon.notify_one();
      mon.unlock();
      std::this_thread::sleep_for(std::chrono::milliseconds(1));
   }

   waiter.join();

   // Should have checked multiple times (handling spurious wakeups)
   EXPECT_GE(checks.load(), 10);
}

// Factory function tests (Phase 4.3)

TEST_F(MonitorTest, CreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Monitor::create(mem, "mon_q");
   ASSERT_TRUE(result.has_value());
   result->destroy();
}

TEST_F(MonitorTest, OpenNotFound) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Monitor::open(mem, "nonexistent");
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NotFound);
}

TEST_F(MonitorTest, RAIIWithGuard) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Monitor::create(mem, "guard_mon");
   ASSERT_TRUE(result.has_value());
   {
      SharedMemoryGuard guard(*result);
      guard->lock();
      guard->unlock();
   }
   // After guard is destroyed, the name should be removed from the table
   auto open_result = Monitor::open(mem, "guard_mon");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);
}
