#include <cstring>
#include <vector>

#include <gtest/gtest.h>
#include <zeroshm/table.h>

using namespace zeroshm;

class TableTest : public ::testing::Test {
 protected:
   void SetUp() override {
      // Allocate memory for testing
      buffer_size =
          Table::calculate_size(64) + 10000;   // Extra space for allocations
      buffer.resize(buffer_size);
      std::memset(buffer.data(), 0, buffer_size);
   }

   std::vector<char> buffer;
   size_t buffer_size;
};

TEST_F(TableTest, CreateNewTable) {
   Table table(buffer.data(), 64, buffer.size(), true);

   EXPECT_EQ(table.entry_count(), 0);
   EXPECT_EQ(table.max_entries(), 64);
   EXPECT_EQ(table.next_offset(), Table::calculate_size(64));
}

TEST_F(TableTest, AddAndFindEntry) {
   Table table(buffer.data(), 64, buffer.size(), true);

   auto add_result = table.add("test_entry", 1000, 500);
   ASSERT_TRUE(add_result.has_value());
   EXPECT_EQ(table.entry_count(), 1);

   auto find_result = table.find("test_entry");
   ASSERT_TRUE(find_result.has_value());
   const auto *entry = *find_result;
   EXPECT_STREQ(entry->name, "test_entry");
   EXPECT_EQ(entry->offset, 1000);
   EXPECT_EQ(entry->size, 500);

   // Non-existent entry
   auto not_found = table.find("nonexistent");
   EXPECT_FALSE(not_found.has_value());
   EXPECT_EQ(not_found.error(), Error::NotFound);
}

TEST_F(TableTest, MultipleEntries) {
   Table table(buffer.data(), 64, buffer.size(), true);

   EXPECT_TRUE(table.add("entry1", 1000, 100).has_value());
   EXPECT_TRUE(table.add("entry2", 2000, 200).has_value());
   EXPECT_TRUE(table.add("entry3", 3000, 300).has_value());

   EXPECT_EQ(table.entry_count(), 3);

   auto e1 = table.find("entry1");
   auto e2 = table.find("entry2");
   auto e3 = table.find("entry3");

   ASSERT_TRUE(e1.has_value());
   ASSERT_TRUE(e2.has_value());
   ASSERT_TRUE(e3.has_value());

   EXPECT_EQ((*e1)->offset, 1000);
   EXPECT_EQ((*e2)->offset, 2000);
   EXPECT_EQ((*e3)->offset, 3000);
}

TEST_F(TableTest, DuplicateNameRejected) {
   Table table(buffer.data(), 64, buffer.size(), true);

   EXPECT_TRUE(table.add("test", 1000, 100).has_value());
   auto result = table.add("test", 2000, 200);
   EXPECT_FALSE(result.has_value());
   EXPECT_EQ(result.error(), Error::NameAlreadyExists);
}

TEST_F(TableTest, LongNameRejected) {
   Table table(buffer.data(), 64, buffer.size(), true);

   std::string long_name(255, 'x');   // 255 chars, too long (max is 254)
   auto long_result = table.add(long_name, 1000, 100);
   EXPECT_FALSE(long_result.has_value());
   EXPECT_EQ(long_result.error(), Error::NameTooLong);

   std::string max_name(254, 'y');   // 254 chars, maximum allowed
   EXPECT_TRUE(table.add(max_name, 1000, 100).has_value());
}

TEST_F(TableTest, ShortNameRejected) {
   Table table(buffer.data(), 64, buffer.size(), true);

   // 1-character name is too short
   auto short_result = table.add("a", 1000, 100);
   EXPECT_FALSE(short_result.has_value());
   EXPECT_EQ(short_result.error(), Error::NameTooShort);

   // 2-character name is too short
   auto short_result2 = table.add("ab", 1000, 100);
   EXPECT_FALSE(short_result2.has_value());
   EXPECT_EQ(short_result2.error(), Error::NameTooShort);

   // 3-character name is the minimum allowed
   auto min_result = table.add("abc", 1000, 100);
   EXPECT_TRUE(min_result.has_value());
   EXPECT_EQ(table.entry_count(), 1);

   // find() also rejects short names
   auto find_short = table.find("ab");
   EXPECT_FALSE(find_short.has_value());
   EXPECT_EQ(find_short.error(), Error::NameTooShort);
}

