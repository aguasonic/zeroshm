#include <thread>
#include <unistd.h>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/map.h>
#include <zeroshm/memory.h>
#include <zeroshm/pool.h>
#include <zeroshm/raii.h>
#include <zeroshm/ring.h>
#include <zeroshm/set.h>

using namespace zeroshm;

class NewStructuresTest : public ::testing::Test {
 protected:
   void SetUp() override {
      // Create unique shared memory name for each test
      shm_name_ = "/test_new_" + std::to_string(getpid()) + "_" +
                  std::to_string(test_counter_++);
   }

   void TearDown() override {
      // Cleanup
      Memory::unlink(shm_name_);
   }

   std::string shm_name_;
   static int test_counter_;
};

int NewStructuresTest::test_counter_ = 0;

// Map Tests
TEST_F(NewStructuresTest, MapBasicOperations) {
   Memory mem(shm_name_, 1024 * 1024);
   Map<int, double> map(mem, "test_map", 100);

   // Insert
   EXPECT_TRUE(map.insert(42, 3.14).has_value());
   EXPECT_TRUE(map.insert(10, 2.71).has_value());

   // Find
   auto val = map.find(42);
   ASSERT_TRUE(val.has_value());
   EXPECT_DOUBLE_EQ(*val, 3.14);

   // Contains
   EXPECT_TRUE(map.contains(42));
   EXPECT_FALSE(map.contains(999));

   // Update
   EXPECT_TRUE(map.insert(42, 6.28).has_value());
   val = map.find(42);
   ASSERT_TRUE(val.has_value());
   EXPECT_DOUBLE_EQ(*val, 6.28);

   // Size
   EXPECT_EQ(map.size(), 2);

   // Erase
   EXPECT_TRUE(map.erase(10).has_value());
   EXPECT_FALSE(map.contains(10));
   EXPECT_EQ(map.size(), 1);
}

TEST_F(NewStructuresTest, MapOpenExisting) {
   Memory mem(shm_name_, 1024 * 1024);
   {
      Map<int, int> map1(mem, "shared_map", 50);
      ASSERT_TRUE(map1.insert(1, 100).has_value());
      ASSERT_TRUE(map1.insert(2, 200).has_value());
   }

   // Open existing map
   Map<int, int> map2(mem, "shared_map");
   EXPECT_EQ(map2.size(), 2);

   auto val = map2.find(1);
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 100);
}

// Set Tests
TEST_F(NewStructuresTest, SetBasicOperations) {
   Memory mem(shm_name_, 1024 * 1024);
   Set<int> set(mem, "test_set", 100);

   // Insert
   EXPECT_TRUE(set.insert(42).has_value());
   EXPECT_TRUE(set.insert(10).has_value());
   EXPECT_TRUE(set.insert(42).has_value());   // Duplicate (idempotent)

   // Contains
   EXPECT_TRUE(set.contains(42));
   EXPECT_FALSE(set.contains(999));

   // Size
   EXPECT_EQ(set.size(), 2);

   // Erase
   EXPECT_TRUE(set.erase(10).has_value());
   EXPECT_FALSE(set.contains(10));
   EXPECT_EQ(set.size(), 1);
}

// Pool Tests
TEST_F(NewStructuresTest, PoolBasicOperations) {
   Memory mem(shm_name_, 1024 * 1024);

   struct TestObject {
      int value;
      double data;
   };

   Pool<TestObject> pool(mem, "test_pool", 10);

   EXPECT_EQ(pool.capacity(), 10);
   EXPECT_EQ(pool.allocated(), 0);
   EXPECT_EQ(pool.available(), 10);

   // Allocate objects
   auto obj1 = pool.allocate();
   ASSERT_TRUE(obj1.has_value());
   (*obj1)->value = 42;
   (*obj1)->data = 3.14;

   auto obj2 = pool.allocate();
   ASSERT_TRUE(obj2.has_value());

   EXPECT_EQ(pool.allocated(), 2);
   EXPECT_EQ(pool.available(), 8);

   // Deallocate
   EXPECT_TRUE(pool.deallocate(*obj1).has_value());
   EXPECT_EQ(pool.allocated(), 1);

   // Allocate again - should reuse freed slot
   auto obj3 = pool.allocate();
   ASSERT_TRUE(obj3.has_value());
   EXPECT_EQ(pool.allocated(), 2);
}

