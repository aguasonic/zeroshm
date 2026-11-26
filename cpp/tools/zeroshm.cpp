// ZeroSHM Shared Memory Inspector CLI Tool v3
// Enhanced with REPL mode and creation/manipulation support
#include <atomic>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <getopt.h>
#include <iomanip>
#include <iostream>
#include <map>
#include <memory>
#include <print>
#include <sstream>
#include <string>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <vector>

// Include ZeroSHM headers for creation and manipulation
#include <zeroshm/array.h>
#include <zeroshm/barrier.h>
#include <zeroshm/channel.h>
#include <zeroshm/latch.h>
#include <zeroshm/map.h>
#include <zeroshm/memory.h>
#include <zeroshm/pool.h>
#include <zeroshm/queue.h>
#include <zeroshm/ring.h>
#include <zeroshm/semaphore.h>
#include <zeroshm/set.h>
#include <zeroshm/stack.h>
#include <zeroshm/stream.h>

//- in this directory.
#include "vfs.h"

// Matches SPECIFICATION.md v3.0 exactly
struct TableHeader {
   uint32_t magic;         // 0x5A49504D ('ZSHM')
   uint32_t version;       // Format version (currently 3)
   uint32_t entry_count;   // Number of active entries
   uint32_t max_entries;   // Maximum number of table entries
   uint64_t memory_size;   // Total memory size
   uint64_t next_offset;   // Next allocation offset
};

struct TableEntry {
   uint64_t hash;       // xxhash3-64 of name (seed = 0x2FE7)
   uint64_t offset;     // Offset from base
   uint64_t size;       // Allocated size
   char name[256];      // null-terminated name (max 254 chars + 1 null = 255
                        // bytes, 1 byte padding)
   char padding[104];   // Padding to reach 384 bytes (matches Rust/Go/Cpp
                        // implementations)
};

static_assert(sizeof(TableEntry) == 384,
              "TableEntry must be exactly 384 bytes to match library layout");

// Structure headers for detection
struct ArrayHeader {
   uint64_t capacity;
};

struct QueueHeader {
   uint64_t head;
   uint64_t tail;
   uint64_t capacity;
};

struct StackHeader {
   int32_t top;
   uint32_t capacity;
   uint32_t elem_size;
};

struct SemaphoreHeader {
   std::atomic<int32_t> count;
   std::atomic<int32_t> waiting;
   int32_t max_count;
   int32_t _padding;
};

struct BarrierHeader {
   std::atomic<int32_t> arrived;
   std::atomic<int32_t> generation;
   int32_t num_participants;
   int32_t _padding;
};

struct LatchHeader {
   std::atomic<int32_t> count;
   int32_t initial_count;
   int32_t _padding[2];
};

class SharedMemoryInspector {
 private:
   std::string shm_name_;
   void *base_ = nullptr;
   size_t size_ = 0;
   int fd_ = -1;
   TableHeader *header_ = nullptr;
   TableEntry *entries_ = nullptr;
   bool read_write_ = false;

   static constexpr uint32_t MAGIC = 0x5A53484D;   // 'ZSHM'

 public:
   SharedMemoryInspector(const std::string &name, bool read_write = false)
       : shm_name_(name), read_write_(read_write) {
      if (!shm_name_.empty() && shm_name_[0] != '/') {
         shm_name_ = "/" + shm_name_;
      }
   }

   ~SharedMemoryInspector() {
      cleanup();
   }

   bool open() {
      // Hash the logical name to produce the OS-level segment name
      std::string os_name = zeroshm::Memory::os_segment_name(shm_name_);

      int flags = read_write_ ? O_RDWR : O_RDONLY;
      fd_ = shm_open(os_name.c_str(), flags, 0);
      if (fd_ == -1) {
         std::cerr << "Error: Failed to open shared memory '" << shm_name_
                   << "': " << strerror(errno) << "\n";
         return false;
      }

      struct stat st;
      if (fstat(fd_, &st) == -1) {
         std::cerr << "Error: Failed to get size of shared memory: "
                   << strerror(errno) << "\n";
         cleanup();
         return false;
      }
      size_ = st.st_size;

      int prot = read_write_ ? (PROT_READ | PROT_WRITE) : PROT_READ;
      base_ = mmap(nullptr, size_, prot, MAP_SHARED, fd_, 0);
      if (base_ == MAP_FAILED) {
         std::cerr << "Error: Failed to map shared memory: " << strerror(errno)
                   << "\n";
         cleanup();
         return false;
      }

      header_ = static_cast<TableHeader *>(base_);
      if (header_->magic != MAGIC) {
         std::cerr << "Error: Invalid magic number. Expected 0x" << std::hex
                   << MAGIC << ", got 0x" << header_->magic << std::dec << "\n";
         cleanup();
         return false;
      }

      entries_ = reinterpret_cast<TableEntry *>(static_cast<char *>(base_) +
                                                sizeof(TableHeader));

      return true;
   }

   void printSummary() {
      std::println("\n=== Shared Memory Summary ===");
      std::println("Name: {}", shm_name_);
      std::println("Mode: {}", read_write_ ? "Read/Write" : "Read-Only");
      std::println("Total Size: {} ({} bytes)", formatSize(size_), size_);
      std::println("Format Version: {}", header_->version);
      std::println("Active Entries: {}", header_->entry_count);
      std::println("Next Allocation Offset: {:#x} ({} bytes)",
                   header_->next_offset, header_->next_offset);

      size_t used = header_->next_offset;
      size_t free = (used < size_) ? size_ - used : 0;
      double usage = (size_ > 0) ? (100.0 * used / size_) : 0;

      std::println("Memory Used: {} ({:.1}%)", formatSize(used), usage);
      std::println("Memory Free: {}", formatSize(free));
   }

