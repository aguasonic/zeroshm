#pragma once

#include <bit>
#include <cstdint>
#include <cstring>
#include <expected>
#include <iterator>
#include <stdexcept>
#include <string_view>

#include <zeroshm/constants_generated.h>
#include <zeroshm/detail/xxhash.h>
#include <zeroshm/error.h>
#include <zeroshm/layout.h>

namespace zeroshm {

constexpr size_t MIN_NAME_SIZE = 3;   // minimum name length in characters
constexpr size_t MAX_NAME_SIZE =
    255;   // max total length including null terminator

// Endianness helpers for cross-platform shared memory access (C++23 P2633).
// The ZeroIPC binary format is little-endian. On big-endian platforms,
// std::byteswap converts between native and storage representation.
// On little-endian platforms (the common case), these are no-ops selected
// at compile time via if constexpr.

inline uint32_t from_le(uint32_t v) {
   if constexpr (std::endian::native == std::endian::big) {
      return std::byteswap(v);
   }
   return v;
}

inline uint32_t to_le(uint32_t v) {
   if constexpr (std::endian::native == std::endian::big) {
      return std::byteswap(v);
   }
   return v;
}

inline uint64_t from_le(uint64_t v) {
   if constexpr (std::endian::native == std::endian::big) {
      return std::byteswap(v);
   }
   return v;
}

inline uint64_t to_le(uint64_t v) {
   if constexpr (std::endian::native == std::endian::big) {
      return std::byteswap(v);
   }
   return v;
}

/**
 * Round n up to the next multiple of a (a must be a power of two).
 *
 * All structures place their sections (header, data array, atomic side-array)
 * on 8-byte boundaries so that element and atomic accesses are naturally
 * aligned regardless of element size. Every language computes offsets with the
 * same rule, so the layout stays binary-compatible across implementations.
 */
constexpr size_t align_up(size_t n, size_t a) {
   return (n + a - 1) & ~(a - 1);
}

// The library guarantees 8-byte section alignment only; element types with a
// stronger requirement cannot be placed correctly (the allocator 8-aligns each
// structure base and stores no per-type alignment in the minimal metadata).
constexpr size_t MAX_ELEM_ALIGN = SECTION_ALIGN;

/**
 * Runtime-configurable table for managing named structures in shared memory.
 *
 * The table is stored at the beginning of shared memory and tracks
 * all allocated structures by name, offset, and size.
 */
class Table {
 public:
   struct Header {
      uint32_t magic;
      uint32_t version;
      uint32_t entry_count;
      uint32_t max_entries;   // Maximum number of table entries
      uint64_t memory_size;   // Total size of the shared memory segment
                              // (supports >4GB)
      uint64_t next_offset;   // Next allocation offset (supports >4GB)
   };

   /// Compile-time validation: Table::Header must be exactly 32 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Header) == EXPECTED_TABLE_HEADER_SIZE,
                 "Table::Header size mismatch -- binary format incompatibility");
   static_assert(offsetof(Header, magic) == TABLE_MAGIC_OFFSET,
                 "Table::Header.magic must be at TABLE_MAGIC_OFFSET");
   static_assert(offsetof(Header, version) == TABLE_VERSION_OFFSET,
                 "Table::Header.version must be at TABLE_VERSION_OFFSET");
   static_assert(offsetof(Header, entry_count) == TABLE_ENTRY_COUNT_OFFSET,
                 "Table::Header.entry_count must be at TABLE_ENTRY_COUNT_OFFSET");
   static_assert(offsetof(Header, max_entries) == TABLE_MAX_ENTRIES_OFFSET,
                 "Table::Header.max_entries must be at TABLE_MAX_ENTRIES_OFFSET");
   static_assert(offsetof(Header, memory_size) == TABLE_MEMORY_SIZE_OFFSET,
                 "Table::Header.memory_size must be at TABLE_MEMORY_SIZE_OFFSET");
   static_assert(offsetof(Header, next_offset) == TABLE_NEXT_OFFSET_OFFSET,
                 "Table::Header.next_offset must be at TABLE_NEXT_OFFSET_OFFSET");