TEST_F(NewStructuresTest, PoolConstruct) {
   Memory mem(shm_name_, 1024 * 1024);

   struct Point {
      int x, y;
      Point(int x_, int y_) : x(x_), y(y_) {
      }
   };

   Pool<Point> pool(mem, "point_pool", 5);

   auto p1 = pool.construct(10, 20);
   ASSERT_TRUE(p1.has_value());
   EXPECT_EQ((*p1)->x, 10);
   EXPECT_EQ((*p1)->y, 20);

   EXPECT_TRUE(pool.destroy(*p1).has_value());
   EXPECT_EQ(pool.allocated(), 0);
}

// Ring Buffer Tests
TEST_F(NewStructuresTest, RingBasicOperations) {
   Memory mem(shm_name_, 1024 * 1024);
   Ring<int> ring(mem, "test_ring", 100 * sizeof(int));

   EXPECT_TRUE(ring.empty());
   EXPECT_FALSE(ring.full());

   // Write data
   EXPECT_TRUE(ring.write(42).has_value());
   EXPECT_TRUE(ring.write(43).has_value());

   EXPECT_FALSE(ring.empty());
   EXPECT_EQ(ring.available(), 2);

   // Read data
   auto val = ring.read();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 42);

   val = ring.read();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 43);

   EXPECT_TRUE(ring.empty());
}

TEST_F(NewStructuresTest, RingBulkOperations) {
   Memory mem(shm_name_, 1024 * 1024);
   Ring<int> ring(mem, "bulk_ring", 100 * sizeof(int));

   // Write bulk
   int data[] = {1, 2, 3, 4, 5};
   size_t written = ring.write_bulk(data, 5);
   EXPECT_EQ(written, 5);
   EXPECT_EQ(ring.available(), 5);

   // Read bulk
   int read_data[5];
   size_t read_count = ring.read_bulk(read_data, 5);
   EXPECT_EQ(read_count, 5);

   for (int i = 0; i < 5; ++i) {
      EXPECT_EQ(read_data[i], i + 1);
   }

   EXPECT_TRUE(ring.empty());
}

TEST_F(NewStructuresTest, RingWrapAround) {
   Memory mem(shm_name_, 1024 * 1024);
   Ring<int> ring(mem, "wrap_ring",
                  5 * sizeof(int));   // Small ring for testing wrap

   // Fill the ring
   for (int i = 0; i < 5; ++i) {
      EXPECT_TRUE(ring.write(i).has_value());
   }

   EXPECT_TRUE(ring.full());
   EXPECT_FALSE(ring.write(99).has_value());   // Should fail when full

   // Read some elements
   for (int i = 0; i < 3; ++i) {
      auto val = ring.read();
      ASSERT_TRUE(val.has_value());
      EXPECT_EQ(*val, i);
   }

   // Write more (will wrap around)
   EXPECT_TRUE(ring.write(10).has_value());
   EXPECT_TRUE(ring.write(11).has_value());
   EXPECT_TRUE(ring.write(12).has_value());

   // Read all remaining
   std::vector<int> values;
   while (!ring.empty()) {
      auto val = ring.read();
      if (val.has_value())
         values.push_back(*val);
   }

   EXPECT_EQ(values.size(), 5);
   EXPECT_EQ(values[0], 3);
   EXPECT_EQ(values[1], 4);
   EXPECT_EQ(values[2], 10);
   EXPECT_EQ(values[3], 11);
   EXPECT_EQ(values[4], 12);
}