   void printTable(bool verbose = false) {
      std::println("\n=== Table Entries ===");

      if (header_->entry_count == 0) {
         std::println("No entries in table");
         return;
      }

      if (verbose) {
         std::println("{:<4}{:<64}{:<12}{:<12}{:<15}", "#", "Name", "Offset",
                      "Size", "Type");
      } else {
         std::println("{:<4}{:<64}{:<12}{:<12}", "#", "Name", "Offset", "Size");
      }

      std::println("{}", std::string(verbose ? 107 : 92, '-'));

      for (uint32_t i = 0; i < header_->entry_count; i++) {
         if (verbose) {
            std::println("{:<4}{:<64}{:#<12x}{:<12}{:<15}", i, entries_[i].name,
                         entries_[i].offset, formatSize(entries_[i].size),
                         detectStructureType(entries_[i]));
         } else {
            std::println("{:<4}{:<64}{:#<12x}{:<12}", i, entries_[i].name,
                         entries_[i].offset, formatSize(entries_[i].size));
         }
      }
   }

   void printStructureInfo(const std::string &entry_name) {
      TableEntry *entry = findEntry(entry_name);
      if (!entry) {
         std::cerr << "Error: Entry '" << entry_name << "' not found\n";
         return;
      }

      std::println("\n=== Structure: {} ===", entry_name);
      std::println("Offset: {:#x} ({} bytes)", entry->offset, entry->offset);
      std::println("Size: {} ({} bytes)", formatSize(entry->size), entry->size);

      std::string type = detectStructureType(*entry);
      std::println("Type: {}\n", type);

      const char *data = static_cast<const char *>(base_) + entry->offset;

      if (type == "Semaphore") {
         const SemaphoreHeader *hdr =
             reinterpret_cast<const SemaphoreHeader *>(data);
         std::println("Count: {}", hdr->count.load());
         std::println("Waiting: {}", hdr->waiting.load());
         std::println("Max Count: {}", hdr->max_count == 0
                                           ? "unbounded"
                                           : std::to_string(hdr->max_count));
      } else if (type == "Barrier") {
         const BarrierHeader *hdr =
             reinterpret_cast<const BarrierHeader *>(data);
         std::println("Arrived: {} / {}", hdr->arrived.load(),
                      hdr->num_participants);
         std::println("Generation: {}", hdr->generation.load());
         std::println("Num Participants: {}", hdr->num_participants);
      } else if (type == "Latch") {
         const LatchHeader *hdr = reinterpret_cast<const LatchHeader *>(data);
         std::println("Count: {} / {}", hdr->count.load(), hdr->initial_count);
         std::println("Initial Count: {}", hdr->initial_count);
         std::println("Status: {}",
                      hdr->count.load() == 0 ? "Released" : "Counting down");
      } else if (type == "Array") {
         const ArrayHeader *hdr = reinterpret_cast<const ArrayHeader *>(data);
         std::println("Capacity: {} elements", hdr->capacity);
         size_t elem_size = (entry->size - sizeof(ArrayHeader)) / hdr->capacity;
         std::println("Element Size: {} bytes", elem_size);
         std::println("Total Data: {}", formatSize(hdr->capacity * elem_size));
      } else if (type == "Queue") {
         const QueueHeader *hdr = reinterpret_cast<const QueueHeader *>(data);
         std::println("Head: {}", hdr->head);
         std::println("Tail: {}", hdr->tail);
         std::println("Capacity: {} elements", hdr->capacity);

         uint64_t count = (hdr->tail >= hdr->head)
                              ? (hdr->tail - hdr->head)
                              : (hdr->capacity - hdr->head + hdr->tail);
         std::println("Current Items: {}", count);
         std::println("Fill: {:.1}%", 100.0 * count / hdr->capacity);
      } else if (type == "Stack") {
         const StackHeader *hdr = reinterpret_cast<const StackHeader *>(data);
         std::println("Top: {}", hdr->top);
         std::println("Capacity: {} elements", hdr->capacity);
         std::println("Current Items: {}", (hdr->top + 1));
         std::println("Element Size: {} bytes", hdr->elem_size);
      }
   }

   void printHexDump(const std::string &entry_name, size_t max_bytes = 256) {
      TableEntry *entry = findEntry(entry_name);
      if (!entry) {
         std::cerr << "Error: Entry '" << entry_name << "' not found\n";
         return;
      }

      std::println("\n=== Hex Dump: {} ===", entry_name);
      std::println("Offset: {:#x}", entry->offset);
      std::println("Size: {}\n", formatSize(entry->size));

      size_t bytes_to_dump =
          std::min(max_bytes, static_cast<size_t>(entry->size));
      const uint8_t *data = static_cast<const uint8_t *>(base_) + entry->offset;

      for (size_t i = 0; i < bytes_to_dump; i += 16) {
         std::print("{:08x}  ", i);

         for (size_t j = 0; j < 16; j++) {
            if (i + j < bytes_to_dump) {
               std::print("{:02x} ", static_cast<int>(data[i + j]));
            } else {
               std::print("   ");
            }
            if (j == 7)
               std::print(" ");
         }

         std::print(" |");

         for (size_t j = 0; j < 16 && i + j < bytes_to_dump; j++) {
            char c = static_cast<char>(data[i + j]);
            std::print("{}", isprint(c) ? c : '.');
         }

         std::println("|");
      }

      if (bytes_to_dump < entry->size) {
         std::println("... ({} more bytes)", (entry->size - bytes_to_dump));
      }
   }

