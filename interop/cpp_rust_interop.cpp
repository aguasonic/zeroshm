// ZeroIPC Cross-Language Interop Tests (Part 1)
// C++ side of Rust <-> C++ interop verification.
//
// Usage:
//   ./cpp_rust_interop <command> <shm_name> [args...]
//
// Commands:
//   array_create <shm> <name> <capacity> <values...>  - Create array, write
//   values array_read <shm> <name>                            - Read array,
//   print values array_write <shm> <name> <values...>               - Write
//   values to existing array queue_create <shm> <name> <capacity> - Create
//   queue queue_push <shm> <name> <values...>                - Push values to
//   queue queue_pop <shm> <name> <count>                     - Pop N values
//   from queue mutex_create <shm> <name>                          - Create
//   mutex mutex_lock <shm> <name>                            - Lock mutex
//   mutex_unlock <shm> <name>                          - Unlock mutex
//   barrier_create <shm> <name> <participants>         - Create barrier
//   barrier_wait <shm> <name>                          - Wait at barrier
//   signal_create <shm> <name> <initial_value>         - Create signal
//   signal_set <shm> <name> <value>                    - Set signal value
//   signal_get <shm> <name>                            - Get signal value
//   latest_create <shm> <name> <initial_value>         - Create Latest with
//   initial value latest_get <shm> <name>                            - Read
//   current value and version latest_set <shm> <name> <value> - Set new value
//   latest_wait <shm> <name> <last_version> <timeout_ms> - Wait for change
//   cleanup <shm>                                      - Unlink shared memory

#include <chrono>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <thread>
#include <vector>
#include <zeroshm/array.h>
#include <zeroshm/barrier.h>
#include <zeroshm/latest.h>
#include <zeroshm/memory.h>
#include <zeroshm/mutex.h>
#include <zeroshm/queue.h>
#include <zeroshm/signal.h>

static std::string ensureName(const std::string &name) {
   if (!name.empty() && name[0] == '/') {
      return name;
   }
   return "/" + name;
}

static int cmdArrayCreate(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 4) {
      std::cerr << "Usage: array_create <shm> <name> <capacity> <values...>"
                << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   int capacity = std::atoi(argv[1]);

   std::vector<double> values;
   for (int i = 2; i < argc; i++) {
      values.push_back(std::stod(argv[i]));
   }

   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);

   zeroshm::Memory mem(shm, 1024 * 1024, 64);
   zeroshm::Array<double> array(mem, name, capacity);

   for (size_t i = 0; i < values.size(); i++) {
      array[i] = values[i];
   }

   std::cout << "C++: Created array '" << name << "' with " << values.size()
             << " values" << std::endl;
   for (size_t i = 0; i < values.size(); i++) {
      std::cout << "C++: array[" << i << "] = " << values[i] << std::endl;
   }

   return 0;
}

static int cmdArrayRead(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: array_read <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);

   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Array<double> array(mem, name);

   std::cout << "C++: Opened array '" << name
             << "', capacity=" << array.capacity() << std::endl;
   for (size_t i = 0; i < array.capacity(); i++) {
      std::cout << "C++: array[" << i << "] = " << array[i] << std::endl;
   }

   return 0;
}

static int cmdArrayWrite(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: array_write <shm> <name> <values...>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);

   std::vector<double> values;
   for (int i = 1; i < argc; i++) {
      values.push_back(std::stod(argv[i]));
   }

   std::string shm = ensureName(shmName);
   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Array<double> array(mem, name);

   for (size_t i = 0; i < values.size(); i++) {
      array[i] = values[i];
   }

   std::cout << "C++: Wrote " << values.size() << " values to array '" << name
             << "'" << std::endl;
   return 0;
}

static int cmdQueueCreate(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: queue_create <shm> <name> <capacity>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   int capacity = std::atoi(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);

   zeroshm::Memory mem(shm, 1024 * 1024, 64);
   zeroshm::Queue<uint32_t> queue(mem, name, capacity);

   std::cout << "C++: Created queue '" << name
             << "', capacity=" << queue.capacity() << std::endl;
   return 0;
}

static int cmdQueuePush(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: queue_push <shm> <name> <values...>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);

   std::vector<uint32_t> values;
   for (int i = 1; i < argc; i++) {
      values.push_back(static_cast<uint32_t>(std::stoul(argv[i])));
   }

   std::string shm = ensureName(shmName);
   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Queue<uint32_t> queue(mem, name);

   for (auto v : values) {
      if (!queue.push(v)) {
         std::cerr << "C++: Failed to push " << v << " (queue full)"
                   << std::endl;
         return 1;
      }
   }

   std::cout << "C++: Pushed " << values.size() << " values to queue '" << name
             << "'" << std::endl;
   return 0;
}