// Concurrent Tests
TEST_F(NewStructuresTest, MapConcurrentInsert) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Map<int, int> map(mem, "concurrent_map", 1000);

   const int num_threads = 4;
   const int items_per_thread = 100;

   std::vector<std::thread> threads;
   for (int t = 0; t < num_threads; ++t) {
      threads.emplace_back([&map, t]() {
         for (int i = 0; i < items_per_thread; ++i) {
            int key = t * items_per_thread + i;
            EXPECT_TRUE(map.insert(key, key * 2).has_value());
         }
      });
   }

   for (auto &t : threads) {
      t.join();
   }

   // Verify all items are present
   EXPECT_EQ(map.size(), num_threads * items_per_thread);

   for (int i = 0; i < num_threads * items_per_thread; ++i) {
      auto val = map.find(i);
      ASSERT_TRUE(val.has_value());
      EXPECT_EQ(*val, i * 2);
   }
}

TEST_F(NewStructuresTest, PoolConcurrentAllocate) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Pool<int> pool(mem, "concurrent_pool", 100);

   const int num_threads = 4;
   const int allocs_per_thread = 20;

   std::vector<std::thread> threads;
   std::vector<std::vector<int *>> thread_allocations(num_threads);

   for (int t = 0; t < num_threads; ++t) {
      threads.emplace_back([&pool, &thread_allocations, t]() {
         for (int i = 0; i < allocs_per_thread; ++i) {
            auto ptr = pool.allocate();
            if (ptr.has_value()) {
               **ptr = t * 1000 + i;
               thread_allocations[t].push_back(*ptr);
            }
         }
      });
   }

   for (auto &t : threads) {
      t.join();
   }

   // Verify total allocations
   size_t total_allocated = 0;
   for (const auto &allocs : thread_allocations) {
      total_allocated += allocs.size();
   }

   EXPECT_EQ(pool.allocated(), total_allocated);
   EXPECT_LE(total_allocated, 100);   // Should not exceed pool capacity

   // Deallocate all
   for (auto &allocs : thread_allocations) {
      for (auto *ptr : allocs) {
         EXPECT_TRUE(pool.deallocate(ptr).has_value());
      }
   }

   EXPECT_EQ(pool.allocated(), 0);
}

// Race condition regression tests

// Test: concurrent insert + find should never read uninitialized key/value
// This verifies the EMPTY -> INSERTING -> OCCUPIED 3-state protocol.
TEST_F(NewStructuresTest, MapConcurrentInsertFindRace) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Map<int, int> map(mem, "race_map", 2000);

   const int num_writers = 4;
   const int num_readers = 4;
   const int items_per_writer = 200;
   std::atomic<bool> done{false};
   std::atomic<int> bad_reads{0};

   // Writers: insert key=k, value=k*100
   std::vector<std::thread> writers;
   for (int w = 0; w < num_writers; ++w) {
      writers.emplace_back([&map, w]() {
         for (int i = 0; i < items_per_writer; ++i) {
            int key = w * items_per_writer + i;
            EXPECT_TRUE(map.insert(key, key * 100).has_value());
         }
      });
   }

   // Readers: find keys and verify value == key * 100 (never garbage)
   std::vector<std::thread> readers;
   for (int r = 0; r < num_readers; ++r) {
      readers.emplace_back([&map, &done, &bad_reads]() {
         int total = num_writers * items_per_writer;
         while (!done.load(std::memory_order_relaxed)) {
            for (int k = 0; k < total; ++k) {
               auto val = map.find(k);
               if (val.has_value() && *val != k * 100) {
                  bad_reads.fetch_add(1, std::memory_order_relaxed);
               }
            }
         }
      });
   }

   for (auto &w : writers)
      w.join();
   done.store(true, std::memory_order_relaxed);
   for (auto &r : readers)
      r.join();

   EXPECT_EQ(bad_reads.load(), 0)
       << "Reader saw uninitialized or corrupt value!";
   EXPECT_EQ(map.size(), num_writers * items_per_writer);
}