   void listSharedMemory() {
      std::println("\n=== Available Shared Memory Objects ===");
      std::println("{:<40}{:<12}", "Name", "Size");
      std::println("{}", std::string(52, '-'));
      system("ls -l /dev/shm/ 2>/dev/null | grep -v '^total' | grep -v '^d' | "
             "awk '{printf \"%-40s %s\\n\", $9, $5}'");
   }

   const std::string &getName() const {
      return shm_name_;
   }
   bool isOpen() const {
      return base_ != nullptr;
   }
   bool isReadWrite() const {
      return read_write_;
   }

 private:
   void cleanup() {
      if (base_ != nullptr && base_ != MAP_FAILED) {
         munmap(base_, size_);
         base_ = nullptr;
      }
      if (fd_ != -1) {
         close(fd_);
         fd_ = -1;
      }
   }

   TableEntry *findEntry(const std::string &name) {
      for (uint32_t i = 0; i < header_->entry_count; i++) {
         if (strcmp(entries_[i].name, name.c_str()) == 0) {
            return &entries_[i];
         }
      }
      return nullptr;
   }

   std::string detectStructureType(const TableEntry &entry) {
      if (entry.size == 16) {
         const char *data = static_cast<const char *>(base_) + entry.offset;
         const int32_t *values = reinterpret_cast<const int32_t *>(data);

         if (values[2] >= 0) {
            if (values[0] >= 0 && values[0] <= values[2]) {
               return "Barrier";
            }
            return "Semaphore";
         }
         return "Latch";
      }

      if (entry.size > sizeof(ArrayHeader)) {
         const char *data = static_cast<const char *>(base_) + entry.offset;
         const ArrayHeader *hdr = reinterpret_cast<const ArrayHeader *>(data);

         if (hdr->capacity > 0 && hdr->capacity < 1000000000) {
            size_t expected_size = sizeof(ArrayHeader) + hdr->capacity * 1;
            if (entry.size >= expected_size) {
               return "Array";
            }
         }
      }

      if (entry.size > sizeof(QueueHeader)) {
         const char *data = static_cast<const char *>(base_) + entry.offset;
         const QueueHeader *hdr = reinterpret_cast<const QueueHeader *>(data);

         if (hdr->capacity > 0 && hdr->capacity < 1000000000 &&
             hdr->head < hdr->capacity && hdr->tail < hdr->capacity) {
            return "Queue";
         }
      }

      if (entry.size > sizeof(StackHeader)) {
         return "Stack";
      }

      return "Unknown";
   }

   std::string formatSize(size_t bytes) {
      std::stringstream ss;
      if (bytes < 1024) {
         ss << bytes << " B";
      } else if (bytes < 1024 * 1024) {
         ss << std::fixed << std::setprecision(2) << (bytes / 1024.0) << " KB";
      } else if (bytes < 1024 * 1024 * 1024) {
         ss << std::fixed << std::setprecision(2) << (bytes / (1024.0 * 1024))
            << " MB";
      } else {
         ss << std::fixed << std::setprecision(2)
            << (bytes / (1024.0 * 1024 * 1024)) << " GB";
      }
      return ss.str();
   }
};

// REPL class for interactive mode
class ZeroSHMRepl {
 private:
   std::unique_ptr<zeroshm::Memory> memory_;
   std::string current_shm_;
   bool running_ = true;
   zeroshm::vfs::NavigationContext nav_context_;

 public:
   void run() {
      std::println(
          "ZeroSHM Interactive Shell v3.0 - Virtual Filesystem Interface");
      std::println("Type 'help' for available commands, 'quit' to exit\n");

      while (running_) {
         std::print("{}", nav_context_.prompt());

         std::string line;
         if (!std::getline(std::cin, line)) {
            break;   // EOF
         }

         if (line.empty())
            continue;

         std::vector<std::string> tokens = tokenize(line);
         if (tokens.empty())
            continue;

         processCommand(tokens);
      }
   }

 private:
   std::vector<std::string> tokenize(const std::string &line) {
      std::vector<std::string> tokens;
      std::istringstream iss(line);
      std::string token;
      while (iss >> token) {
         tokens.push_back(token);
      }
      return tokens;
   }

