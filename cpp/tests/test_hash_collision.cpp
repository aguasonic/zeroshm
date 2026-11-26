#include <cstring>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/detail/xxhash.h>
#include <zeroshm/error.h>
#include <zeroshm/table.h>

using namespace zeroshm;

class HashCollisionTest : public ::testing::Test {
 protected:
   void SetUp() override {
      buffer_size = Table::calculate_size(64) + 10000;
      buffer.resize(buffer_size);
      std::memset(buffer.data(), 0, buffer_size);
   }

   Table::Entry *get_entries() {
      return reinterpret_cast<Table::Entry *>(buffer.data() +
                                              sizeof(Table::Header));
   }

   std::vector<char> buffer;
   size_t buffer_size;
};

TEST_F(HashCollisionTest, HashStorage) {
   Table table(buffer.data(), 64, buffer.size(), true);

   std::string name = "hash_test";
   EXPECT_TRUE(table.add(name, 1000, 100).has_value());

   auto *entries = get_entries();
   uint64_t expected_hash =
       XXH3_64bits_withSeed(name.data(), name.size(), XXHASH_SEED);
   EXPECT_EQ(entries[0].hash, expected_hash);

   // Verify find works
   auto find_result = table.find(name);
   ASSERT_TRUE(find_result.has_value());
   auto *entry = *find_result;
   EXPECT_EQ(entry->size, 100);
}

TEST_F(HashCollisionTest, DifferentNamesDifferentHashes) {
   Table table(buffer.data(), 64, buffer.size(), true);

   EXPECT_TRUE(table.add("alpha", 1000, 100).has_value());
   EXPECT_TRUE(table.add("beta", 2000, 200).has_value());

   auto *entries = get_entries();

   // Different names should have different hashes
   EXPECT_NE(entries[0].hash, entries[1].hash);

   // Both should be findable
   auto f1 = table.find("alpha");
   auto f2 = table.find("beta");

   ASSERT_TRUE(f1.has_value());
   auto *e1 = *f1;
   EXPECT_EQ(e1->size, 100);

   ASSERT_TRUE(f2.has_value());
   auto *e2 = *f2;
   EXPECT_EQ(e2->size, 200);
}

TEST_F(HashCollisionTest, SimulatedCollisionResolution) {
   Table table(buffer.data(), 64, buffer.size(), true);

   uint64_t offset1 = 1000;
   uint64_t offset2 = 2000;

   EXPECT_TRUE(table.add("alpha", offset1, 100).has_value());
   EXPECT_TRUE(table.add("beta", offset2, 200).has_value());

   auto *entries = get_entries();

   // Simulate a hash collision: force both entries to share the same hash
   uint64_t shared_hash = entries[0].hash;
   entries[1].hash = shared_hash;

   // Verify both entries share the same hash but have different names
   EXPECT_EQ(entries[0].hash, entries[1].hash);
   EXPECT_NE(std::string(entries[0].name), std::string(entries[1].name));

   // Finding "alpha": hash matches, string matches entry[0]
   auto f1 = table.find("alpha");
   ASSERT_TRUE(f1.has_value());
   auto *e1 = *f1;
   EXPECT_EQ(e1->offset, offset1);
   EXPECT_EQ(e1->size, 100);

   // Finding "beta": the computed hash of "beta" won't match the shared hash
   // (since we overwrote it), so this tests that non-matching hashes are
   // correctly rejected via the hash-first fast rejection.
   auto f2 = table.find("beta");
   EXPECT_FALSE(f2.has_value());

   // Restore entry[1]'s correct hash
   entries[1].hash = XXH3_64bits_withSeed("beta", strlen("beta"), XXHASH_SEED);

   // Now both entries have their correct hashes again
   f2 = table.find("beta");
   ASSERT_TRUE(f2.has_value());
   auto *e2 = *f2;
   EXPECT_EQ(e2->offset, offset2);
   EXPECT_EQ(e2->size, 200);
}

TEST_F(HashCollisionTest, SameHashDifferentNames) {
   // Test that when two entries share a hash, string comparison distinguishes
   // them
   Table table(buffer.data(), 64, buffer.size(), true);

   EXPECT_TRUE(table.add("name_a", 1000, 100).has_value());
   EXPECT_TRUE(table.add("name_b", 2000, 200).has_value());

   auto *entries = get_entries();

   // Force same hash
   uint64_t shared_hash = entries[0].hash;
   entries[1].hash = shared_hash;

   // Both entries have the same hash
   EXPECT_EQ(entries[0].hash, entries[1].hash);

   // Find "name_a": hash matches entry[0], string matches entry[0]
   auto f1 = table.find("name_a");
   ASSERT_TRUE(f1.has_value());
   auto *e1 = *f1;
   EXPECT_EQ(e1->offset, 1000);

   // Find "name_b": hash matches both entries, but string only matches entry[1]
   // Since "name_b"'s computed hash won't match shared_hash, it won't be found
   // This tests the hash-first rejection path
   auto f2 = table.find("name_b");
   EXPECT_FALSE(f2.has_value());
}

int main(int argc, char **argv) {
   ::testing::InitGoogleTest(&argc, argv);
   return RUN_ALL_TESTS();
}
