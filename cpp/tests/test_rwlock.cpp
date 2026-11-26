#include <atomic>
#include <thread>
#include <unistd.h>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/array.h>
#include <zeroshm/memory.h>
#include <zeroshm/raii.h>
#include <zeroshm/rwlock.h>

#include "test_config.h"

using namespace zeroshm;
using namespace zeroshm::test;

class RWLockTest : public SharedMemoryTestBase {};

TEST_F(RWLockTest, BasicReaderLock) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");

   rwlock.reader_lock();
   // Critical read section
   int value = 42;
   EXPECT_EQ(value, 42);
   rwlock.reader_unlock();
}

TEST_F(RWLockTest, BasicWriterLock) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");

   rwlock.writer_lock();
   // Critical write section
   int value = 42;
   EXPECT_EQ(value, 42);
   rwlock.writer_unlock();
}

TEST_F(RWLockTest, MultipleReadersAllowed) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");
   zeroshm::Array<int> data(mem, "data", 1);
   data[0] = 100;

   std::atomic<int> concurrent_readers{0};
   std::atomic<int> max_concurrent{0};

   std::vector<std::thread> readers;
   for (int i = 0; i < 8; i++) {
      readers.emplace_back([&]() {
         rwlock.reader_lock();

         int current = concurrent_readers.fetch_add(1) + 1;

         // Update max
         int expected_max = max_concurrent.load();
         while (current > expected_max &&
                !max_concurrent.compare_exchange_weak(expected_max, current))
            ;

         // Read data
         int value = data[0];
         EXPECT_EQ(value, 100);

         std::this_thread::sleep_for(std::chrono::milliseconds(5));

         concurrent_readers.fetch_sub(1);
         rwlock.reader_unlock();
      });
   }

   for (auto &t : readers) {
      t.join();
   }

   // Multiple readers should have been concurrent
   EXPECT_GT(max_concurrent.load(), 1);
}

TEST_F(RWLockTest, WriterExcludesReaders) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");
   zeroshm::Array<int> data(mem, "data", 1);
   data[0] = 0;

   std::atomic<bool> writer_active{false};
   std::atomic<bool> reader_saw_writer{false};

   std::thread writer([&]() {
      std::this_thread::sleep_for(TestTiming::THREAD_START_DELAY);

      rwlock.writer_lock();
      writer_active = true;
      data[0] = 42;
      std::this_thread::sleep_for(TestTiming::THREAD_SYNC_DELAY);
      writer_active = false;
      rwlock.writer_unlock();
   });

   std::thread reader([&]() {
      std::this_thread::sleep_for(std::chrono::milliseconds(2));

      rwlock.reader_lock();
      if (writer_active) {
         reader_saw_writer = true;
      }
      int value = data[0];
      (void)value;
      rwlock.reader_unlock();

      EXPECT_FALSE(reader_saw_writer);   // Writer should be exclusive
   });

   writer.join();
   reader.join();
}

TEST_F(RWLockTest, WriterExcludesWriters) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");
   zeroshm::Array<int> counter(mem, "counter", 1);
   counter[0] = 0;

   std::vector<std::thread> writers;
   for (int i = 0; i < 4; i++) {
      writers.emplace_back([&]() {
         for (int j = 0; j < 25; j++) {
            rwlock.writer_lock();
            counter[0]++;
            rwlock.writer_unlock();
         }
      });
   }

   for (auto &t : writers) {
      t.join();
   }

   EXPECT_EQ(counter[0], 100);
}

TEST_F(RWLockTest, ReaderWriterCoordination) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");
   zeroshm::Array<int> data(mem, "data", 1);
   data[0] = 0;

   std::atomic<int> reads{0};
   std::atomic<int> writes{0};

   // Multiple readers
   std::vector<std::thread> readers;
   for (int i = 0; i < 4; i++) {
      readers.emplace_back([&]() {
         for (int j = 0; j < 10; j++) {
            rwlock.reader_lock();
            int value = data[0];
            (void)value;
            reads++;
            rwlock.reader_unlock();
            std::this_thread::sleep_for(std::chrono::microseconds(100));
         }
      });
   }

   // Single writer
   std::thread writer([&]() {
      for (int i = 0; i < 10; i++) {
         rwlock.writer_lock();
         data[0]++;
         writes++;
         rwlock.writer_unlock();
         std::this_thread::sleep_for(std::chrono::microseconds(100));
      }
   });

   for (auto &t : readers) {
      t.join();
   }
   writer.join();

   EXPECT_EQ(reads.load(), 40);
   EXPECT_EQ(writes.load(), 10);
   EXPECT_EQ(data[0], 10);
}

TEST_F(RWLockTest, SharedLockGuard) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");

   {
      zeroshm::SharedLock lock(rwlock);
      // Read operations
      int value = 42;
      EXPECT_EQ(value, 42);
   }   // Automatically unlocked

   // Should be able to acquire write lock
   rwlock.writer_lock();
   rwlock.writer_unlock();
}