   void processCommand(const std::vector<std::string> &tokens) {
      const std::string &cmd = tokens[0];

      try {
         if (cmd == "help" || cmd == "?") {
            printHelp();
         } else if (cmd == "quit" || cmd == "exit") {
            running_ = false;
         } else if (cmd == "create") {
            cmdCreate(tokens);
         } else if (cmd == "open") {
            cmdOpen(tokens);
         } else if (cmd == "close") {
            cmdClose();
         } else if (cmd == "summary") {
            cmdSummary();
         } else if (cmd == "table") {
            cmdTable(tokens);
         } else if (cmd == "info") {
            cmdInfo(tokens);
         } else if (cmd == "dump") {
            cmdDump(tokens);
         } else if (cmd == "list") {
            cmdList();
         } else if (cmd == "create-array") {
            cmdCreateArray(tokens);
         } else if (cmd == "create-queue") {
            cmdCreateQueue(tokens);
         } else if (cmd == "create-stack") {
            cmdCreateStack(tokens);
         } else if (cmd == "create-semaphore") {
            cmdCreateSemaphore(tokens);
         } else if (cmd == "create-barrier") {
            cmdCreateBarrier(tokens);
         } else if (cmd == "create-latch") {
            cmdCreateLatch(tokens);
         } else if (cmd == "push") {
            cmdPush(tokens);
         } else if (cmd == "pop") {
            cmdPop(tokens);
         } else if (cmd == "enqueue") {
            cmdEnqueue(tokens);
         } else if (cmd == "dequeue") {
            cmdDequeue(tokens);
         } else if (cmd == "acquire") {
            cmdAcquire(tokens);
         } else if (cmd == "release") {
            cmdRelease(tokens);
         } else if (cmd == "wait") {
            cmdWait(tokens);
         } else if (cmd == "count-down") {
            cmdCountDown(tokens);
         } else if (cmd == "create-ring") {
            cmdCreateRing(tokens);
         } else if (cmd == "ring-write") {
            cmdRingWrite(tokens);
         } else if (cmd == "ring-read") {
            cmdRingRead(tokens);
         } else if (cmd == "create-map") {
            cmdCreateMap(tokens);
         } else if (cmd == "map-insert") {
            cmdMapInsert(tokens);
         } else if (cmd == "map-find") {
            cmdMapFind(tokens);
         } else if (cmd == "map-erase") {
            cmdMapErase(tokens);
         } else if (cmd == "create-set") {
            cmdCreateSet(tokens);
         } else if (cmd == "set-insert") {
            cmdSetInsert(tokens);
         } else if (cmd == "set-contains") {
            cmdSetContains(tokens);
         } else if (cmd == "set-erase") {
            cmdSetErase(tokens);
         } else if (cmd == "create-pool") {
            cmdCreatePool(tokens);
         } else if (cmd == "create-channel") {
            cmdCreateChannel(tokens);
         } else if (cmd == "channel-send") {
            cmdChannelSend(tokens);
         } else if (cmd == "channel-recv") {
            cmdChannelRecv(tokens);
         } else if (cmd == "channel-close") {
            cmdChannelClose(tokens);
         } else if (cmd == "ls") {
            cmdLs(tokens);
         } else if (cmd == "cd") {
            cmdCd(tokens);
         } else if (cmd == "pwd") {
            cmdPwd(tokens);
         } else {
            std::cerr << "Unknown command: " << cmd << "\n";
            std::cerr << "Type 'help' for available commands\n";
         }
      } catch (const std::exception &e) {
         std::cerr << "Error: " << e.what() << "\n";
      }
   }

   void printHelp() {
      std::println("\n=== ZeroSHM Commands ===\n");

      std::println("Navigation (Virtual Filesystem):");
      std::println("  ls [path]                            List contents at "
                   "current location or path");
      std::println("  cd <path>                            Change directory");
      std::println(
          "  pwd                                  Print working directory\n");

      std::println("Memory Management:");
      std::println(
          "  create <name> <size_mb>              Create new shared memory");
      std::println(
          "  open <name>                          Open existing shared memory");
      std::println(
          "  close                                Close current shared memory");
      std::println("  list                                 List all shared "
                   "memory objects\n");

      std::println("Inspection:");
      std::println(
          "  summary                              Show memory summary");
      std::println("  table [verbose]                      Show table entries");
      std::println(
          "  info <name>                          Show structure info");
      std::println(
          "  dump <name> [bytes]                  Hex dump of structure\n");

      std::println("Structure Creation:");
      std::println(
          "  create-array <name> <capacity> <elem_size>      Create array");
      std::println(
          "  create-queue <name> <capacity> <elem_size>      Create queue");
      std::println(
          "  create-stack <name> <capacity> <elem_size>      Create stack");
      std::println("  create-ring <name> <capacity> <elem_size>       Create "
                   "ring buffer");
      std::println(
          "  create-map <name> <capacity> <k_sz> <v_sz>      Create map");
      std::println(
          "  create-set <name> <capacity> <elem_size>        Create set");
      std::println(
          "  create-pool <name> <capacity> <elem_size>       Create pool");
      std::println(
          "  create-channel <name> <capacity> <elem_size>    Create channel");
      std::println(
          "  create-semaphore <name> <count> [max]           Create semaphore");
      std::println(
          "  create-barrier <name> <participants>            Create barrier");
      std::println(
          "  create-latch <name> <count>                     Create latch\n");

      std::println("Structure Manipulation (int32 only):");
      std::println("  push <stack_name> <value>            Push to stack");
      std::println("  pop <stack_name>                     Pop from stack");
      std::println("  enqueue <queue_name> <value>         Enqueue to queue");
      std::println("  dequeue <queue_name>                 Dequeue from queue");
      std::println(
          "  ring-write <ring_name> <value>       Write to ring buffer");
      std::println(
          "  ring-read <ring_name>                Read from ring buffer");
      std::println("  map-insert <map_name> <key> <value>  Insert into map");
      std::println("  map-find <map_name> <key>            Find in map");
      std::println("  map-erase <map_name> <key>           Erase from map");
      std::println("  set-insert <set_name> <value>        Insert into set");
      std::println(
          "  set-contains <set_name> <value>      Check if set contains");
      std::println("  set-erase <set_name> <value>         Erase from set");
      std::println("  channel-send <ch_name> <value>       Send to channel");
      std::println(
          "  channel-recv <ch_name>               Receive from channel");
      std::println("  channel-close <ch_name>              Close channel");
      std::println("  acquire <semaphore_name>             Acquire semaphore");
      std::println("  release <semaphore_name>             Release semaphore");
      std::println(
          "  wait <barrier/latch_name>            Wait at barrier/latch");
      std::println("  count-down <latch_name> [n]          Count down latch\n");

      std::println("General:");
      std::println("  help, ?                              Show this help");
      std::println("  quit, exit                           Exit REPL\n");
   }

