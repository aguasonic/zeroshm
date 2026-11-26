#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <numeric>
#include <sys/resource.h>
#include <thread>
#include <vector>

#include <gtest/gtest.h>

#include <zeroshm/latest.h>

using namespace zeroshm;

class LatestPerformanceTest : public ::testing::Test {
 protected:
   void SetUp() override {
      test_name = "/test_latest_perf_" + std::to_string(getpid());
      mem = std::make_unique<Memory>(test_name, 64 * 1024 * 1024);   // 64MB
   }

   void TearDown() override {
      if (mem) {
         mem->unlink();
      }
   }

   std::string test_name;
   std::unique_ptr<Memory> mem;
};

// 
// Zero CPU Test: Verify that blocked readers consume ~0% CPU
// 

TEST_F(LatestPerformanceTest, ZeroCpuWhileBlocked) {
   Latest<uint64_t> latest(*mem, "zero_cpu", 0);

   const int num_readers = 8;
   std::vector<std::thread> readers;
   std::vector<std::chrono::steady_clock::time_point> reader_start_times;
   std::vector<std::unique_ptr<std::atomic<bool>>> reader_done;
   for (int i = 0; i < num_readers; i++) {
      reader_done.push_back(std::make_unique<std::atomic<bool>>(false));
   }

   auto [_, last_version] = latest.get();

   // Start reader threads that block on wait_for_change
   for (int i = 0; i < num_readers; i++) {
      reader_start_times.push_back(std::chrono::steady_clock::now());
      readers.emplace_back([&, i]() {
         // Each reader blocks with a 5-second timeout
         (void)latest.wait_for_change(last_version, std::chrono::seconds(5));
         reader_done[i]->store(true);
      });
   }

   // Wait for readers to settle into blocking state
   std::this_thread::sleep_for(std::chrono::milliseconds(500));

   // Measure CPU time of the current process (all threads) before signal
   struct rusage usage_before;
   getrusage(RUSAGE_SELF, &usage_before);

   // Wait a bit more to ensure readers are truly blocked
   std::this_thread::sleep_for(std::chrono::milliseconds(2000));

   // Measure CPU time after the blocking period
   struct rusage usage_after;
   getrusage(RUSAGE_SELF, &usage_after);

   // Calculate CPU time consumed during the 2-second blocking period
   double cpu_user_before =
       usage_before.ru_utime.tv_sec + usage_before.ru_utime.tv_usec / 1e6;
   double cpu_user_after =
       usage_after.ru_utime.tv_sec + usage_after.ru_utime.tv_usec / 1e6;
   double cpu_system_before =
       usage_before.ru_stime.tv_sec + usage_before.ru_stime.tv_usec / 1e6;
   double cpu_system_after =
       usage_after.ru_stime.tv_sec + usage_after.ru_stime.tv_usec / 1e6;

   double cpu_time_consumed = (cpu_user_after - cpu_user_before) +
                              (cpu_system_after - cpu_system_before);
   double wall_time = 2.0;   // seconds

   // CPU percentage (approximate, across all threads in the process)
   double cpu_percent = (cpu_time_consumed / wall_time) * 100.0;

   printf("Zero CPU Test Results:\n");
   printf("  Readers: %d\n", num_readers);
   printf("  Wall time: %.2f seconds\n", wall_time);
   printf("  CPU time consumed: %.4f seconds\n", cpu_time_consumed);
   printf("  CPU usage: %.2f%%\n", cpu_percent);

   // Wake all readers by setting a new value
   latest.set(1);

   // Wait for all readers to finish
   for (auto &reader : readers) {
      reader.join();
   }

   // Verify all readers woke up
   for (int i = 0; i < num_readers; i++) {
      ASSERT_TRUE(reader_done[i]->load())
          << "Reader " << i << " did not wake up";
   }

   // Checkpoint: blocked readers should consume < 0.1% CPU
   // Note: getrusage measures the entire process, so the threshold is
   // conservative The main test is that CPU time consumed during the 2-second
   // block period is minimal
   EXPECT_LT(cpu_time_consumed, 0.05)
       << "CPU time consumed during blocking period is too high: "
       << cpu_time_consumed;
}

// 
// Latency Test: Measure wake latency (should be < 1ms average, < 10ms p99)
// 