// Test: concurrent erase of the same key must decrement size exactly once
// This verifies the CAS-based erase (no double-decrement).
TEST_F(NewStructuresTest, MapConcurrentEraseNoDoubleDecrement) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Map<int, int> map(mem, "erase_map", 2000);

   const int num_items = 500;
   const int num_erasers = 8;

   // Pre-populate
   for (int i = 0; i < num_items; ++i) {
      ASSERT_TRUE(map.insert(i, i * 10).has_value());
   }
   ASSERT_EQ(map.size(), num_items);

   // Race: all threads try to erase the same set of keys
   std::vector<std::thread> erasers;
   std::vector<int> success_counts(num_erasers, 0);

   for (int e = 0; e < num_erasers; ++e) {
      erasers.emplace_back([&map, &success_counts, e]() {
         for (int i = 0; i < num_items; ++i) {
            if (map.erase(i).has_value()) {
               success_counts[e]++;
            }
         }
      });
   }

   for (auto &e : erasers)
      e.join();

   // Total successful erases must equal the number of items
   int total_erased = 0;
   for (int c : success_counts)
      total_erased += c;
   EXPECT_EQ(total_erased, num_items)
       << "Each key must be erased exactly once across all threads";

   // Size must be exactly 0, no underflow
   EXPECT_EQ(map.size(), 0);
}

// Test: concurrent insert + find for Set (same 3-state protocol)
TEST_F(NewStructuresTest, SetConcurrentInsertContainsRace) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Set<int> set(mem, "race_set", 2000);

   const int num_writers = 4;
   const int num_readers = 4;
   const int items_per_writer = 200;
   std::atomic<bool> done{false};

   // Writers: insert sequential values per thread
   std::vector<std::thread> writers;
   for (int w = 0; w < num_writers; ++w) {
      writers.emplace_back([&set, w]() {
         for (int i = 0; i < items_per_writer; ++i) {
            int value = w * items_per_writer + i;
            EXPECT_TRUE(set.insert(value).has_value());
         }
      });
   }

   // Readers: check contains (should never crash or hang)
   std::vector<std::thread> readers;
   for (int r = 0; r < num_readers; ++r) {
      readers.emplace_back([&set, &done]() {
         int total = num_writers * items_per_writer;
         while (!done.load(std::memory_order_relaxed)) {
            for (int k = 0; k < total; ++k) {
               (void)set.contains(k);   // Must not read garbage
            }
         }
      });
   }

   for (auto &w : writers)
      w.join();
   done.store(true, std::memory_order_relaxed);
   for (auto &r : readers)
      r.join();

   EXPECT_EQ(set.size(), num_writers * items_per_writer);
}

// Test: concurrent erase of the same elements in Set
TEST_F(NewStructuresTest, SetConcurrentEraseNoDoubleDecrement) {
   Memory mem(shm_name_, 10 * 1024 * 1024);
   Set<int> set(mem, "erase_set", 2000);

   const int num_items = 500;
   const int num_erasers = 8;

   // Pre-populate
   for (int i = 0; i < num_items; ++i) {
      ASSERT_TRUE(set.insert(i).has_value());
   }
   ASSERT_EQ(set.size(), num_items);

   // Race: all threads try to erase the same set of values
   std::vector<std::thread> erasers;
   std::vector<int> success_counts(num_erasers, 0);

   for (int e = 0; e < num_erasers; ++e) {
      erasers.emplace_back([&set, &success_counts, e]() {
         for (int i = 0; i < num_items; ++i) {
            if (set.erase(i).has_value()) {
               success_counts[e]++;
            }
         }
      });
   }

   for (auto &e : erasers)
      e.join();

   // Total successful erases must equal the number of items
   int total_erased = 0;
   for (int c : success_counts)
      total_erased += c;
   EXPECT_EQ(total_erased, num_items)
       << "Each element must be erased exactly once across all threads";

   // Size must be exactly 0, no underflow
   EXPECT_EQ(set.size(), 0);
}