   void cmdCreate(const std::vector<std::string> &tokens) {
      if (tokens.size() < 3) {
         std::cerr << "Usage: create <name> <size_mb>\n";
         return;
      }

      std::string name = tokens[1];
      if (name[0] != '/')
         name = "/" + name;

      size_t size_mb = std::stoull(tokens[2]);
      size_t size = size_mb * 1024 * 1024;

      memory_ = std::make_unique<zeroshm::Memory>(name, size,
                                                  256);   // 256 table entries
      current_shm_ = name;

      std::println("Created shared memory '{}' ({} MB)", name, size_mb);
   }

   void cmdOpen(const std::vector<std::string> &tokens) {
      if (tokens.size() < 2) {
         std::cerr << "Usage: open <name>\n";
         return;
      }

      std::string name = tokens[1];
      if (name[0] != '/')
         name = "/" + name;

      memory_ = std::make_unique<zeroshm::Memory>(name);
      current_shm_ = name;

      std::println("Opened shared memory '{}'", name);
   }

   void cmdClose() {
      if (!memory_) {
         std::cerr << "No shared memory currently open\n";
         return;
      }

      memory_.reset();
      current_shm_.clear();

      std::println("Closed shared memory");
   }

   void cmdSummary() {
      if (!memory_) {
         std::cerr
             << "No shared memory currently open. Use 'open <name>' first.\n";
         return;
      }

      SharedMemoryInspector inspector(current_shm_);
      if (inspector.open()) {
         inspector.printSummary();
      }
   }

   void cmdTable(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr
             << "No shared memory currently open. Use 'open <name>' first.\n";
         return;
      }

      bool verbose = (tokens.size() > 1 && tokens[1] == "verbose");