TEST_F(RWLockTest, UniqueLockGuard) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");

   {
      zeroshm::UniqueLock lock(rwlock);
      // Write operations
      int value = 42;
      EXPECT_EQ(value, 42);
   }   // Automatically unlocked

   // Should be able to acquire read lock
   rwlock.reader_lock();
   rwlock.reader_unlock();
}

TEST_F(RWLockTest, CrossProcess) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");
   zeroshm::Array<int> data(mem, "data", 1);
   data[0] = 0;

   pid_t pid = fork();
   ASSERT_NE(pid, -1);

   if (pid == 0) {
      // Child - reader
      zeroshm::Memory child_mem(shm_name_);
      zeroshm::RWLock child_rwlock(child_mem, "test_rwlock");
      zeroshm::Array<int> child_data(child_mem, "data");

      for (int i = 0; i < 50; i++) {
         child_rwlock.reader_lock();
         int value = child_data[0];
         (void)value;
         child_rwlock.reader_unlock();
      }
      exit(0);
   } else {
      // Parent - writer
      for (int i = 0; i < 50; i++) {
         rwlock.writer_lock();
         data[0]++;
         rwlock.writer_unlock();
      }

      int status;
      waitpid(pid, &status, 0);
      EXPECT_EQ(data[0], 50);
   }
}

TEST_F(RWLockTest, TryReaderLock) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");

   // Should succeed when unlocked
   EXPECT_TRUE(rwlock.try_reader_lock().has_value());
   rwlock.reader_unlock();

   // Should succeed even with other readers
   rwlock.reader_lock();
   EXPECT_TRUE(rwlock.try_reader_lock().has_value());
   rwlock.reader_unlock();
   rwlock.reader_unlock();

   // Should fail when writer holds lock
   rwlock.writer_lock();
   EXPECT_FALSE(rwlock.try_reader_lock().has_value());
   rwlock.writer_unlock();
}

TEST_F(RWLockTest, TryWriterLock) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "test_rwlock");

   // Should succeed when unlocked
   EXPECT_TRUE(rwlock.try_writer_lock().has_value());
   rwlock.writer_unlock();

   // Should fail when reader holds lock
   rwlock.reader_lock();
   EXPECT_FALSE(rwlock.try_writer_lock().has_value());
   rwlock.reader_unlock();

   // Should fail when writer holds lock
   rwlock.writer_lock();
   EXPECT_FALSE(rwlock.try_writer_lock().has_value());
   rwlock.writer_unlock();
}

// Regression for the reader/writer mutual-exclusion race: readers checked
// writer_active under reader_mutex_, but writers set writer_active and
// sampled readers without it, so a reader and a writer could both acquire
// the lock. Writers keep an invariant pair equal; readers assert it.
TEST_F(RWLockTest, ReaderWriterMutualExclusionStress) {
   zeroshm::Memory mem(shm_name_, 1024 * 1024);
   zeroshm::RWLock rwlock(mem, "rw_excl");
   zeroshm::Array<uint64_t> data(mem, "rw_excl_data", 2);
   data[0] = 0;
   data[1] = 0;

   std::atomic<bool> stop{false};
   std::atomic<int> violations{0};

   std::vector<std::thread> threads;
   for (int r = 0; r < 4; ++r) {
      threads.emplace_back([&]() {
         while (!stop.load(std::memory_order_relaxed)) {
            rwlock.reader_lock();
            uint64_t a = data[0];
            uint64_t b = data[1];
            if (a != b)
               violations.fetch_add(1);
            rwlock.reader_unlock();
         }
      });
   }
   for (int w = 0; w < 2; ++w) {
      threads.emplace_back([&]() {
         for (int i = 0; i < 10000; ++i) {
            rwlock.writer_lock();
            uint64_t v = data[0] + 1;
            data[0] = v;
            std::this_thread::yield();   // widen the exclusion window
            data[1] = v;
            rwlock.writer_unlock();
         }
         stop.store(true);
      });
   }
   for (auto &t : threads)
      t.join();

   EXPECT_EQ(violations.load(), 0)
       << "a reader observed a writer's half-completed update";
}

// Factory function tests (Phase 4.3)

TEST_F(RWLockTest, CreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = RWLock::create(mem, "rw_q");
   ASSERT_TRUE(result.has_value());
   result->destroy();
}

TEST_F(RWLockTest, OpenNotFound) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = RWLock::open(mem, "nonexistent");
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NotFound);
}

TEST_F(RWLockTest, RAIIWithGuard) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = RWLock::create(mem, "guard_rw");
   ASSERT_TRUE(result.has_value());
   {
      SharedMemoryGuard guard(*result);
      guard->reader_lock();
      guard->reader_unlock();
      guard->writer_lock();
      guard->writer_unlock();
   }
   // After guard is destroyed, the name should be removed from the table
   auto open_result = RWLock::open(mem, "guard_rw");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);
}