   struct Entry {
      uint64_t hash;       // xxhash3-64 of name (seed = 0x2FE7)
      uint64_t offset;     // Supports offsets >4GB
      uint64_t size;       // Supports sizes >4GB
      char name[256];      // null-terminated name (max 254 chars + 1 null = 255
                           // bytes, 1 byte padding)
      char padding[104];   // Padding to reach 384 bytes (matches Rust/Go
                           // implementations)
   };

   /// Entry field offsets within the 384-byte entry slot.
   /// These constants define the binary layout shared across C++, Go, and Rust.
   static constexpr std::size_t EntryHashOffset   = 0;
   static constexpr std::size_t EntryOffsetOffset = 8;
   static constexpr std::size_t EntrySizeOffset   = 16;
   static constexpr std::size_t EntryNameOffset   = 24;
   static constexpr std::size_t EntryNameEnd      = 280; // EntryNameOffset + 256

   /// Compile-time validation: Table::Entry must be exactly 384 bytes and
   /// match the layout expected by Go/Rust implementations.
   static_assert(sizeof(Entry) == EXPECTED_TABLE_ENTRY_SIZE,
                 "Table::Entry size mismatch -- binary format incompatibility");
   static_assert(offsetof(Entry, hash) == EntryHashOffset,
                 "Table::Entry.hash must be at EntryHashOffset");
   static_assert(offsetof(Entry, offset) == EntryOffsetOffset,
                 "Table::Entry.offset must be at EntryOffsetOffset");
   static_assert(offsetof(Entry, size) == EntrySizeOffset,
                 "Table::Entry.size must be at EntrySizeOffset");
   static_assert(offsetof(Entry, name) == EntryNameOffset,
                 "Table::Entry.name must be at EntryNameOffset");
   static_assert(offsetof(Entry, padding) == EntryNameEnd,
                 "Table::Entry.padding must be at EntryNameEnd");

 private:
   void initialize() {
      auto *header = get_header();
      // Write header fields in little-endian format (C++23 std::byteswap
      // handles conversion on big-endian platforms; no-op on little-endian)
      header->magic = to_le(TABLE_MAGIC);
      header->version = to_le(TABLE_VERSION);
      header->entry_count = to_le(0u);
      header->max_entries = to_le(static_cast<uint32_t>(max_entries_));
      header->memory_size = to_le(static_cast<uint64_t>(memory_size_));
      header->next_offset = to_le(static_cast<uint64_t>(calculate_size(
          max_entries_)));   // Already aligned due to struct sizes

      // Zero out entries
      auto *entries = get_entries();
      std::memset(entries, 0, max_entries_ * sizeof(Entry));
   }

   void validate() {
      auto *header = get_header();

      // Read header fields from little-endian storage (C++23 std::byteswap
      // converts to native representation on big-endian platforms)
      if (from_le(header->magic) != TABLE_MAGIC) {
         throw std::runtime_error("Invalid table magic number");
      }

      if (from_le(header->version) != TABLE_VERSION) {
         throw std::runtime_error("Incompatible table version");
      }

      if (from_le(header->entry_count) > max_entries_) {
         throw std::runtime_error(
             "Table corruption: entry count exceeds maximum");
      }

      // Use the stored memory size when opening existing table
      memory_size_ = from_le(header->memory_size);
   }

   Header *get_header() {
      return reinterpret_cast<Header *>(memory_);
   }

   const Header *get_header() const {
      return reinterpret_cast<const Header *>(memory_);
   }

   Entry *get_entries() {
      return reinterpret_cast<Entry *>(memory_ + sizeof(Header));
   }

   const Entry *get_entries() const {
      return reinterpret_cast<const Entry *>(memory_ + sizeof(Header));
   }

