#include <atomic>
#include <thread>
#include <unistd.h>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/array.h>
#include <zeroshm/memory.h>
#include <zeroshm/mutex.h>

#include "test_config.h"

using namespace zeroshm::test;

class MutexTest : public SharedMemoryTestBase {};

TEST_F(MutexTest, BasicLockUnlock) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Mutex mtx(mem, "test_mutex");

   // Lock
   mtx.lock();

   // Critical section
   int value = 42;
   EXPECT_EQ(value, 42);

   // Unlock
   mtx.unlock();
}

TEST_F(MutexTest, TryLock) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Mutex mtx(mem, "test_mutex");

   // Should succeed
   EXPECT_TRUE(mtx.try_lock().has_value());

   // Should fail (already locked)
   EXPECT_FALSE(mtx.try_lock().has_value());

   mtx.unlock();

   // Should succeed again
   EXPECT_TRUE(mtx.try_lock().has_value());
   mtx.unlock();
}

TEST_F(MutexTest, MutualExclusion) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Mutex mtx(mem, "test_mutex");
   zeroshm::Array<int> counter(mem, "counter", 1);
   counter[0] = 0;

   std::vector<std::thread> threads;
   for (int i = 0; i < TestTiming::MEDIUM_THREADS; i++) {
      threads.emplace_back([&]() {
         for (int j = 0; j < TestTiming::FAST_ITERATIONS; j++) {
            mtx.lock();
            counter[0]++;
            mtx.unlock();
         }
      });
   }

   for (auto &t : threads) {
      t.join();
   }

   int expected = TestTiming::MEDIUM_THREADS * TestTiming::FAST_ITERATIONS;
   EXPECT_EQ(counter[0], expected);
}

TEST_F(MutexTest, LockGuard) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Mutex mtx(mem, "test_mutex");

   {
      std::lock_guard<zeroshm::Mutex> lock(mtx);
      // Critical section
      int value = 42;
      EXPECT_EQ(value, 42);
   }   // lock automatically released

   // Should be able to lock again
   EXPECT_TRUE(mtx.try_lock().has_value());
   mtx.unlock();
}

TEST_F(MutexTest, CrossProcessMutex) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Mutex mtx(mem, "test_mutex");
   zeroshm::Array<int> counter(mem, "counter", 1);
   counter[0] = 0;

   pid_t pid = fork();
   ASSERT_NE(pid, -1);

   if (pid == 0) {
      // Child process
      zeroshm::Memory child_mem(shm_name_);
      zeroshm::Mutex child_mtx(child_mem, "test_mutex");
      zeroshm::Array<int> child_counter(child_mem, "counter");

      for (int i = 0; i < 100; i++) {
         child_mtx.lock();
         child_counter[0]++;
         child_mtx.unlock();
      }
      exit(0);
   } else {
      // Parent process
      for (int i = 0; i < 100; i++) {
         mtx.lock();
         counter[0]++;
         mtx.unlock();
      }

      int status;
      waitpid(pid, &status, 0);

      EXPECT_EQ(counter[0], 200);   // Both processes incremented
   }
}

TEST_F(MutexTest, Contention) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::Mutex mtx(mem, "test_mutex");
   zeroshm::Array<int> counter(mem, "counter", 1);
   counter[0] = 0;

   std::atomic<int> attempts{0};

   std::vector<std::thread> threads;
   for (int i = 0; i < 8; i++) {
      threads.emplace_back([&]() {
         for (int j = 0; j < 50; j++) {
            attempts++;
            mtx.lock();
            counter[0]++;
            std::this_thread::sleep_for(std::chrono::microseconds(10));
            mtx.unlock();
         }
      });
   }

   for (auto &t : threads) {
      t.join();
   }

   EXPECT_EQ(counter[0], 400);   // 8 threads * 50 iterations
   EXPECT_EQ(attempts.load(), 400);
}

TEST_F(MutexTest, OpenExisting) {
   {
      zeroshm::Memory mem(shm_name_, 1024 * 1024);
      zeroshm::Mutex mtx(mem, "test_mutex");
      mtx.lock();
      mtx.unlock();
   }

   // Reopen
   {
      zeroshm::Memory mem(shm_name_);
      zeroshm::Mutex mtx(mem, "test_mutex");

      // Should be able to lock
      EXPECT_TRUE(mtx.try_lock().has_value());
      mtx.unlock();
   }
}