// Span View Access Tests
TEST_F(NewStructuresTest, MapEntriesView) {
   Memory mem(shm_name_, 1024 * 1024);
   Map<int, int> map(mem, "span_map", 16);

   EXPECT_TRUE(map.insert(1, 10).has_value());
   EXPECT_TRUE(map.insert(2, 20).has_value());

   auto entries = map.entries();
   EXPECT_EQ(entries.size(), 16);   // entire capacity

   // Find the occupied entries via the span view
   int found_count = 0;
   for (const auto &entry : entries) {
      if (entry.state.load(std::memory_order_relaxed) ==
          Map<int, int>::OCCUPIED) {
         found_count++;
      }
   }
   EXPECT_EQ(found_count, 2);
}

TEST_F(NewStructuresTest, SetEntriesView) {
   Memory mem(shm_name_, 1024 * 1024);
   Set<int> set(mem, "span_set", 16);

   EXPECT_TRUE(set.insert(10).has_value());
   EXPECT_TRUE(set.insert(20).has_value());
   EXPECT_TRUE(set.insert(30).has_value());

   auto entries = set.entries();
   EXPECT_EQ(entries.size(), 16);   // entire capacity

   // Find the occupied entries via the span view
   int found_count = 0;
   for (const auto &entry : entries) {
      if (entry.state.load(std::memory_order_relaxed) == Set<int>::OCCUPIED) {
         found_count++;
      }
   }
   EXPECT_EQ(found_count, 3);
}

TEST_F(NewStructuresTest, RingSpanAccess) {
   Memory mem(shm_name_, 64 * 1024);
   Ring<int> ring(mem, "span_ring", 8 * sizeof(int));

   EXPECT_TRUE(ring.write(1).has_value());
   EXPECT_TRUE(ring.write(2).has_value());
   EXPECT_TRUE(ring.write(3).has_value());

   auto span = ring.data();
   EXPECT_EQ(span.size(), 8);   // entire capacity
   EXPECT_EQ(span[0], 1);
   EXPECT_EQ(span[1], 2);
   EXPECT_EQ(span[2], 3);

   // Raw pointer compatibility
   EXPECT_EQ(ring.raw_data()[0], 1);

   // Const span
   const Ring<int> &cring = ring;
   auto cspan = cring.data();
   EXPECT_EQ(cspan.size(), 8);
   EXPECT_EQ(cspan[0], 1);
}

TEST_F(NewStructuresTest, PoolNodesView) {
   Memory mem(shm_name_, 1024 * 1024);

   struct TestObj {
      int value;
   };

   Pool<TestObj> pool(mem, "span_pool", 8);

   auto obj = pool.allocate();
   ASSERT_TRUE(obj.has_value());
   (*obj)->value = 42;

   auto nodes = pool.nodes();
   EXPECT_EQ(nodes.size(), 8);   // entire capacity

   // Find the allocated node via the span view
   bool found = false;
   for (const auto &node : nodes) {
      if (node.data.value == 42) {
         found = true;
         break;
      }
   }
   EXPECT_TRUE(found);
}

// Factory function tests (Phase 4.3)

TEST_F(NewStructuresTest, MapCreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Map<int, double>::create(mem, "map_q", 100);
   ASSERT_TRUE(result.has_value());
   EXPECT_TRUE(result->insert(42, 3.14).has_value());
   auto val = result->find(42);
   ASSERT_TRUE(val.has_value());
   EXPECT_DOUBLE_EQ(*val, 3.14);
   result->destroy();
}

TEST_F(NewStructuresTest, MapCreateInvalidCapacity) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Map<int, double>::create(mem, "map_q", 0);
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::InvalidCapacity);
}

TEST_F(NewStructuresTest, MapOpenNotFound) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Map<int, double>::open(mem, "nonexistent");
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NotFound);
}

TEST_F(NewStructuresTest, SetCreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Set<int>::create(mem, "set_q", 100);
   ASSERT_TRUE(result.has_value());
   EXPECT_TRUE(result->insert(42).has_value());
   EXPECT_TRUE(result->contains(42));
   result->destroy();
}

TEST_F(NewStructuresTest, PoolCreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);

   struct TestObj {
      int value;
   };

   auto result = Pool<TestObj>::create(mem, "pool_q", 10);
   ASSERT_TRUE(result.has_value());
   auto obj = result->allocate();
   ASSERT_TRUE(obj.has_value());
   (*obj)->value = 42;
   result->destroy();
}