   char *memory_;
   size_t max_entries_;
   size_t memory_size_;

 public:
   /**
    * Initialize a table in existing memory
    * @param memory Pointer to the start of shared memory
    * @param max_entries Maximum number of entries this table can hold
    * @param memory_size Total size of the shared memory segment
    * @param create If true, initialize a new table; if false, open existing
    */
   Table(void *memory, size_t max_entries, size_t memory_size,
         bool create = false)
       : memory_(static_cast<char *>(memory)), max_entries_(max_entries),
         memory_size_(memory_size) {

      if (create) {
         initialize();
      } else {
         validate();
      }
   }

   /**
    * Find an entry by name (hash-first comparison with string fallback)
    * @return std::expected containing pointer to entry, or Error::NotFound
    */
   std::expected<const Entry *, Error> find(std::string_view name) const {
      if (name.size() < MIN_NAME_SIZE) {
         return std::unexpected(Error::NameTooShort);
      }
      if (name.size() >= MAX_NAME_SIZE) {
         return std::unexpected(Error::NameTooLong);
      }
      auto *header = get_header();
      auto *entries = get_entries();

      // Compute hash for fast comparison
      uint64_t hash =
          XXH3_64bits_withSeed(name.data(), name.size(), XXHASH_SEED);

      uint32_t count = from_le(header->entry_count);
      for (uint32_t i = 0; i < count; ++i) {
         if (from_le(entries[i].hash) == hash && name == entries[i].name) {
            return &entries[i];
         }
      }
      return std::unexpected(Error::NotFound);
   }

   /**
    * Add a new entry to the table
    * @return std::expected<void, Error> -- Error::TableFull if table is full,
    *         Error::NameTooLong if name exceeds max length,
    *         Error::NameAlreadyExists if name is already in the table
    */
   [[nodiscard]] std::expected<void, Error>
   add(std::string_view name, uint64_t offset, uint64_t size) {
      if (name.size() < MIN_NAME_SIZE) {
         return std::unexpected(Error::NameTooShort);
      }
      if (name.size() >= MAX_NAME_SIZE) {
         return std::unexpected(Error::NameTooLong);
      }

      auto *header = get_header();
      if (from_le(header->entry_count) >= max_entries_) {
         return std::unexpected(Error::TableFull);
      }

      if (find(name).has_value()) {
         return std::unexpected(Error::NameAlreadyExists);
      }

      auto *entries = get_entries();

      // Transactional add: write entry data first, then increment entry_count.
      // This ensures the entry only becomes visible (counted in entry_count)
      // after all its fields are written. If any write fails before the
      // increment, the entry_count remains unchanged and the slot stays
      // invisible to find() and table readers.
      size_t index = from_le(header->entry_count);
      auto &entry = entries[index];

      // Compute and store hash in little-endian format
      entry.hash =
          to_le(XXH3_64bits_withSeed(name.data(), name.size(), XXHASH_SEED));

      std::memset(entry.name, 0, sizeof(entry.name));
      std::memcpy(entry.name, name.data(), name.size());
      entry.offset = to_le(offset);
      entry.size = to_le(size);

      // Commit: increment entry_count to make the entry visible
      header->entry_count = to_le(from_le(header->entry_count) + 1);

      return {};
   }

   /**
    * Remove an entry from the table by name.
    * Uses swap-and-pop: moves the last entry into the removed slot,
    * then decrements entry_count. O(1) after the find.
    * @return std::expected<void, Error> -- Error::NotFound if the name doesn't
    * exist
    */
   [[nodiscard]] std::expected<void, Error> remove(std::string_view name) {
      auto *header = get_header();
      uint32_t count = from_le(header->entry_count);
      if (count == 0) {
         return std::unexpected(Error::NotFound);
      }

      auto *entries = get_entries();

      // Compute hash for fast comparison
      uint64_t hash =
          XXH3_64bits_withSeed(name.data(), name.size(), XXHASH_SEED);

      // Find the entry
      uint32_t index = 0;
      for (; index < count; ++index) {
         if (from_le(entries[index].hash) == hash &&
             name == entries[index].name) {
            break;
         }
      }

      if (index == count) {
         return std::unexpected(Error::NotFound);
      }

      // Swap-and-pop: move the last entry into the removed slot
      if (index != count - 1) {
         entries[index] = entries[count - 1];
      }

      // Clear the last slot and decrement count
      std::memset(&entries[count - 1], 0, sizeof(Entry));
      header->entry_count = to_le(count - 1);

      return {};
   }

