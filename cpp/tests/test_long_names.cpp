#include <cstring>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/error.h>
#include <zeroshm/table.h>

using namespace zeroshm;

class LongNamesTest : public ::testing::Test {
 protected:
   void SetUp() override {
      buffer_size = Table::calculate_size(64) + 10000;
      buffer.resize(buffer_size);
      std::memset(buffer.data(), 0, buffer_size);
   }

   std::vector<char> buffer;
   size_t buffer_size;
};

TEST_F(LongNamesTest, ShortNames) {
   Table table(buffer.data(), 64, buffer.size(), true);

   EXPECT_TRUE(table.add("short", 1000, 100).has_value());

   auto find_result = table.find("short");
   ASSERT_TRUE(find_result.has_value());
   auto *entry = *find_result;
   EXPECT_STREQ(entry->name, "short");
   EXPECT_EQ(entry->size, 100);
}

TEST_F(LongNamesTest, MediumNames) {
   Table table(buffer.data(), 64, buffer.size(), true);

   std::string name_50(50, 'a');
   std::string name_100(100, 'b');
   std::string name_200(200, 'c');

   EXPECT_TRUE(table.add(name_50, 1000, 100).has_value());
   EXPECT_TRUE(table.add(name_100, 2000, 200).has_value());
   EXPECT_TRUE(table.add(name_200, 3000, 300).has_value());

   EXPECT_EQ(table.entry_count(), 3);

   auto f1 = table.find(name_50);
   auto f2 = table.find(name_100);
   auto f3 = table.find(name_200);

   ASSERT_TRUE(f1.has_value());
   auto *e1 = *f1;
   EXPECT_EQ(e1->size, 100);

   ASSERT_TRUE(f2.has_value());
   auto *e2 = *f2;
   EXPECT_EQ(e2->size, 200);

   ASSERT_TRUE(f3.has_value());
   auto *e3 = *f3;
   EXPECT_EQ(e3->size, 300);
}

TEST_F(LongNamesTest, MaxLengthName) {
   Table table(buffer.data(), 64, buffer.size(), true);

   std::string name_254(254, 'd');   // max 254 chars (255 bytes with null)

   EXPECT_TRUE(table.add(name_254, 1000, 400).has_value());

   auto find_result = table.find(name_254);
   ASSERT_TRUE(find_result.has_value());
   auto *entry = *find_result;
   EXPECT_EQ(entry->size, 400);

   // Verify the stored name matches
   std::string stored(entry->name);
   EXPECT_EQ(stored, name_254);
}

TEST_F(LongNamesTest, TooLongNameThrows) {
   Table table(buffer.data(), 64, buffer.size(), true);

   std::string name_255(255, 'e');   // 255 chars is too long (max is 254)

   auto add_result = table.add(name_255, 1000, 500);
   EXPECT_FALSE(add_result.has_value());
   EXPECT_EQ(add_result.error(), Error::NameTooLong);
   EXPECT_EQ(table.entry_count(), 0);
}

TEST_F(LongNamesTest, HashIsStored) {
   Table table(buffer.data(), 64, buffer.size(), true);

   std::string name = "hash_test";
   EXPECT_TRUE(table.add(name, 1000, 100).has_value());

   // Access the entry directly to verify hash storage
   auto *entries =
       reinterpret_cast<Table::Entry *>(buffer.data() + sizeof(Table::Header));

   uint64_t expected_hash =
       XXH3_64bits_withSeed(name.data(), name.size(), XXHASH_SEED);
   EXPECT_EQ(entries[0].hash, expected_hash);
}

int main(int argc, char **argv) {
   ::testing::InitGoogleTest(&argc, argv);
   return RUN_ALL_TESTS();
}
