#include <cassert>
#include <cstring>
#include <iostream>
#include <string>

#include <zeroshm/detail/xxhash.h>
#include <zeroshm/error.h>
#include <zeroshm/memory.h>
#include <zeroshm/table.h>

using namespace zeroshm;

static void write_entries(Memory &mem, const std::string &shm_name) {
   std::cout << "C++ writer: Creating shared memory '" << shm_name << "'\n";

   // Create entries with long names
   std::string name_50(50, 'a');
   std::string name_100(100, 'b');
   std::string name_200(200, 'c');
   std::string name_254(254, 'd');

   // Add entries using the table's allocate() which computes offsets
   auto *table = mem.table();
   uint64_t off_50 = table->allocate(100);
   if (!table->add(name_50, off_50, 100).has_value())
      throw std::runtime_error("Failed to add name_50");
   uint64_t off_100 = table->allocate(200);
   if (!table->add(name_100, off_100, 200).has_value())
      throw std::runtime_error("Failed to add name_100");
   uint64_t off_200 = table->allocate(300);
   if (!table->add(name_200, off_200, 300).has_value())
      throw std::runtime_error("Failed to add name_200");
   uint64_t off_254 = table->allocate(400);
   if (!table->add(name_254, off_254, 400).has_value())
      throw std::runtime_error("Failed to add name_254");
   uint64_t off_short = table->allocate(500);
   if (!table->add("short_name", off_short, 500).has_value())
      throw std::runtime_error("Failed to add short_name");

   // Write some data at the allocated offsets
   char *base = static_cast<char *>(mem.data());
   std::memcpy(base + off_50, "data50", 6);
   std::memcpy(base + off_100, "data100", 7);
   std::memcpy(base + off_200, "data200", 7);
   std::memcpy(base + off_254, "data254", 7);
   std::memcpy(base + off_short, "data_short", 10);

   std::cout << "C++ writer: Added 5 entries (including long names up to 254 "
                "chars)\n";
   std::cout << "C++ writer: Entry count: " << table->entry_count() << "\n";
}

static void read_entries(Memory &mem, const std::string &shm_name) {
   std::cout << "C++ reader: Opening shared memory '" << shm_name << "'\n";

   // Verify entries are found via hash-first lookup
   std::string name_50(50, 'a');
   std::string name_100(100, 'b');
   std::string name_200(200, 'c');
   std::string name_254(254, 'd');

   auto *table = mem.table();

   auto f1 = table->find(name_50);
   if (!f1.has_value())
      throw std::runtime_error("Failed to find name_50");
   auto *e1 = *f1;
   if (e1->size != 100)
      throw std::runtime_error("Wrong size for name_50");
   std::cout << "C++ reader: Found '" << name_50.substr(0, 10)
             << "...' (offset=" << e1->offset << ", size=" << e1->size << ")\n";

   auto f2 = table->find(name_100);
   if (!f2.has_value())
      throw std::runtime_error("Failed to find name_100");
   auto *e2 = *f2;
   if (e2->size != 200)
      throw std::runtime_error("Wrong size for name_100");
   std::cout << "C++ reader: Found '" << name_100.substr(0, 10)
             << "...' (offset=" << e2->offset << ", size=" << e2->size << ")\n";

   auto f3 = table->find(name_200);
   if (!f3.has_value())
      throw std::runtime_error("Failed to find name_200");
   auto *e3 = *f3;
   if (e3->size != 300)
      throw std::runtime_error("Wrong size for name_200");
   std::cout << "C++ reader: Found '" << name_200.substr(0, 10)
             << "...' (offset=" << e3->offset << ", size=" << e3->size << ")\n";

   auto f4 = table->find(name_254);
   if (!f4.has_value())
      throw std::runtime_error("Failed to find name_254");
   auto *e4 = *f4;
   if (e4->size != 400)
      throw std::runtime_error("Wrong size for name_254");
   std::cout << "C++ reader: Found '" << name_254.substr(0, 10)
             << "...' (offset=" << e4->offset << ", size=" << e4->size << ")\n";

   auto f5 = table->find("short_name");
   if (!f5.has_value())
      throw std::runtime_error("Failed to find short_name");
   auto *e5 = *f5;
   if (e5->size != 500)
      throw std::runtime_error("Wrong size for short_name");
   std::cout << "C++ reader: Found 'short_name' (offset=" << e5->offset
             << ", size=" << e5->size << ")\n";

   // Verify hash values match
   auto *header = reinterpret_cast<Table::Header *>(mem.data());
   auto *entries = reinterpret_cast<Table::Entry *>(
       static_cast<char *>(mem.data()) + sizeof(Table::Header));

   for (uint32_t i = 0; i < header->entry_count; i++) {
      std::string_view name(entries[i].name);
      uint64_t expected_hash =
          XXH3_64bits_withSeed(name.data(), name.size(), XXHASH_SEED);
      if (entries[i].hash != expected_hash)
         throw std::runtime_error("Hash mismatch for entry " +
                                  std::to_string(i));
   }
   std::cout << "C++ reader: All " << header->entry_count
             << " entries have correct hash values\n";

   std::cout << "C++ reader: Verified entry count: " << table->entry_count()
             << "\n";
}

int main(int argc, char **argv) {
   if (argc < 3) {
      std::cerr << "Usage: " << argv[0] << " <write|read> <shm_name>\n";
      return 1;
   }

   std::string mode = argv[1];
   std::string shm_name = argv[2];

   // Ensure name starts with /
   if (!shm_name.empty() && shm_name[0] != '/') {
      shm_name = "/" + shm_name;
   }

   if (mode == "write") {
      // Clean up any existing segment before creating new one
      Memory::unlink(shm_name);
      Memory mem(shm_name, 1024 * 1024 * 4, 64);
      write_entries(mem, shm_name);
   } else if (mode == "read") {
      Memory mem(shm_name);
      read_entries(mem, shm_name);
   } else {
      std::cerr << "Unknown mode: " << mode << "\n";
      return 1;
   }

   return 0;
}