      SharedMemoryInspector inspector(current_shm_);
      if (inspector.open()) {
         inspector.printTable(verbose);
      }
   }

   void cmdInfo(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr
             << "No shared memory currently open. Use 'open <name>' first.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: info <name>\n";
         return;
      }

      SharedMemoryInspector inspector(current_shm_);
      if (inspector.open()) {
         inspector.printStructureInfo(tokens[1]);
      }
   }

   void cmdDump(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr
             << "No shared memory currently open. Use 'open <name>' first.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: dump <name> [bytes]\n";
         return;
      }

      size_t bytes = 256;
      if (tokens.size() > 2) {
         bytes = std::stoull(tokens[2]);
      }

      SharedMemoryInspector inspector(current_shm_);
      if (inspector.open()) {
         inspector.printHexDump(tokens[1], bytes);
      }
   }

   void cmdList() {
      SharedMemoryInspector inspector("");
      inspector.listSharedMemory();
   }

   void cmdCreateArray(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-array <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);
      size_t elem_size = std::stoull(tokens[3]);

      if (elem_size == 4) {
         try {
            auto arr = zeroshm::Array<int32_t>(*memory_, name, capacity);
            std::println("Created array<int32> '{}' with {} elements", name,
                         capacity);
         } catch (const std::exception &e) {
            std::cerr << "Error creating array: " << e.what() << "\n";
         }
      } else if (elem_size == 8) {
         try {
            auto arr = zeroshm::Array<int64_t>(*memory_, name, capacity);
            std::println("Created array<int64> '{}' with {} elements", name,
                         capacity);
         } catch (const std::exception &e) {
            std::cerr << "Error creating array: " << e.what() << "\n";
         }
      } else {
         std::cerr << "Unsupported element size. Use 4 or 8 bytes.\n";
      }
   }

   void cmdCreateQueue(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-queue <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);
      size_t elem_size = std::stoull(tokens[3]);

      if (elem_size == 4) {
         auto result =
             zeroshm::Queue<int32_t>::create(*memory_, name, capacity);
         if (result) {
            std::println("Created queue<int32> '{}' with capacity {}", name,
                         capacity);
         } else {
            std::cerr << "Error creating queue: "
                      << error_message(result.error()) << "\n";
         }
      } else if (elem_size == 8) {
         auto result =
             zeroshm::Queue<int64_t>::create(*memory_, name, capacity);
         if (result) {
            std::println("Created queue<int64> '{}' with capacity {}", name,
                         capacity);
         } else {
            std::cerr << "Error creating queue: "
                      << error_message(result.error()) << "\n";
         }
      } else {
         std::cerr << "Unsupported element size. Use 4 or 8 bytes.\n";
      }
   }

   void cmdCreateStack(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-stack <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);
      size_t elem_size = std::stoull(tokens[3]);

      if (elem_size == 4) {
         auto result =
             zeroshm::Stack<int32_t>::create(*memory_, name, capacity);
         if (result) {
            std::println("Created stack<int32> '{}' with capacity {}", name,
                         capacity);
         } else {
            std::cerr << "Error creating stack: "
                      << error_message(result.error()) << "\n";
         }
      } else if (elem_size == 8) {
         auto result =
             zeroshm::Stack<int64_t>::create(*memory_, name, capacity);
         if (result) {
            std::println("Created stack<int64> '{}' with capacity {}", name,
                         capacity);
         } else {
            std::cerr << "Error creating stack: "
                      << error_message(result.error()) << "\n";
         }
      } else {
         std::cerr << "Unsupported element size. Use 4 or 8 bytes.\n";
      }
   }

   void cmdCreateSemaphore(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: create-semaphore <name> <count> [max_count]\n";
         return;
      }

      std::string name = tokens[1];
      int32_t count = std::stoi(tokens[2]);
      int32_t max_count = 0;

      if (tokens.size() > 3) {
         max_count = std::stoi(tokens[3]);
      }

      auto result =
          zeroshm::Semaphore::create(*memory_, name, count, max_count);
      if (result) {
         if (max_count > 0) {
            std::println("Created semaphore '{}' with count {} (max: {})", name,
                         count, max_count);
         } else {
            std::println("Created semaphore '{}' with count {}", name, count);
         }
      } else {
         std::cerr << "Error creating semaphore: "
                   << error_message(result.error()) << "\n";
      }
   }

   void cmdCreateBarrier(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: create-barrier <name> <participants>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t participants = std::stoi(tokens[2]);

      auto result = zeroshm::Barrier::create(*memory_, name, participants);
      if (result) {
         std::println("Created barrier '{}' with {} participants", name,
                      participants);
      } else {
         std::cerr << "Error creating barrier: "
                   << error_message(result.error()) << "\n";
      }
   }

   void cmdCreateLatch(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: create-latch <name> <count>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t count = std::stoi(tokens[2]);

      auto result = zeroshm::Latch::create(*memory_, name, count);
      if (result) {
         std::println("Created latch '{}' with count {}", name, count);
      } else {
         std::cerr << "Error creating latch: " << error_message(result.error())
                   << "\n";
      }
   }

   void cmdPush(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: push <stack_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Stack<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->push(value)) {
         std::println("Pushed {} to stack '{}'", value, name);
      } else {
         std::println("Stack '{}' is full", name);
      }
   }

   void cmdPop(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: pop <stack_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Stack<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      auto val = result->pop();
      if (val) {
         std::println("Popped: {}", *val);
      } else {
         std::println("Stack '{}' is empty", name);
      }
   }

   void cmdEnqueue(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: enqueue <queue_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Queue<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->push(value)) {
         std::println("Enqueued {} to queue '{}'", value, name);
      } else {
         std::println("Queue '{}' is full", name);
      }
   }

   void cmdDequeue(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: dequeue <queue_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Queue<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      auto val = result->pop();
      if (val) {
         std::println("Dequeued: {}", *val);
      } else {
         std::println("Queue '{}' is empty", name);
      }
   }

   void cmdAcquire(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: acquire <semaphore_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Semaphore::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      result->acquire();
      std::println("Acquired semaphore '{}'", name);
   }

   void cmdRelease(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: release <semaphore_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Semaphore::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      result->release();
      std::println("Released semaphore '{}'", name);
   }

   void cmdWait(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: wait <barrier/latch_name>\n";
         return;
      }

      std::string name = tokens[1];

      // Try barrier first
      auto bar_result = zeroshm::Barrier::open(*memory_, name);
      if (bar_result) {
         bar_result->wait();
         std::println("Passed barrier '{}'", name);
         return;
      }

      // Try latch
      auto lat_result = zeroshm::Latch::open(*memory_, name);
      if (lat_result) {
         lat_result->wait();
         std::println("Latch '{}' released", name);
         return;
      }

      std::cerr << "Error: " << error_message(bar_result.error()) << "\n";
   }

   void cmdCountDown(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: count-down <latch_name> [n]\n";
         return;
      }

      std::string name = tokens[1];
      int32_t n = 1;

      if (tokens.size() > 2) {
         n = std::stoi(tokens[2]);
      }

      auto result = zeroshm::Latch::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      result->count_down(n);
      std::println("Counted down latch '{}' by {}", name, n);
   }

   // Ring buffer commands
   void cmdCreateRing(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-ring <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);
      size_t elem_size = std::stoull(tokens[3]);

      if (elem_size == 4) {
         auto result = zeroshm::Ring<int32_t>::create(*memory_, name, capacity);
         if (result) {
            std::println("Created ring<int32> '{}' with capacity {} bytes",
                         name, capacity);
         } else {
            std::cerr << "Error creating ring: "
                      << error_message(result.error()) << "\n";
         }
      } else {
         std::cerr << "Unsupported element size. Use 4 bytes.\n";
      }
   }

   void cmdRingWrite(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: ring-write <ring_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Ring<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->write(value)) {
         std::println("Wrote {} to ring '{}'", value, name);
      } else {
         std::println("Ring '{}' is full", name);
      }
   }

   void cmdRingRead(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: ring-read <ring_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Ring<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      auto val = result->read();
      if (val) {
         std::println("Read: {}", *val);
      } else {
         std::println("Ring '{}' is empty", name);
      }
   }

   // Map commands
   void cmdCreateMap(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 5) {
         std::cerr
             << "Usage: create-map <name> <capacity> <key_size> <value_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);

      auto result =
          zeroshm::Map<int32_t, int32_t>::create(*memory_, name, capacity);
      if (result) {
         std::println("Created map<int32,int32> '{}' with capacity {}", name,
                      capacity);
      } else {
         std::cerr << "Error creating map: " << error_message(result.error())
                   << "\n";
      }
   }

   void cmdMapInsert(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: map-insert <map_name> <key> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t key = std::stoi(tokens[2]);
      int32_t value = std::stoi(tokens[3]);

      auto result = zeroshm::Map<int32_t, int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->insert(key, value)) {
         std::println("Inserted [{} => {}] into map '{}'", key, value, name);
      } else {
         std::println("Map '{}' is full", name);
      }
   }

   void cmdMapFind(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: map-find <map_name> <key>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t key = std::stoi(tokens[2]);

      auto result = zeroshm::Map<int32_t, int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      auto val = result->find(key);
      if (val) {
         std::println("Found: [{} => {}]", key, *val);
      } else {
         std::println("Key {} not found in map '{}'", key, name);
      }
   }

   void cmdMapErase(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: map-erase <map_name> <key>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t key = std::stoi(tokens[2]);

      auto result = zeroshm::Map<int32_t, int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->erase(key)) {
         std::println("Erased key {} from map '{}'", key, name);
      } else {
         std::println("Key {} not found in map '{}'", key, name);
      }
   }

   // Set commands
   void cmdCreateSet(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-set <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);

      auto result = zeroshm::Set<int32_t>::create(*memory_, name, capacity);
      if (result) {
         std::println("Created set<int32> '{}' with capacity {}", name,
                      capacity);
      } else {
         std::cerr << "Error creating set: " << error_message(result.error())
                   << "\n";
      }
   }

   void cmdSetInsert(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: set-insert <set_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Set<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->insert(value)) {
         std::println("Inserted {} into set '{}'", value, name);
      } else {
         std::println("Value {} already exists or set is full", value);
      }
   }

   void cmdSetContains(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: set-contains <set_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Set<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->contains(value)) {
         std::println("Set '{}' contains {}", name, value);
      } else {
         std::println("Set '{}' does not contain {}", name, value);
      }
   }

   void cmdSetErase(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: set-erase <set_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Set<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->erase(value)) {
         std::println("Erased {} from set '{}'", value, name);
      } else {
         std::println("Value {} not found in set '{}'", value, name);
      }
   }

   // Pool commands
   void cmdCreatePool(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-pool <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);

      auto result = zeroshm::Pool<int32_t>::create(*memory_, name, capacity);
      if (result) {
         std::println("Created pool<int32> '{}' with capacity {}", name,
                      capacity);
      } else {
         std::cerr << "Error creating pool: " << error_message(result.error())
                   << "\n";
      }
   }

   // Channel commands
   void cmdCreateChannel(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open. Use 'create' or 'open' "
                      "first.\n";
         return;
      }

      if (tokens.size() < 4) {
         std::cerr << "Usage: create-channel <name> <capacity> <elem_size>\n";
         return;
      }

      std::string name = tokens[1];
      size_t capacity = std::stoull(tokens[2]);

      auto result = zeroshm::Channel<int32_t>::create(*memory_, name, capacity);
      if (result) {
         std::println("Created channel<int32> '{}' with capacity {}", name,
                      capacity);
      } else {
         std::cerr << "Error creating channel: "
                   << error_message(result.error()) << "\n";
      }
   }

   void cmdChannelSend(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 3) {
         std::cerr << "Usage: channel-send <channel_name> <value>\n";
         return;
      }

      std::string name = tokens[1];
      int32_t value = std::stoi(tokens[2]);

      auto result = zeroshm::Channel<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      if (result->send(value)) {
         std::println("Sent {} to channel '{}'", value, name);
      } else {
         std::println("Channel '{}' is full or closed", name);
      }
   }

   void cmdChannelRecv(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: channel-recv <channel_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Channel<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      auto val = result->recv();
      if (val) {
         std::println("Received: {}", *val);
      } else {
         std::println("Channel '{}' is empty or closed", name);
      }
   }

   void cmdChannelClose(const std::vector<std::string> &tokens) {
      if (!memory_) {
         std::cerr << "No shared memory currently open.\n";
         return;
      }

      if (tokens.size() < 2) {
         std::cerr << "Usage: channel-close <channel_name>\n";
         return;
      }

      std::string name = tokens[1];

      auto result = zeroshm::Channel<int32_t>::open(*memory_, name);
      if (!result) {
         std::cerr << "Error: " << error_message(result.error()) << "\n";
         return;
      }

      result->close();
      std::println("Closed channel '{}'", name);
   }

   // Virtual Filesystem Navigation Commands

   void cmdLs(const std::vector<std::string> &tokens) {
      using namespace zeroshm::vfs;

      // Parse optional path argument
      std::string target = tokens.size() > 1 ? tokens[1] : "";
      Path path = target.empty() ? nav_context_.current_path
                                 : nav_context_.current_path.resolve(target);

      if (path.isRoot()) {
         // List shared memory segments
         auto segments = listSharedMemorySegments();
         std::println("\n=== Shared Memory Segments ===");
         std::println("{:<30}{:<15}", "Name", "Size");
         std::println("{}", std::string(50, '-'));

         for (const auto &[name, size] : segments) {
            std::println("{:<30}{:<15}", name, formatSize(size));
         }
      } else if (path.depth() == 1) {
         // List structures in segment
         std::string segment_name = "/" + path[0];

         // Always use SharedMemoryInspector in read-only mode
         SharedMemoryInspector inspector(segment_name);
         if (inspector.open()) {
            inspector.printTable(true);   // verbose to show types
         }
      } else if (path.depth() == 2) {
         // Show structure contents
         std::string segment_name = "/" + path[0];
         std::string structure_name = path[1];

         SharedMemoryInspector inspector(segment_name);
         if (inspector.open()) {
            inspector.printStructureInfo(structure_name);
         }
      } else {
         std::cerr << "Invalid path depth\n";
      }
   }

   void cmdCd(const std::vector<std::string> &tokens) {
      if (tokens.size() < 2) {
         std::cerr << "Usage: cd <path>\n";
         return;
      }

      std::string target = tokens[1];

      // Save old state in case we need to revert
      auto old_path = nav_context_.current_path;
      auto old_location = nav_context_.location_type;

      // Try to change directory
      if (!nav_context_.cd(target)) {
         std::cerr << "cd: invalid path (max depth is 2)\n";
         return;
      }

      // If we changed segments, update the open memory
      if (nav_context_.location_type == zeroshm::vfs::LocationType::SEGMENT ||
          nav_context_.location_type == zeroshm::vfs::LocationType::STRUCTURE) {

         std::string segment = "/" + nav_context_.segment_name;

         // If segment changed, open it
         if (current_shm_ != segment) {
            try {
               memory_ = std::make_unique<zeroshm::Memory>(segment);
               current_shm_ = segment;
            } catch (const std::exception &e) {
               std::cerr << "Error opening segment '" << segment
                         << "': " << e.what() << "\n";
               // Revert navigation
               nav_context_.current_path = old_path;
               nav_context_.location_type = old_location;
               return;
            }
         }
      } else if (nav_context_.location_type ==
                 zeroshm::vfs::LocationType::ROOT) {
         // Going back to root, close current segment
         memory_.reset();
         current_shm_.clear();
      }
   }

   void cmdPwd(const std::vector<std::string> &tokens) {
      (void)tokens;
      std::println("{}", nav_context_.pwd());
   }
};