TEST_F(NewStructuresTest, PoolCreateInvalidCapacity) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Pool<int>::create(mem, "pool_q", 0);
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::InvalidCapacity);
}

TEST_F(NewStructuresTest, RingCreateWithExpected) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Ring<int>::create(mem, "ring_q", 100 * sizeof(int));
   ASSERT_TRUE(result.has_value());
   EXPECT_TRUE(result->write(42).has_value());
   auto val = result->read();
   ASSERT_TRUE(val.has_value());
   EXPECT_EQ(*val, 42);
   result->destroy();
}

TEST_F(NewStructuresTest, RingCreateInvalidCapacity) {
   Memory mem(shm_name_, 1024 * 1024);
   auto result = Ring<int>::create(mem, "ring_q", 0);
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::InvalidCapacity);
}

TEST_F(NewStructuresTest, RAIIWithGuard) {
   Memory mem(shm_name_, 1024 * 1024);

   auto result = Map<int, int>::create(mem, "guard_map", 50);
   ASSERT_TRUE(result.has_value());
   {
      SharedMemoryGuard guard(*result);
      EXPECT_TRUE(guard->insert(1, 100).has_value());
      EXPECT_EQ(guard->size(), 1);
   }

   // After guard is destroyed, the name should be removed from the table
   auto open_result = Map<int, int>::open(mem, "guard_map");
   EXPECT_FALSE(open_result.has_value());
   EXPECT_EQ(open_result.error(), Error::NotFound);
}

int main(int argc, char **argv) {
   ::testing::InitGoogleTest(&argc, argv);
   return RUN_ALL_TESTS();
}
// Regression: insert must scan the whole probe chain before claiming a
// reusable DELETED slot. With the multiplicative hash and capacity 8, keys
// k and k+8 share a home slot, so (insert k1, insert k2, erase k1,
// re-insert k2) used to duplicate k2: the re-insert claimed k1's DELETED
// slot instead of updating k2 in place further down the chain.
TEST_F(NewStructuresTest, MapNoDuplicateAfterDeletedSlotReuse) {
   Memory mem(shm_name_, 1024 * 1024);
   Map<int, int> map(mem, "dup_map", 8);

   ASSERT_TRUE(map.insert(1, 100).has_value());   // home slot
   ASSERT_TRUE(
       map.insert(9, 200).has_value());   // collides, displaced past key 1
   ASSERT_TRUE(
       map.erase(1).has_value());   // leaves a DELETED slot in 9's chain

   ASSERT_TRUE(
       map.insert(9, 300).has_value());   // must UPDATE, not claim the hole
   EXPECT_EQ(map.size(), 1u);

   auto v = map.find(9);
   ASSERT_TRUE(v.has_value());
   EXPECT_EQ(*v, 300);

   ASSERT_TRUE(map.erase(9).has_value());
   EXPECT_FALSE(map.find(9).has_value())
       << "stale duplicate entry survived erase";
   EXPECT_FALSE(map.erase(9).has_value());
   EXPECT_EQ(map.size(), 0u);
}

TEST_F(NewStructuresTest, SetNoDuplicateAfterDeletedSlotReuse) {
   Memory mem(shm_name_, 1024 * 1024);
   Set<int> set(mem, "dup_set", 8);

   ASSERT_TRUE(set.insert(1).has_value());
   ASSERT_TRUE(set.insert(9).has_value());   // collides, displaced past 1
   ASSERT_TRUE(set.erase(1).has_value());    // DELETED slot in 9's chain

   EXPECT_TRUE(set.insert(9).has_value())
       << "re-insert of an existing value is idempotent";
   EXPECT_EQ(set.size(), 1u);

   ASSERT_TRUE(set.erase(9).has_value());
   EXPECT_FALSE(set.contains(9)) << "stale duplicate entry survived erase";
   EXPECT_EQ(set.size(), 0u);
}