TEST_F(TableTest, TableFull) {
   Table table(buffer.data(), 4, buffer.size(),
               true);   // Small table for testing

   EXPECT_TRUE(table.add("entry1", 1000, 100).has_value());
   EXPECT_TRUE(table.add("entry2", 2000, 100).has_value());
   EXPECT_TRUE(table.add("entry3", 3000, 100).has_value());
   EXPECT_TRUE(table.add("entry4", 4000, 100).has_value());

   // Table is now full
   auto full_result = table.add("entry5", 5000, 100);
   EXPECT_FALSE(full_result.has_value());
   EXPECT_EQ(full_result.error(), Error::TableFull);
   EXPECT_EQ(table.entry_count(), 4);
}

TEST_F(TableTest, Allocation) {
   Table table(buffer.data(), 64, buffer.size(), true);

   uint32_t initial = table.next_offset();

   // Allocate with default alignment (8)
   uint32_t offset1 = table.allocate(100);
   EXPECT_EQ(offset1, initial);
   EXPECT_EQ(table.next_offset(), initial + 100);

   // Allocate with specific alignment
   uint32_t offset2 = table.allocate(50, 16);
   EXPECT_EQ(offset2 % 16, 0);   // Should be aligned to 16
   EXPECT_GE(offset2, initial + 100);
}

TEST_F(TableTest, OpenExistingTable) {
   // Create a table
   {
      Table table(buffer.data(), 64, buffer.size(), true);
      ASSERT_TRUE(table.add("persistent", 1000, 500).has_value());
   }

   // Open the existing table
   {
      Table table(buffer.data(), 64, buffer.size(), false);
      EXPECT_EQ(table.entry_count(), 1);

      auto find_result = table.find("persistent");
      ASSERT_TRUE(find_result.has_value());
      const auto *entry = *find_result;
      EXPECT_STREQ(entry->name, "persistent");
      EXPECT_EQ(entry->offset, 1000);
      EXPECT_EQ(entry->size, 500);
   }
}

TEST_F(TableTest, InvalidMagicThrows) {
   // Create a table and corrupt the magic number
   Table table(buffer.data(), 64, buffer.size(), true);
   *reinterpret_cast<uint32_t *>(buffer.data()) = 0xDEADBEEF;

   EXPECT_THROW(Table(buffer.data(), 64, false), std::runtime_error);
}

TEST_F(TableTest, CalculateSize) {
   // Size should be header + entries
   size_t size_64 = Table::calculate_size(64);
   size_t size_128 = Table::calculate_size(128);

   EXPECT_EQ(size_64, sizeof(Table::Header) + 64 * sizeof(Table::Entry));
   EXPECT_EQ(size_128, sizeof(Table::Header) + 128 * sizeof(Table::Entry));
   EXPECT_GT(size_128, size_64);
}

TEST_F(TableTest, AlignmentWorks) {
   Table table(buffer.data(), 64, buffer.size(), true);

   // Start at unaligned position
   table.allocate(7);   // Now next_offset is not aligned to 8

   // Request 8-byte alignment
   uint32_t aligned = table.allocate(100, 8);
   EXPECT_EQ(aligned % 8, 0);

   // Request 16-byte alignment
   table.allocate(7);   // Misalign again
   uint32_t aligned16 = table.allocate(100, 16);
   EXPECT_EQ(aligned16 % 16, 0);

   // Request 64-byte alignment
   table.allocate(7);   // Misalign again
   uint32_t aligned64 = table.allocate(100, 64);
   EXPECT_EQ(aligned64 % 64, 0);
}

TEST_F(TableTest, EntriesGenerator) {
   Table table(buffer.data(), 64, buffer.size(), true);

   // Empty table should yield nothing
   {
      size_t count = 0;
      for (const auto &entry : table.entries()) {
         (void)entry;
         ++count;
      }
      EXPECT_EQ(count, 0);
   }

   // Add entries and verify generator yields all of them
   EXPECT_TRUE(table.add("alpha", 1000, 100).has_value());
   EXPECT_TRUE(table.add("beta", 2000, 200).has_value());
   EXPECT_TRUE(table.add("gamma", 3000, 300).has_value());

   std::vector<Table::Entry> collected;
   for (const auto &entry : table.entries()) {
      collected.push_back(entry);
   }

   EXPECT_EQ(collected.size(), 3u);
   EXPECT_STREQ(collected[0].name, "alpha");
   EXPECT_EQ(collected[0].offset, 1000);
   EXPECT_EQ(collected[0].size, 100);
   EXPECT_STREQ(collected[1].name, "beta");
   EXPECT_EQ(collected[1].offset, 2000);
   EXPECT_EQ(collected[1].size, 200);
   EXPECT_STREQ(collected[2].name, "gamma");
   EXPECT_EQ(collected[2].offset, 3000);
   EXPECT_EQ(collected[2].size, 300);
}

int main(int argc, char **argv) {
   ::testing::InitGoogleTest(&argc, argv);
   return RUN_ALL_TESTS();
}