   /**
    * Allocate space for a new structure
    * @param size Size in bytes to allocate
    * @param alignment Alignment requirement (default 8)
    * @return Offset of allocated space
    */
   uint64_t allocate(size_t size, size_t alignment = SECTION_ALIGN) {
      auto *header = get_header();

      // Read next_offset from little-endian storage
      uint64_t next = from_le(header->next_offset);

      // Align the offset
      uint64_t aligned = (next + alignment - 1) & ~(alignment - 1);
      uint64_t result = aligned;

      // Check if allocation would exceed memory bounds
      if (aligned + size < aligned) {   // Check for overflow
         throw std::runtime_error("Allocation size overflow");
      }

      if (aligned + size > memory_size_) {   // Check against total memory size
         throw std::runtime_error("Allocation would exceed memory bounds");
      }

      // Write updated next_offset in little-endian format
      header->next_offset = to_le(aligned + size);
      return result;
   }

   /**
    * Get the total size of the table in bytes.
    * constexpr: computed at compile time when max_entries is a constant,
    * otherwise evaluated at runtime.
    */
   static constexpr size_t calculate_size(size_t max_entries) {
      return sizeof(Header) + max_entries * sizeof(Entry);
   }

   /**
    * Get the number of entries currently in the table
    */
   size_t entry_count() const {
      return from_le(get_header()->entry_count);
   }

   /**
    * Get the maximum number of entries this table can hold
    */
   size_t max_entries() const {
      return max_entries_;
   }

   /**
    * Get the next allocation offset
    */
   uint64_t next_offset() const {
      return from_le(get_header()->next_offset);
   }

   /**
    * Lazy range view over table entries (C++23-style lazy iteration).
    *
    * std::generator is not yet available in all C++23 standard libraries,
    * so this provides equivalent lazy iteration using a custom range view.
    * Enables range-based for loops without materializing a collection:
    *
    *   for (const auto& entry : table.entries()) {
    *       std::println("{}", entry.name);
    *   }
    *
    * @return Range view that iterates over each active entry
    */
   class EntriesView {
    private:
      const Entry *entries_;
      uint32_t count_;

    public:
      class iterator {
       private:
         const Entry *ptr_;

       public:
         using value_type = Entry;
         using difference_type = std::ptrdiff_t;
         using reference = const Entry &;
         using pointer = const Entry *;
         using iterator_category = std::input_iterator_tag;

         iterator(const Entry *ptr) : ptr_(ptr) {
         }

         reference operator*() const {
            return *ptr_;
         }
         pointer operator->() const {
            return ptr_;
         }
         iterator &operator++() {
            ++ptr_;
            return *this;
         }
         iterator operator++(int) {
            auto tmp = *this;
            ++ptr_;
            return tmp;
         }
         bool operator==(const iterator &other) const {
            return ptr_ == other.ptr_;
         }
         bool operator!=(const iterator &other) const {
            return ptr_ != other.ptr_;
         }
      };

      EntriesView(const Entry *entries, uint32_t count)
          : entries_(entries), count_(count) {
      }

      iterator begin() const {
         return iterator(entries_);
      }
      iterator end() const {
         return iterator(entries_ + count_);
      }
   };

   [[nodiscard]] EntriesView entries() const {
      return EntriesView(get_entries(), from_le(get_header()->entry_count));
   }
};

}   // namespace zeroshm