void printUsage(const char *program) {
   std::println("ZeroSHM Shared Memory Inspector v3.0");
   std::println("Enhanced with REPL mode and creation/manipulation support");
   std::println("\nUsage: {} [OPTIONS] [<shm_name>]", program);
   std::println("\nOptions:");
   std::println("  -r, --repl         Start interactive REPL mode");
   std::println("  -s, --summary      Show summary information (default)");
   std::println("  -t, --table        Show table entries");
   std::println("  -v, --verbose      Verbose output (show structure types)");
   std::println("  -d, --dump <name>  Hex dump of named entry");
   std::println(
       "  -i, --info <name>  Show structure information for named entry");
   std::println("  -l, --list         List all shared memory objects");
   std::println("  -a, --all          Show all information");
   std::println("  -h, --help         Show this help message");
   std::println("\nExamples:");
   std::println("  {} -r                         # Start REPL", program);
   std::println("  {} /my_shm                    # Show summary", program);
   std::println("  {} -tv /my_shm                # Show table with types",
                program);
   std::println("  {} -i my_semaphore /my_shm    # Info about semaphore",
                program);
   std::println("  {} -a /my_shm                 # Show everything", program);
   std::println("  {} -l                         # List all shared memory",
                program);
}

int main(int argc, char *argv[]) {
   static struct option long_options[] = {
       {"repl", no_argument, 0, 'r'},       {"summary", no_argument, 0, 's'},
       {"table", no_argument, 0, 't'},      {"verbose", no_argument, 0, 'v'},
       {"dump", required_argument, 0, 'd'}, {"info", required_argument, 0, 'i'},
       {"list", no_argument, 0, 'l'},       {"all", no_argument, 0, 'a'},
       {"help", no_argument, 0, 'h'},       {0, 0, 0, 0}};

   bool repl_mode = false;
   bool show_summary = false;
   bool show_table = false;
   bool verbose = false;
   bool show_all = false;
   bool list_only = false;
   std::string dump_entry;
   std::string info_entry;

   int opt;
   int option_index = 0;
   while ((opt = getopt_long(argc, argv, "rstvd:i:lah", long_options,
                             &option_index)) != -1) {
      switch (opt) {
      case 'r':
         repl_mode = true;
         break;
      case 's':
         show_summary = true;
         break;
      case 't':
         show_table = true;
         break;
      case 'v':
         verbose = true;
         break;
      case 'd':
         dump_entry = optarg;
         break;
      case 'i':
         info_entry = optarg;
         break;
      case 'l':
         list_only = true;
         break;
      case 'a':
         show_all = true;
         break;
      case 'h':
         printUsage(argv[0]);
         return 0;
      default:
         printUsage(argv[0]);
         return 1;
      }
   }

   // REPL mode
   if (repl_mode) {
      ZeroSHMRepl repl;
      repl.run();
      return 0;
   }

   // List mode
   if (list_only) {
      SharedMemoryInspector inspector("");
      inspector.listSharedMemory();
      return 0;
   }

   // Inspection mode
   if (optind >= argc) {
      std::cerr << "Error: Missing shared memory name\n";
      printUsage(argv[0]);
      return 1;
   }

   std::string shm_name = argv[optind];
   SharedMemoryInspector inspector(shm_name);

   if (!inspector.open()) {
      return 1;
   }

   if (!show_summary && !show_table && dump_entry.empty() &&
       info_entry.empty() && !show_all) {
      show_summary = true;
   }

   if (show_all) {
      show_summary = true;
      show_table = true;
      verbose = true;
   }

   if (show_summary)
      inspector.printSummary();
   if (show_table)
      inspector.printTable(verbose);
   if (!dump_entry.empty())
      inspector.printHexDump(dump_entry);
   if (!info_entry.empty())
      inspector.printStructureInfo(info_entry);

   return 0;
}