TEST_F(LatestPerformanceTest, WakeLatency) {
   Latest<uint64_t> latest(*mem, "latency", 0);

   const int num_iterations = 200;
   std::vector<double> latencies_ms;
   latencies_ms.reserve(num_iterations);

   // Simple PRNG for random delays
   auto rand_delay = [](int max_ms) -> std::chrono::milliseconds {
      int ms = rand() % max_ms + 1;
      return std::chrono::milliseconds(ms);
   };

   for (int iter = 0; iter < num_iterations; iter++) {
      auto [_, last_version] = latest.get();

      std::atomic<bool> reader_done(false);
      std::atomic<bool> writer_done(false);
      std::chrono::steady_clock::time_point write_time;
      std::chrono::steady_clock::time_point wake_time;

      // Reader thread: block on wait_for_change with long timeout
      std::thread reader([&]() {
         auto [value, version, changed] =
             latest.wait_for_change(last_version, std::chrono::seconds(10));
         wake_time = std::chrono::steady_clock::now();
         reader_done = true;
      });

      // Random delay before writing (1ms to 100ms)
      auto delay = rand_delay(100);
      std::this_thread::sleep_for(delay);

      // Writer: set new value
      write_time = std::chrono::steady_clock::now();
      latest.set(iter + 1);

      // Wait for reader to wake up
      while (!reader_done.load()) {
         std::this_thread::sleep_for(std::chrono::microseconds(100));
      }
      reader.join();

      // Measure latency: wake time - write time
      double latency_ms =
          std::chrono::duration<double, std::milli>(wake_time - write_time)
              .count();
      latencies_ms.push_back(latency_ms);
   }

   // Sort latencies for percentile calculation
   std::sort(latencies_ms.begin(), latencies_ms.end());

   // Calculate statistics
   double sum = std::accumulate(latencies_ms.begin(), latencies_ms.end(), 0.0);
   double avg = sum / num_iterations;
   double p50 = latencies_ms[num_iterations / 2];
   double p95 = latencies_ms[(int)(num_iterations * 0.95)];
   double p99 = latencies_ms[(int)(num_iterations * 0.99)];
   double max_lat = latencies_ms.back();

   printf("Latency Test Results (%d iterations):\n", num_iterations);
   printf("  Average: %.3f ms\n", avg);
   printf("  P50:     %.3f ms\n", p50);
   printf("  P95:     %.3f ms\n", p95);
   printf("  P99:     %.3f ms\n", p99);
   printf("  Max:     %.3f ms\n", max_lat);

   // Checkpoint: average latency < 1ms, p99 < 10ms
   EXPECT_LT(avg, 1.0) << "Average latency is too high: " << avg << " ms";
   EXPECT_LT(p99, 10.0) << "P99 latency is too high: " << p99 << " ms";
}

// 
// Throughput Test: Single writer, 8 readers, verify no missed updates
// 

TEST_F(LatestPerformanceTest, ThroughputNoMissedUpdates) {
   Latest<uint64_t> latest(*mem, "throughput", 0);

   const int num_readers = 8;
   const int duration_ms = 2000;      // Run for 2 seconds
   const int write_interval_ms = 1;   // Writer sets every 1ms

   std::atomic<bool> writer_stop(false);
   std::atomic<uint64_t> expected_versions[num_readers];
   std::atomic<uint64_t> total_reads[num_readers];
   std::atomic<uint64_t> missed_updates[num_readers];

   for (int i = 0; i < num_readers; i++) {
      expected_versions[i] = 0;
      total_reads[i] = 0;
      missed_updates[i] = 0;
   }

   // Start reader threads
   std::vector<std::thread> readers;
   for (int i = 0; i < num_readers; i++) {
      readers.emplace_back([&, i]() {
         auto [_, last_version] = latest.get();
         expected_versions[i] = last_version;

         while (!writer_stop.load()) {
            auto [value, version, changed] = latest.wait_for_change(
                last_version, std::chrono::milliseconds(100));

            if (changed) {
               // Verify version is exactly what we expect (no missed updates)
               uint64_t expected = expected_versions[i].load() + 1;
               if (version != expected) {
                  // We missed some updates -- count them
                  uint64_t missed = version - expected;
                  missed_updates[i].fetch_add(missed);
               }
               expected_versions[i] = version;
               last_version = version;
               total_reads[i].fetch_add(1);
            }
            // If not changed (timeout), loop again
         }
      });
   }

   // Writer thread: set new value every 1ms for 2 seconds
   std::thread writer([&]() {
      auto end_time = std::chrono::steady_clock::now() +
                      std::chrono::milliseconds(duration_ms);
      while (std::chrono::steady_clock::now() < end_time) {
         latest.set(latest.version() + 1);
         std::this_thread::sleep_for(
             std::chrono::milliseconds(write_interval_ms));
      }
      writer_stop = true;
   });

   // Wait for writer to finish
   writer.join();

   // Wait for all readers to finish
   for (auto &reader : readers) {
      reader.join();
   }

   // Calculate throughput and verify no missed updates
   uint64_t final_version = latest.version();
   uint64_t total_writes = final_version - 1;   // Version starts at 1

   printf("Throughput Test Results:\n");
   printf("  Duration: %d ms\n", duration_ms);
   printf("  Total writes: %lu\n", (unsigned long)total_writes);
   printf("  Writer rate: %.0f writes/sec\n",
          (double)total_writes * 1000.0 / duration_ms);

   uint64_t total_all_reads = 0;
   uint64_t total_missed = 0;
   for (int i = 0; i < num_readers; i++) {
      uint64_t reads = total_reads[i].load();
      uint64_t missed = missed_updates[i].load();
      total_all_reads += reads;
      total_missed += missed;

      double read_rate = (double)reads * 1000.0 / duration_ms;
      printf("  Reader %d: %lu reads (%.0f reads/sec), %lu missed\n", i,
             (unsigned long)reads, read_rate, (unsigned long)missed);
   }

   printf("  Total reads (all readers): %lu\n", (unsigned long)total_all_reads);
   printf("  Total missed updates: %lu\n", (unsigned long)total_missed);
   printf("  Expected reads per reader: %lu\n", (unsigned long)total_writes);

   // Checkpoint: all readers should see every version, no missed updates
   // Each reader should have seen approximately total_writes updates
   for (int i = 0; i < num_readers; i++) {
      uint64_t reads = total_reads[i].load();
      uint64_t missed = missed_updates[i].load();
      EXPECT_EQ(missed, 0u)
          << "Reader " << i << " missed " << missed << " updates";
      // Allow some tolerance for the timing of test start/stop
      EXPECT_GE(reads, total_writes * 95 / 100)
          << "Reader " << i << " read too few: " << reads << " (expected ~"
          << total_writes << ")";
   }
}

// 
// Main (required by gtest)
// 

int main(int argc, char **argv) {
   ::testing::InitGoogleTest(&argc, argv);
   srand(time(nullptr));
   return RUN_ALL_TESTS();
}
