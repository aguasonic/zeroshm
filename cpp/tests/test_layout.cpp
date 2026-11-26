#include <atomic>
#include <iostream>

#include <gtest/gtest.h>

#include <zeroshm/layout.h>

using namespace zeroshm;

// 
// align_up_consteval() tests
// 

TEST(LayoutTest, AlignUpAlreadyAligned) {
   EXPECT_EQ(align_up_consteval(8, 8), 8);
   EXPECT_EQ(align_up_consteval(16, 8), 16);
   EXPECT_EQ(align_up_consteval(0, 8), 0);
   EXPECT_EQ(align_up_consteval(32, 4), 32);
}

TEST(LayoutTest, AlignUpRoundsUp) {
   EXPECT_EQ(align_up_consteval(1, 8), 8);
   EXPECT_EQ(align_up_consteval(5, 8), 8);
   EXPECT_EQ(align_up_consteval(9, 8), 16);
   EXPECT_EQ(align_up_consteval(13, 8), 16);
   EXPECT_EQ(align_up_consteval(17, 8), 24);
}

TEST(LayoutTest, AlignUpPowerOfTwo) {
   EXPECT_EQ(align_up_consteval(1, 4), 4);
   EXPECT_EQ(align_up_consteval(3, 4), 4);
   EXPECT_EQ(align_up_consteval(5, 4), 8);
   EXPECT_EQ(align_up_consteval(10, 4), 12);
}

TEST(LayoutTest, AlignUpAlignmentOfOne) {
   EXPECT_EQ(align_up_consteval(5, 1), 5);
   EXPECT_EQ(align_up_consteval(100, 1), 100);
}

// 
// Expected size constants
// 

TEST(LayoutTest, ExpectedSizes) {
   EXPECT_EQ(EXPECTED_TABLE_HEADER_SIZE, 32);
   EXPECT_EQ(EXPECTED_TABLE_ENTRY_SIZE, 384);
   EXPECT_EQ(EXPECTED_QUEUE_HEADER_SIZE, 16);
   EXPECT_EQ(EXPECTED_RING_HEADER_SIZE, 24);
   EXPECT_EQ(EXPECTED_ARRAY_HEADER_SIZE, 8);
}

// 
// table_total_size() tests
// 

TEST(LayoutTest, TableTotalSizeZeroEntries) {
   EXPECT_EQ(table_total_size(0), 32);   // Just the header
}

TEST(LayoutTest, TableTotalSizeOneEntry) {
   EXPECT_EQ(table_total_size(1), 32 + 384);   // 416
}

TEST(LayoutTest, TableTotalSizeMultipleEntries) {
   EXPECT_EQ(table_total_size(10), 32 + 10 * 384);     // 3872
   EXPECT_EQ(table_total_size(100), 32 + 100 * 384);   // 38432
}

// 
// queue_total_size<T>() tests
// 

TEST(LayoutTest, QueueTotalSizeChar) {
   // Header(16) + align_up(cap * 1, 8) + cap * 4 (atomic<uint32_t>)
   // cap=1: 16 + align_up(1,8) + 4 = 16 + 8 + 4 = 28
   EXPECT_EQ(queue_total_size<char>(1), 28);
   // cap=8: 16 + align_up(8,8) + 32 = 16 + 8 + 32 = 56
   EXPECT_EQ(queue_total_size<char>(8), 56);
}

TEST(LayoutTest, QueueTotalSizeInt32) {
   // Header(16) + align_up(cap * 4, 8) + cap * 4
   // cap=1: 16 + align_up(4,8) + 4 = 16 + 8 + 4 = 28
   EXPECT_EQ(queue_total_size<uint32_t>(1), 28);
   // cap=2: 16 + align_up(8,8) + 8 = 16 + 8 + 8 = 32
   EXPECT_EQ(queue_total_size<uint32_t>(2), 32);
   // cap=5: 16 + align_up(20,8) + 20 = 16 + 24 + 20 = 60
   EXPECT_EQ(queue_total_size<uint32_t>(5), 60);
}

TEST(LayoutTest, QueueTotalSizeDouble) {
   // Header(16) + align_up(cap * 8, 8) + cap * 4
   // cap=1: 16 + align_up(8,8) + 4 = 16 + 8 + 4 = 28
   EXPECT_EQ(queue_total_size<double>(1), 28);
   // cap=2: 16 + align_up(16,8) + 8 = 16 + 16 + 8 = 40
   EXPECT_EQ(queue_total_size<double>(2), 40);
}

// 
// ring_total_size<T>() tests
// 

TEST(LayoutTest, RingTotalSize) {
   // Header(24) + capacity
   EXPECT_EQ(ring_total_size<char>(0), 24);
   EXPECT_EQ(ring_total_size<char>(100), 124);
   EXPECT_EQ(ring_total_size<uint64_t>(50), 74);
}

// 
// array_total_size<T>() tests
// 

TEST(LayoutTest, ArrayTotalSizeChar) {
   // Header(8) + cap * 1
   EXPECT_EQ(array_total_size<char>(0), 8);
   EXPECT_EQ(array_total_size<char>(100), 108);
}

