#include <atomic>
#include <thread>
#include <unistd.h>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/array.h>
#include <zeroshm/memory.h>
#include <zeroshm/once.h>
#include <zeroshm/raii.h>

#include "test_config.h"

using namespace zeroshm;
using namespace zeroshm::test;

class OnceTest : public SharedMemoryTestBase {};

TEST_F(OnceTest, BasicCallOnce) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Once once(mem, "test_once");

   int counter = 0;

   // Call once
   once.call([&]() { counter++; });

   EXPECT_EQ(counter, 1);

   // Call again - should not execute
   once.call([&]() { counter++; });

   EXPECT_EQ(counter, 1);   // Still 1, function not called again
}

TEST_F(OnceTest, MultiThreadCallOnce) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Once once(mem, "test_once");

   std::atomic<int> counter{0};
   std::atomic<int> total_calls{0};

   // Launch multiple threads that all try to call_once
   std::vector<std::thread> threads;
   for (int i = 0; i < TestTiming::MEDIUM_THREADS; i++) {
      threads.emplace_back([&]() {
         once.call([&]() {
            counter++;
            std::this_thread::sleep_for(TestTiming::THREAD_SYNC_DELAY);   // 2ms
         });
         total_calls++;
      });
   }

   for (auto &t : threads) {
      t.join();
   }

   // Counter incremented exactly once
   EXPECT_EQ(counter.load(), 1);
   // But all threads completed
   EXPECT_EQ(total_calls.load(), TestTiming::MEDIUM_THREADS);
}

TEST_F(OnceTest, CrossProcessCallOnce) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Once once(mem, "test_once");
   zeroshm::Array<int> shared_counter(mem, "counter", 1);
   shared_counter[0] = 0;

   pid_t pid = fork();
   ASSERT_NE(pid, -1);

   if (pid == 0) {
      // Child process
      zeroshm::Memory child_mem(shm_name_);
      zeroshm::Once child_once(child_mem, "test_once");
      zeroshm::Array<int> child_counter(child_mem, "counter");

      std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);   // 1ms

      child_once.call([&]() { child_counter[0]++; });

      exit(0);
   } else {
      // Parent process
      once.call([&]() { shared_counter[0]++; });

      int status;
      waitpid(pid, &status, 0);

      // Counter incremented exactly once across both processes
      EXPECT_EQ(shared_counter[0], 1);
   }
}

TEST_F(OnceTest, AlreadyCalledCheck) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Once once(mem, "test_once");

   EXPECT_FALSE(once.already_called());

   once.call([]() {});

   EXPECT_TRUE(once.already_called());
}

TEST_F(OnceTest, OpenExistingOnce) {
   {
      zeroshm::Memory mem(shm_name_, 1024 * 1024);
      zeroshm::Once once(mem, "test_once");

      int counter = 0;
      once.call([&]() { counter++; });

      EXPECT_EQ(counter, 1);
   }

   // Reopen
   {
      zeroshm::Memory mem(shm_name_);
      zeroshm::Once once(mem, "test_once");

      EXPECT_TRUE(once.already_called());

      int counter = 0;
      once.call([&]() { counter++; });

      EXPECT_EQ(counter, 0);   // Not called
   }
}

TEST_F(OnceTest, ExceptionSafety) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Once once(mem, "test_once");

   // First call throws
   EXPECT_THROW(
       { once.call([]() { throw std::runtime_error("test error"); }); },
       std::runtime_error);

   // Once is still marked as called (call_once semantics)
   EXPECT_TRUE(once.already_called());

   // Subsequent calls don't execute
   int counter = 0;
   once.call([&]() { counter++; });
   EXPECT_EQ(counter, 0);
}

// Factory function tests (Phase 4.3)

TEST_F(OnceTest, CreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Once::create(mem, "once_q");
   ASSERT_TRUE(result.has_value());
   EXPECT_FALSE(result->already_called());
   result->destroy();
}

TEST_F(OnceTest, OpenNotFound) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Once::open(mem, "nonexistent");
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NotFound);
}

TEST_F(OnceTest, RAIIWithGuard) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Once::create(mem, "guard_once");
   ASSERT_TRUE(result.has_value());
   {
      SharedMemoryGuard guard(*result);
      int counter = 0;
      guard->call([&]() { counter++; });
      EXPECT_EQ(counter, 1);
   }
   // After guard is destroyed, the name should be removed from the table
   auto open_result = Once::open(mem, "guard_once");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);
}
