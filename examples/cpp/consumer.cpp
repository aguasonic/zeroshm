// Consumer opens shared memory, reads from an array, and pops from a queue.
#include <chrono>
#include <iostream>
#include <numeric>
#include <thread>

#include <zeroshm/array.h>
#include <zeroshm/memory.h>
#include <zeroshm/queue.h>

int main() {
   std::cout << "=== ZeroSHM C++ Consumer ===" << std::endl;

   const std::string shmName = "/zeroshm_example";
   const std::string arrayName = "temperatures";
   const std::string queueName = "events";

   // Wait for producer to create shared memory
   std::cout << "Waiting for producer to create shared memory..." << std::endl;
   zeroshm::Memory mem(shmName);

   // Open the temperature array (capacity=0 means open existing)
   zeroshm::Array<double> arr(mem, arrayName);

   // Open the event queue
   zeroshm::Queue<int64_t> q(mem, queueName);

   // Read data from queue until completion signal
   std::cout << "Consuming events..." << std::endl;
   size_t count = 0;
   for (;;) {
      auto popResult = q.pop();
      if (!popResult) {
         // Queue empty, wait a bit and retry
         std::this_thread::sleep_for(std::chrono::milliseconds(50));
         continue;
      }

      int64_t ts = *popResult;

      // Check for completion signal
      if (ts == -1) {
         std::cout << "Received completion signal." << std::endl;
         break;
      }

      count++;

      // Read the corresponding temperature from the array
      double temp = arr[count - 1];
      std::cout << "  Event " << count << ": timestamp=" << ts
                << ", temp=" << temp << "C" << std::endl;
   }

   std::cout << "Consumed " << count << " events total." << std::endl;

   // Read all temperatures from the array and compute stats
   std::cout << "\nReading all temperatures from array:" << std::endl;
   double sum = 0.0;
   for (size_t i = 0; i < count; i++) {
      double temp = arr[i];
      sum += temp;
      std::cout << "  arr[" << i << "] = " << temp << "C" << std::endl;
   }
   if (count > 0) {
      std::cout << "Average temperature: " << (sum / static_cast<double>(count))
                << "C" << std::endl;
   }

   std::cout << "Consumer done." << std::endl;

   return 0;
}