static int cmdQueuePop(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: queue_pop <shm> <name> <count>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   int count = std::atoi(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Queue<uint32_t> queue(mem, name);

   for (int i = 0; i < count; i++) {
      auto val = queue.pop();
      if (!val) {
         std::cout << "C++: Queue empty" << std::endl;
         return 1;
      }
      std::cout << "C++: Popped " << *val << " from queue '" << name << "'"
                << std::endl;
   }

   return 0;
}

static int cmdMutexCreate(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: mutex_create <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);

   zeroshm::Memory mem(shm, 1024 * 1024, 64);
   zeroshm::Mutex mutex(mem, name);

   std::cout << "C++: Created mutex '" << name << "'" << std::endl;
   return 0;
}

static int cmdMutexLock(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: mutex_lock <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);

   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Mutex mutex(mem, name);

   mutex.lock();
   std::cout << "C++: Locked mutex '" << name << "'" << std::endl;

   // Hold the lock for a moment so another process can detect it's locked
   std::this_thread::sleep_for(std::chrono::milliseconds(100));

   mutex.unlock();
   return 0;
}

static int cmdMutexUnlock(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: mutex_unlock <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);

   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Mutex mutex(mem, name);

   // Try to lock and unlock (verifies cross-language unlock worked)
   mutex.lock();
   std::cout << "C++: Acquired mutex '" << name
             << "' (was unlocked by other language)" << std::endl;
   mutex.unlock();
   return 0;
}

static int cmdBarrierCreate(const std::string &shmName, int argc,
                            char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: barrier_create <shm> <name> <participants>"
                << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   int participants = std::atoi(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);

   zeroshm::Memory mem(shm, 1024 * 1024, 64);
   zeroshm::Barrier barrier(mem, name, participants);

   std::cout << "C++: Created barrier '" << name << "' with " << participants
             << " participants" << std::endl;
   return 0;
}

static int cmdBarrierWait(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: barrier_wait <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);

   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Barrier barrier(mem, name);

   std::cout << "C++: Waiting at barrier '" << name << "'..." << std::endl;
   barrier.wait();
   std::cout << "C++: Barrier '" << name << "' released" << std::endl;

   return 0;
}

static int cmdSignalCreate(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: signal_create <shm> <name> <initial_value>"
                << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   uint64_t initial = std::stoul(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);

   zeroshm::Memory mem(shm, 1024 * 1024, 64);
   zeroshm::Signal<uint64_t> signal(mem, name, initial);

   std::cout << "C++: Created signal '" << name << "' with initial value "
             << initial << std::endl;
   return 0;
}

static int cmdSignalSet(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: signal_set <shm> <name> <value>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   uint64_t value = std::stoul(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Signal<uint64_t> signal(mem, name,
                                    zeroshm::Signal<uint64_t>::OpenExisting{});

   signal.set(value);
   std::cout << "C++: Set signal '" << name << "' to value " << value
             << std::endl;
   return 0;
}

static int cmdSignalGet(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: signal_get <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);

   zeroshm::Memory mem(shm, 0, 64);
   zeroshm::Signal<uint64_t> signal(mem, name,
                                    zeroshm::Signal<uint64_t>::OpenExisting{});

   uint64_t value = signal.get();
   uint64_t version = signal.version();
   std::cout << "C++: Signal '" << name << "' value = " << value
             << ", version = " << version << std::endl;
   return 0;
}

static int cmdLatestCreate(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: latest_create <shm> <name> <initial_value>"
                << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   uint64_t initial = std::stoull(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);

   zeroshm::Memory mem(shm, 1024 * 1024, 64);
   auto result = zeroshm::Latest<uint64_t>::create(mem, name, initial);
   if (!result) {
      std::cerr << "C++: Failed to create Latest: "
                << zeroshm::error_message(result.error()) << std::endl;
      return 1;
   }

   std::cout << "C++: Created Latest '" << name << "' with value " << initial
             << std::endl;
   return 0;
}

static int cmdLatestGet(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 1) {
      std::cerr << "Usage: latest_get <shm> <name>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   std::string shm = ensureName(shmName);

   zeroshm::Memory mem(shm, 0, 64);
   auto result = zeroshm::Latest<uint64_t>::open(mem, name);
   if (!result) {
      std::cerr << "C++: Failed to open Latest: "
                << zeroshm::error_message(result.error()) << std::endl;
      return 1;
   }

   auto latest = std::move(result.value());
   auto [value, version] = latest.get();
   std::cout << "C++: Latest '" << name << "' value = " << value
             << ", version = " << version << std::endl;
   return 0;
}

