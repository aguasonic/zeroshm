// Producer creates shared memory, writes data to an array, and pushes to a
// queue.
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <thread>

#include <zeroshm/array.h>
#include <zeroshm/memory.h>
#include <zeroshm/queue.h>

int main() {
   std::cout << "=== ZeroSHM C++ Producer ===" << std::endl;

   const std::string shmName = "/zeroshm_example";
   const std::string arrayName = "temperatures";
   const std::string queueName = "events";
   const size_t arraySize = 100;
   const size_t queueCap = 32;
   const size_t numReadings = 10;

   // Clean up any existing shared memory
   ::shm_unlink(shmName.c_str());

   // Create shared memory segment (1 MB)
   zeroshm::Memory mem(shmName, 1024 * 1024, 64);

   // Create a typed array for temperature readings
   zeroshm::Array<double> arr(mem, arrayName, arraySize);

   // Create a lock-free queue for event notifications
   zeroshm::Queue<int64_t> q(mem, queueName, queueCap);

   // Simulate sensor readings: write to array and push notifications to queue
   std::cout << "Producing " << numReadings << " sensor readings..."
             << std::endl;
   for (size_t i = 0; i < numReadings; i++) {
      // Generate a synthetic temperature reading
      double temp = 20.0 + static_cast<double>(i) * 0.5;
      arr[i] = temp;

      // Push a timestamp notification to the queue
      auto ts =
          std::chrono::high_resolution_clock::now().time_since_epoch().count();
      auto pushResult = q.push(ts);
      if (!pushResult) {
         std::cout << "Queue full, waiting..." << std::endl;
         std::this_thread::sleep_for(std::chrono::milliseconds(10));
         (void)q.push(ts);
      }
      std::cout << "  Reading " << i << ": temp=" << temp << "C, event pushed"
                << std::endl;

      // Small delay to simulate real sensor interval
      std::this_thread::sleep_for(std::chrono::milliseconds(100));
   }

   // Signal completion by pushing a sentinel value (-1)
   (void)q.push(-1);
   std::cout << "Sent completion signal. Consumer can now exit." << std::endl;

   // Keep the process alive briefly so the consumer can read all data
   std::cout << "Waiting for consumer to finish reading..." << std::endl;
   std::this_thread::sleep_for(std::chrono::seconds(2));

   std::cout << "Producer done." << std::endl;

   // Clean up shared memory (only the owner should unlink)
   ::shm_unlink(shmName.c_str());

   return 0;
}