TEST(LayoutTest, ArrayTotalSizeInt32) {
   // Header(8) + cap * 4
   EXPECT_EQ(array_total_size<uint32_t>(0), 8);
   EXPECT_EQ(array_total_size<uint32_t>(10), 48);
}

TEST(LayoutTest, ArrayTotalSizeDouble) {
   // Header(8) + cap * 8
   EXPECT_EQ(array_total_size<double>(0), 8);
   EXPECT_EQ(array_total_size<double>(5), 48);
}

// 
// Latest<T> layout tests (unified binary format with pthread mutex + condvar)
// 

TEST(LayoutTest, LatestPthreadSizes) {
   // Print pthread structure sizes for cross-language comparison
   std::cout << "pthread_mutex_t: size=" << PTHREAD_MUTEX_SIZE
             << ", align=" << PTHREAD_MUTEX_ALIGN << std::endl;
   std::cout << "pthread_cond_t: size=" << PTHREAD_COND_SIZE
             << ", align=" << PTHREAD_COND_ALIGN << std::endl;
   EXPECT_LE(PTHREAD_MUTEX_ALIGN, 8);
   EXPECT_LE(PTHREAD_COND_ALIGN, 8);
}

TEST(LayoutTest, LatestLayoutElemSize4) {
   // elem_size=4: aligned_size=8
   // version_end = 8 + 8 + 8 = 24
   // mutex_start = align_up(24, mutex_align)
   // cond_start = align_up(mutex_start + mutex_size, cond_align)
   // total = cond_start + cond_size
   std::size_t elem_size = 4;
   std::size_t total = latest_total_size(elem_size);
   std::size_t ver_off = latest_version_offset(0, elem_size);
   std::size_t mutex_off = latest_mutex_offset(0, elem_size);
   std::size_t cond_off = latest_cond_offset(0, elem_size);

   std::cout << "Latest layout (elem_size=4): total=" << total
             << ", version=" << ver_off << ", mutex=" << mutex_off
             << ", cond=" << cond_off << std::endl;

   // Verify version is 8-aligned
   EXPECT_EQ(ver_off % 8, 0u);
   // Verify ordering: version < mutex < cond < total
   EXPECT_LT(ver_off, mutex_off);
   EXPECT_LT(mutex_off, cond_off);
   EXPECT_EQ(total, cond_off + PTHREAD_COND_SIZE);
}

TEST(LayoutTest, LatestLayoutElemSize8) {
   // elem_size=8: aligned_size=8
   std::size_t elem_size = 8;
   std::size_t total = latest_total_size(elem_size);
   std::size_t ver_off = latest_version_offset(0, elem_size);
   std::size_t mutex_off = latest_mutex_offset(0, elem_size);
   std::size_t cond_off = latest_cond_offset(0, elem_size);

   std::cout << "Latest layout (elem_size=8): total=" << total
             << ", version=" << ver_off << ", mutex=" << mutex_off
             << ", cond=" << cond_off << std::endl;

   // Verify version is 8-aligned
   EXPECT_EQ(ver_off % 8, 0u);
   // Verify ordering: version < mutex < cond < total
   EXPECT_LT(ver_off, mutex_off);
   EXPECT_LT(mutex_off, cond_off);
   EXPECT_EQ(total, cond_off + PTHREAD_COND_SIZE);
}

TEST(LayoutTest, LatestLayoutElemSize16) {
   // elem_size=16: aligned_size=16
   std::size_t elem_size = 16;
   std::size_t total = latest_total_size(elem_size);
   std::size_t ver_off = latest_version_offset(0, elem_size);
   std::size_t mutex_off = latest_mutex_offset(0, elem_size);
   std::size_t cond_off = latest_cond_offset(0, elem_size);

   std::cout << "Latest layout (elem_size=16): total=" << total
             << ", version=" << ver_off << ", mutex=" << mutex_off
             << ", cond=" << cond_off << std::endl;

   // Verify version is 8-aligned
   EXPECT_EQ(ver_off % 8, 0u);
   // Verify ordering: version < mutex < cond < total
   EXPECT_LT(ver_off, mutex_off);
   EXPECT_LT(mutex_off, cond_off);
   EXPECT_EQ(total, cond_off + PTHREAD_COND_SIZE);
}

// Print all Latest layout values for cross-language comparison
TEST(LayoutTest, LatestLayoutSummary) {
   std::cout << "\n=== C++ Latest Layout Summary ===" << std::endl;
   std::cout << "pthread_mutex_t: size=" << PTHREAD_MUTEX_SIZE
             << ", align=" << PTHREAD_MUTEX_ALIGN << std::endl;
   std::cout << "pthread_cond_t: size=" << PTHREAD_COND_SIZE
             << ", align=" << PTHREAD_COND_ALIGN << std::endl;
   for (std::size_t elem_size : {4, 8, 16}) {
      std::cout << "elem_size=" << elem_size
                << ": total=" << latest_total_size(elem_size)
                << ", version=" << latest_version_offset(0, elem_size)
                << ", mutex=" << latest_mutex_offset(0, elem_size)
                << ", cond=" << latest_cond_offset(0, elem_size) << std::endl;
   }
   std::cout << "=== End C++ Summary ===\n" << std::endl;
}