static int cmdLatestSet(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 2) {
      std::cerr << "Usage: latest_set <shm> <name> <value>" << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   uint64_t value = std::stoull(argv[1]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory mem(shm, 0, 64);
   auto result = zeroshm::Latest<uint64_t>::open(mem, name);
   if (!result) {
      std::cerr << "C++: Failed to open Latest: "
                << zeroshm::error_message(result.error()) << std::endl;
      return 1;
   }

   auto latest = std::move(result.value());
   latest.set(value);
   std::cout << "C++: Set Latest '" << name << "' to " << value << std::endl;
   return 0;
}

static int cmdLatestWait(const std::string &shmName, int argc, char *argv[]) {
   if (argc < 3) {
      std::cerr << "Usage: latest_wait <shm> <name> <last_version> <timeout_ms>"
                << std::endl;
      return 1;
   }

   std::string name = ensureName(argv[0]);
   uint64_t lastVersion = std::stoull(argv[1]);
   int timeoutMs = std::stoi(argv[2]);

   std::string shm = ensureName(shmName);
   zeroshm::Memory mem(shm, 0, 64);
   auto result = zeroshm::Latest<uint64_t>::open(mem, name);
   if (!result) {
      std::cerr << "C++: Failed to open Latest: "
                << zeroshm::error_message(result.error()) << std::endl;
      return 1;
   }

   auto latest = std::move(result.value());
   auto [value, version, changed] = latest.wait_for_change(
       lastVersion, std::chrono::milliseconds(timeoutMs));

   if (changed) {
      std::cout << "C++: Latest '" << name << "' changed: value = " << value
                << ", version = " << version << std::endl;
   } else {
      std::cout << "C++: Latest '" << name << "' timeout: value = " << value
                << ", version = " << version << std::endl;
   }
   return 0;
}

static int cmdCleanup(const std::string &shmName) {
   std::string shm = ensureName(shmName);
   zeroshm::Memory::unlink(shm);
   std::cout << "C++: Cleaned up " << shm << std::endl;
   return 0;
}

int main(int argc, char *argv[]) {
   if (argc < 3) {
      std::cerr << "Usage: cpp_rust_interop <command> <shm_name> [args...]"
                << std::endl;
      return 1;
   }

   std::string command = argv[1];
   std::string shmName = argv[2];
   int argCount = argc - 3;
   char **args = argv + 3;

   try {
      if (command == "array_create") {
         return cmdArrayCreate(shmName, argCount, args);
      } else if (command == "array_read") {
         return cmdArrayRead(shmName, argCount, args);
      } else if (command == "array_write") {
         return cmdArrayWrite(shmName, argCount, args);
      } else if (command == "queue_create") {
         return cmdQueueCreate(shmName, argCount, args);
      } else if (command == "queue_push") {
         return cmdQueuePush(shmName, argCount, args);
      } else if (command == "queue_pop") {
         return cmdQueuePop(shmName, argCount, args);
      } else if (command == "mutex_create") {
         return cmdMutexCreate(shmName, argCount, args);
      } else if (command == "mutex_lock") {
         return cmdMutexLock(shmName, argCount, args);
      } else if (command == "mutex_unlock") {
         return cmdMutexUnlock(shmName, argCount, args);
      } else if (command == "barrier_create") {
         return cmdBarrierCreate(shmName, argCount, args);
      } else if (command == "barrier_wait") {
         return cmdBarrierWait(shmName, argCount, args);
      } else if (command == "signal_create") {
         return cmdSignalCreate(shmName, argCount, args);
      } else if (command == "signal_set") {
         return cmdSignalSet(shmName, argCount, args);
      } else if (command == "signal_get") {
         return cmdSignalGet(shmName, argCount, args);
      } else if (command == "latest_create") {
         return cmdLatestCreate(shmName, argCount, args);
      } else if (command == "latest_get") {
         return cmdLatestGet(shmName, argCount, args);
      } else if (command == "latest_set") {
         return cmdLatestSet(shmName, argCount, args);
      } else if (command == "latest_wait") {
         return cmdLatestWait(shmName, argCount, args);
      } else if (command == "cleanup") {
         return cmdCleanup(shmName);
      } else {
         std::cerr << "Unknown command: " << command << std::endl;
         return 1;
      }
   } catch (const std::exception &e) {
      std::cerr << "ERROR: " << e.what() << std::endl;
      return 1;
   }
}
