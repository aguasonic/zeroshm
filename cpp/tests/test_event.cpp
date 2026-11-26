#include <atomic>
#include <chrono>
#include <thread>
#include <unistd.h>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/event.h>
#include <zeroshm/memory.h>
#include <zeroshm/raii.h>

#include "test_config.h"

using namespace zeroshm;
using namespace zeroshm::test;

class EventTest : public SharedMemoryTestBase {};

TEST_F(EventTest, AutoResetBasic) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::AutoReset);

   EXPECT_FALSE(event.is_signaled());

   // Signal the event
   event.signal();

   // Should be signaled
   EXPECT_TRUE(event.is_signaled());

   // Wait should succeed immediately and auto-reset
   event.wait();

   // Should be reset now
   EXPECT_FALSE(event.is_signaled());
}

TEST_F(EventTest, ManualResetBasic) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::ManualReset);

   EXPECT_FALSE(event.is_signaled());

   // Signal the event
   event.signal();
   EXPECT_TRUE(event.is_signaled());

   // Multiple waits should succeed
   event.wait();
   EXPECT_TRUE(event.is_signaled());   // Still signaled

   event.wait();
   EXPECT_TRUE(event.is_signaled());   // Still signaled

   // Reset
   event.reset();
   EXPECT_FALSE(event.is_signaled());
}

TEST_F(EventTest, AutoResetSingleWaiter) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::AutoReset);

   std::atomic<int> counter{0};

   std::thread waiter([&]() {
      event.wait();
      counter++;
   });

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);   // 1ms

   EXPECT_EQ(counter.load(), 0);   // Still waiting

   event.signal();

   waiter.join();

   EXPECT_EQ(counter.load(), 1);
   EXPECT_FALSE(event.is_signaled());   // Auto-reset
}

TEST_F(EventTest, ManualResetMultipleWaiters) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::ManualReset);

   std::atomic<int> counter{0};

   std::vector<std::thread> waiters;
   for (int i = 0; i < 4; i++) {
      waiters.emplace_back([&]() {
         event.wait();
         counter++;
      });
   }

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);   // 1ms

   EXPECT_EQ(counter.load(), 0);   // All still waiting

   // Signal should wake all waiters
   event.signal();

   for (auto &t : waiters) {
      t.join();
   }

   EXPECT_EQ(counter.load(), 4);       // All woke up
   EXPECT_TRUE(event.is_signaled());   // Still signaled
}

TEST_F(EventTest, AutoResetOnlyWakesOne) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::AutoReset);

   std::atomic<int> counter{0};

   std::vector<std::thread> waiters;
   for (int i = 0; i < 4; i++) {
      waiters.emplace_back([&]() {
         event.wait();
         counter++;
      });
   }

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);   // 1ms

   // Signal once - only one waiter should wake
   event.signal();
   std::this_thread::sleep_for(TestTiming::THREAD_SYNC_DELAY);   // 2ms

   EXPECT_EQ(counter.load(), 1);

   // Signal again - one more wakes
   event.signal();
   std::this_thread::sleep_for(TestTiming::THREAD_SYNC_DELAY);   // 2ms

   EXPECT_EQ(counter.load(), 2);

   // Signal remaining
   event.signal();
   event.signal();

   for (auto &t : waiters) {
      t.join();
   }

   EXPECT_EQ(counter.load(), 4);
}

TEST_F(EventTest, WaitFor) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::AutoReset);

   // Should timeout
   auto start = std::chrono::steady_clock::now();
   bool result = event.wait_for(std::chrono::milliseconds(10)).has_value();
   auto elapsed = std::chrono::steady_clock::now() - start;

   EXPECT_FALSE(result);
   EXPECT_GE(elapsed, std::chrono::milliseconds(10));

   // Now signal
   event.signal();
   result = event.wait_for(std::chrono::milliseconds(100)).has_value();
   EXPECT_TRUE(result);
}

TEST_F(EventTest, CrossProcessManualReset) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::ManualReset);

   pid_t pid = fork();
   ASSERT_NE(pid, -1);

   if (pid == 0) {
      // Child process - waits for signal
      zeroshm::Memory child_mem(shm_name_);
      zeroshm::Event child_event(child_mem, "test_event");

      child_event.wait();

      EXPECT_TRUE(child_event.is_signaled());
      exit(0);
   } else {
      // Parent process - signals
      std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);   // 1ms

      event.signal();

      int status;
      waitpid(pid, &status, 0);
      EXPECT_EQ(WEXITSTATUS(status), 0);
   }
}

TEST_F(EventTest, OpenExisting) {
   {
      zeroshm::Memory mem(shm_name_, 1024 * 1024);
      zeroshm::Event event(mem, "test_event", zeroshm::EventMode::ManualReset);
      event.signal();
   }

   // Reopen
   {
      zeroshm::Memory mem(shm_name_);
      zeroshm::Event event(mem, "test_event");

      EXPECT_TRUE(event.is_signaled());
   }
}

TEST_F(EventTest, Pulse) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Event event(mem, "test_event", zeroshm::EventMode::ManualReset);

   std::atomic<int> counter{0};

   std::vector<std::thread> waiters;
   for (int i = 0; i < 4; i++) {
      waiters.emplace_back([&]() {
         event.wait();
         counter++;
      });
   }

   std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);   // 1ms

   // Pulse: signal + reset atomically
   event.pulse();

   for (auto &t : waiters) {
      t.join();
   }

   // All waiters woke up
   EXPECT_EQ(counter.load(), 4);
   // But event is reset
   EXPECT_FALSE(event.is_signaled());
}

// Factory function tests (Phase 4.3)

TEST_F(EventTest, CreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Event::create(mem, "event_q", EventMode::AutoReset);
   ASSERT_TRUE(result.has_value());
   EXPECT_FALSE(result->is_signaled());
   result->destroy();
}

TEST_F(EventTest, OpenNotFound) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Event::open(mem, "nonexistent");
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NotFound);
}

TEST_F(EventTest, RAIIWithGuard) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Event::create(mem, "guard_event", EventMode::AutoReset);
   ASSERT_TRUE(result.has_value());
   {
      SharedMemoryGuard guard(*result);
      guard->signal();
      EXPECT_TRUE(guard->is_signaled());
   }
   // After guard is destroyed, the name should be removed from the table
   auto open_result = Event::open(mem, "guard_event");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);
}
