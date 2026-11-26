//! zeroshm - CLI tool for inspecting and managing ZeroSHM shared memory
//!
//! Usage:
//!
//! ```text
//! zeroshm <command> [arguments]
//!
//! Commands:
//!   list                     List shared memory segments
//!   info <shm>               Show table info for a segment
//!   entries <shm>            List all entries in a segment's table
//!   create <shm> <size>      Create a new shared memory segment
//!   delete <shm>             Delete a shared memory segment
//!   dump <shm>               Hexdump the first N bytes of a segment
//!
//! Structure inspection:
//!   array <shm> <name>       Inspect an array
//!   queue <shm> <name>       Inspect a queue
//!   stack <shm> <name>       Inspect a stack
//!   semaphore <shm> <name>   Inspect a semaphore
//!   mutex <shm> <name>       Inspect a mutex
//!   barrier <shm> <name>     Inspect a barrier
//!   latch <shm> <name>       Inspect a latch
//!   once <shm> <name>        Inspect a once flag
//! ```

use clap::{Parser, Subcommand};
use std::process;
use zeroshm::layout::{
    TABLE_MAGIC,
    QUEUE_HEAD_OFFSET, QUEUE_TAIL_OFFSET, QUEUE_CAPACITY_OFFSET, QUEUE_ELEM_SIZE_OFFSET,
    STACK_TOP_OFFSET, STACK_CAPACITY_OFFSET, STACK_ELEM_SIZE_OFFSET,
    SEMAPHORE_COUNT_OFFSET, SEMAPHORE_WAITING_OFFSET, SEMAPHORE_MAX_COUNT_OFFSET,
    BARRIER_ARRIVED_OFFSET, BARRIER_GENERATION_OFFSET, BARRIER_NUM_PARTICIPANTS_OFFSET,
    LATCH_COUNT_OFFSET, LATCH_INITIAL_COUNT_OFFSET,
    ONCE_STATE_OFFSET,
};
use zeroshm::Memory;

const VERSION: &str = "1.0.0";

#[derive(Parser)]
#[command(name = "zeroshm", version = VERSION, about = "CLI tool for ZeroSHM shared memory")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List shared memory segments
    List,
    /// Show table header for a segment
    Info {
        /// Shared memory name (e.g., /mydata or mydata)
        shm: String,
    },
    /// List all entries in a segment's table
    Entries {
        /// Shared memory name
        shm: String,
    },
    /// Create a new shared memory segment
    Create {
        /// Shared memory name
        shm: String,
        /// Size in bytes (supports K/M/G suffixes)
        size: String,
    },
    /// Delete a shared memory segment
    Delete {
        /// Shared memory name
        shm: String,
    },
    /// Hexdump the first N bytes of a segment
    Dump {
        /// Shared memory name
        shm: String,
        /// Number of bytes to dump (default 256)
        #[arg(default_value_t = 256)]
        bytes: usize,
    },
    /// Inspect an array
    Array {
        /// Shared memory name
        shm: String,
        /// Array name
        name: String,
    },
    /// Inspect a queue
    Queue {
        /// Shared memory name
        shm: String,
        /// Queue name
        name: String,
    },
    /// Inspect a stack
    Stack {
        /// Shared memory name
        shm: String,
        /// Stack name
        name: String,
    },
    /// Inspect a semaphore
    Semaphore {
        /// Shared memory name
        shm: String,
        /// Semaphore name
        name: String,
    },
    /// Inspect a mutex (binary semaphore)
    Mutex {
        /// Shared memory name
        shm: String,
        /// Mutex name
        name: String,
    },
    /// Inspect a barrier
    Barrier {
        /// Shared memory name
        shm: String,
        /// Barrier name
        name: String,
    },
    /// Inspect a latch
    Latch {
        /// Shared memory name
        shm: String,
        /// Latch name
        name: String,
    },
    /// Inspect a once flag
    Once {
        /// Shared memory name
        shm: String,
        /// Once flag name
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::List => cmd_list(),
        Commands::Info { shm } => cmd_info(&shm),
        Commands::Entries { shm } => cmd_entries(&shm),
        Commands::Create { shm, size } => cmd_create(&shm, &size),
        Commands::Delete { shm } => cmd_delete(&shm),
        Commands::Dump { shm, bytes } => cmd_dump(&shm, bytes),
        Commands::Array { shm, name } => cmd_array(&shm, &name),
        Commands::Queue { shm, name } => cmd_queue(&shm, &name),
        Commands::Stack { shm, name } => cmd_stack(&shm, &name),
        Commands::Semaphore { shm, name } => cmd_semaphore(&shm, &name),
        Commands::Mutex { shm, name } => cmd_mutex(&shm, &name),
        Commands::Barrier { shm, name } => cmd_barrier(&shm, &name),
        Commands::Latch { shm, name } => cmd_latch(&shm, &name),
        Commands::Once { shm, name } => cmd_once(&shm, &name),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

/// Normalize the shm name to ensure it starts with /
fn normalize_name(name: &str) -> String {
    if name.starts_with('/') {
        name.to_string()
    } else {
        format!("/{}", name)
    }
}

/// Format a byte size as human-readable
fn format_size(size: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;

    match size {
        s if s >= GB => format!("{:.1}G", s as f64 / GB as f64),
        s if s >= MB => format!("{:.1}M", s as f64 / MB as f64),
        s if s >= KB => format!("{:.1}K", s as f64 / KB as f64),
        s => format!("{}B", s),
    }
}

/// Parse a size string like "1M", "1024K", "1048576"
fn parse_size(s: &str) -> Result<usize, String> {
    let s = s.trim().to_uppercase();

    let (num_str, multiplier) = if s.ends_with('K') {
        (&s[..s.len() - 1], 1024)
    } else if s.ends_with('M') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('G') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else {
        (s.as_str(), 1)
    };

    let n: usize = num_str.parse().map_err(|_| format!("invalid size: {}", s))?;
    Ok(n * multiplier)
}

/// List shared memory segments
fn cmd_list() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        cmd_list_linux()
    }
    #[cfg(target_os = "macos")]
    {
        cmd_list_macos()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok(println!("Shared memory listing not supported on this platform."))
    }
}

/// List shared memory segments on Linux (read /dev/shm directory)
#[cfg(target_os = "linux")]
fn cmd_list_linux() -> Result<(), String> {
    use std::fs;

    let entries = fs::read_dir("/dev/shm")
        .map_err(|e| format!("failed to read /dev/shm: {}", e))?;

    println!("{:<50} {:>10} {:>8}", "NAME", "SIZE", "ZEROSHM");
    println!("{:<50} {:>10} {:>8}", "-".repeat(50), "-".repeat(10), "-".repeat(8));

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let full_name = format!("/{}", name);
        let metadata = entry.metadata().map_err(|e| format!("failed to stat {}: {}", name, e))?;
        let size = metadata.len();

        let is_zeroshm = check_zeroshm(&full_name);
        let zeroshm_str = if is_zeroshm { "yes" } else { "" };

        println!("{:<50} {:>10} {:>8}", name, format_size(size as usize), zeroshm_str);
    }

    Ok(())
}

/// List shared memory segments on macOS
#[cfg(target_os = "macos")]
fn cmd_list_macos() -> Result<(), String> {
    // macOS does not expose /dev/shm; shared memory is managed via kernel objects.
    // There is no reliable way to enumerate POSIX shared memory segment names.
    eprintln!("Warning: macOS does not support listing shared memory segments.");
    eprintln!();
    eprintln!("On macOS, shared memory segments are managed by the kernel and");
    eprintln!("cannot be listed like files. Use 'zeroshm info <name>' to inspect");
    eprintln!("a specific segment if you know its name.");
    eprintln!();
    eprintln!("Alternatively, check sysctl output:");
    eprintln!("  sysctl -n kern.posix.shm");

    Ok(())
}

/// Check if a shared memory segment has the ZeroSHM magic number
#[cfg(target_os = "linux")]
fn check_zeroshm(name: &str) -> bool {
    let name = normalize_name(name);
    let mem = match Memory::open(&name, 64) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let header = match mem.table().header() {
        Ok(h) => h,
        Err(_) => return false,
    };
    header.magic == layout::TABLE_MAGIC
}

/// Show table header info for a segment
fn cmd_info(shm: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let header = mem.table().header()
        .map_err(|e| format!("failed to read header: {}", e))?;

    println!("Shared Memory: {}", name);
    println!("Size:          {} ({} bytes)", format_size(mem.size()), mem.size());
    println!();
    println!("Table Header:");
    print!("  Magic:       0x{:08X}", header.magic);
    if header.magic == TABLE_MAGIC {
        print!(" (ZSHM )");
    } else {
        print!(" (invalid!)");
    }
    println!();
    println!("  Version:     {}", header.version);
    println!("  Entries:     {}", header.entry_count);
    println!("  Max Entries: {}", header.max_entries);
    println!("  Memory Size: {}", header.memory_size);
    println!("  Next Offset: {} (0x{:X})", header.next_offset, header.next_offset);

    Ok(())
}

/// List all entries in a segment's table
fn cmd_entries(shm: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 256)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let mut entries = Vec::new();
    for entry_result in mem.table().entries() {
        match entry_result {
            Ok(entry) => entries.push(entry),
            Err(e) => return Err(format!("failed to read entry: {}", e)),
        }
    }

    if entries.is_empty() {
        println!("No entries in {}", name);
        return Ok(());
    }

    println!("{:<40} {:>10} {:>10} {:>20}", "NAME", "OFFSET", "SIZE", "TYPE (guess)");
    println!("{:<40} {:>10} {:>10} {:>20}", "-".repeat(40), "-".repeat(10), "-".repeat(10), "-".repeat(20));

    for entry in entries {
        let entry_name = entry.name_string();
        let type_guess = guess_type(entry.size as usize);
        println!("{:<40} {:>10} {:>10} {:>20}", entry_name, entry.offset, entry.size, type_guess);
    }

    Ok(())
}

/// Try to guess the structure type based on size
fn guess_type(size: usize) -> &'static str {
    match size {
        4 => "once",
        8 => "array/signal header",
        12 => "stack header",
        16 => "semaphore/barrier/latch/queue header",
        _ if size > 16 => "data structure",
        _ => "unknown",
    }
}

/// Create a new shared memory segment
fn cmd_create(shm: &str, size_str: &str) -> Result<(), String> {
    let name = normalize_name(shm);
    let size = parse_size(size_str)?;

    // Unlink any existing segment first
    let _ = zeroshm::platform::unlink_shm(&name);

    let mem = Memory::create(&name, size, 64)
        .map_err(|e| format!("failed to create {}: {}", name, e))?;

    println!("Created shared memory: {} ({})", name, format_size(size));

    // Drop the memory to close the mapping, but keep the segment
    drop(mem);

    Ok(())
}

/// Delete a shared memory segment
fn cmd_delete(shm: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    zeroshm::platform::unlink_shm(&name)
        .map_err(|e| format!("failed to delete {}: {}", name, e))?;

    println!("Deleted shared memory: {}", name);

    Ok(())
}

/// Hexdump the first N bytes of a segment
fn cmd_dump(shm: &str, num_bytes: usize) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let data = mem.data();
    let len = num_bytes.min(data.len());

    hex_dump(&data[..len]);

    Ok(())
}

/// Print a hex dump of data
fn hex_dump(data: &[u8]) {
    for i in (0..data.len()).step_by(16) {
        // Offset
        print!("{:08x}  ", i);

        // Hex bytes
        for j in 0..16 {
            if i + j < data.len() {
                print!("{:02x} ", data[i + j]);
            } else {
                print!("   ");
            }
            if j == 7 {
                print!(" ");
            }
        }

        // ASCII
        print!(" |");
        for j in 0..16 {
            if i + j < data.len() {
                let b = data[i + j];
                if b >= 32 && b < 127 {
                    print!("{}", b as char);
                } else {
                    print!(".");
                }
            }
        }
        println!("|");
    }
}

/// Inspect an array
fn cmd_array(shm: &str, array_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(array_name)
        .map_err(|e| format!("failed to find array: {}", e))?
        .ok_or_else(|| format!("array '{}' not found", array_name))?;

    // Read array header (capacity: u64 at offset 0)
    let offset = entry.offset as usize;
    let data = mem.data();
    let capacity = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());

    println!("Array: {}", array_name);
    println!("  Offset:   {}", entry.offset);
    println!("  Size:     {} bytes", entry.size);
    println!("  Capacity: {} elements", capacity);

    // Calculate element size
    let header_size = 8;
    let data_size = entry.size as usize - header_size;
    if capacity > 0 {
        let elem_size = data_size / capacity as usize;
        println!("  Elem Size: {} bytes", elem_size);

        // Show first few elements as hex
        let max_show = 10.min(capacity as usize);
        println!("  First {} elements (hex):", max_show);
        for i in 0..max_show {
            let elem_offset = offset + header_size + i * elem_size;
            let elem_data = &data[elem_offset..elem_offset + elem_size];
            let hex_str: String = elem_data.iter().map(|b| format!("{:02x}", b)).collect();
            println!("    [{}]: {}", i, hex_str);
        }
    }

    Ok(())
}

/// Inspect a queue
fn cmd_queue(shm: &str, queue_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(queue_name)
        .map_err(|e| format!("failed to find queue: {}", e))?
        .ok_or_else(|| format!("queue '{}' not found", queue_name))?;

    // Read queue header
    let offset = entry.offset as usize;
    let data = mem.data();
    let head = u32::from_le_bytes(data[offset + QUEUE_HEAD_OFFSET..offset + QUEUE_HEAD_OFFSET + 4].try_into().unwrap());
    let tail = u32::from_le_bytes(data[offset + QUEUE_TAIL_OFFSET..offset + QUEUE_TAIL_OFFSET + 4].try_into().unwrap());
    let capacity = u32::from_le_bytes(data[offset + QUEUE_CAPACITY_OFFSET..offset + QUEUE_CAPACITY_OFFSET + 4].try_into().unwrap());
    let elem_size = u32::from_le_bytes(data[offset + QUEUE_ELEM_SIZE_OFFSET..offset + QUEUE_ELEM_SIZE_OFFSET + 4].try_into().unwrap());

    // Calculate size
    let size = if tail >= head {
        tail - head
    } else {
        capacity - head + tail
    };

    println!("Queue: {}", queue_name);
    println!("  Offset:    {}", entry.offset);
    println!("  Head:      {}", head);
    println!("  Tail:      {}", tail);
    println!("  Capacity:  {}", capacity);
    println!("  Elem Size: {} bytes", elem_size);
    println!("  Size:      {} elements", size);
    println!("  Empty:     {}", head == tail);
    println!("  Full:      {}", (tail + 1) % capacity == head);

    Ok(())
}

/// Inspect a stack
fn cmd_stack(shm: &str, stack_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(stack_name)
        .map_err(|e| format!("failed to find stack: {}", e))?
        .ok_or_else(|| format!("stack '{}' not found", stack_name))?;

    // Read stack header
    let offset = entry.offset as usize;
    let data = mem.data();
    let top = i32::from_le_bytes(data[offset + STACK_TOP_OFFSET..offset + STACK_TOP_OFFSET + 4].try_into().unwrap());
    let capacity = u32::from_le_bytes(data[offset + STACK_CAPACITY_OFFSET..offset + STACK_CAPACITY_OFFSET + 4].try_into().unwrap());
    let elem_size = u32::from_le_bytes(data[offset + STACK_ELEM_SIZE_OFFSET..offset + STACK_ELEM_SIZE_OFFSET + 4].try_into().unwrap());

    let size = if top >= 0 {
        top as u32 + 1
    } else {
        0
    };

    println!("Stack: {}", stack_name);
    println!("  Offset:    {}", entry.offset);
    println!("  Top:       {}", top);
    println!("  Capacity:  {}", capacity);
    println!("  Elem Size: {} bytes", elem_size);
    println!("  Size:      {} elements", size);
    println!("  Empty:     {}", top < 0);
    println!("  Full:      {}", top >= capacity as i32 - 1);

    Ok(())
}

/// Inspect a semaphore
fn cmd_semaphore(shm: &str, sem_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(sem_name)
        .map_err(|e| format!("failed to find semaphore: {}", e))?
        .ok_or_else(|| format!("semaphore '{}' not found", sem_name))?;

    // Read semaphore header
    let offset = entry.offset as usize;
    let data = mem.data();
    let count = i32::from_le_bytes(data[offset + SEMAPHORE_COUNT_OFFSET..offset + SEMAPHORE_COUNT_OFFSET + 4].try_into().unwrap());
    let waiting = i32::from_le_bytes(data[offset + SEMAPHORE_WAITING_OFFSET..offset + SEMAPHORE_WAITING_OFFSET + 4].try_into().unwrap());
    let max_count = i32::from_le_bytes(data[offset + SEMAPHORE_MAX_COUNT_OFFSET..offset + SEMAPHORE_MAX_COUNT_OFFSET + 4].try_into().unwrap());

    println!("Semaphore: {}", sem_name);
    println!("  Offset:    {}", entry.offset);
    println!("  Count:     {}", count);
    println!("  Waiting:   {}", waiting);
    print!("  Max Count: {}", max_count);
    if max_count == 0 {
        print!(" (unbounded)");
    } else if max_count == 1 {
        print!(" (binary/mutex)");
    }
    println!();

    Ok(())
}

/// Inspect a mutex (which is a binary semaphore)
fn cmd_mutex(shm: &str, mtx_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(mtx_name)
        .map_err(|e| format!("failed to find mutex: {}", e))?
        .ok_or_else(|| format!("mutex '{}' not found", mtx_name))?;

    // Read semaphore header (mutex is a binary semaphore)
    let offset = entry.offset as usize;
    let data = mem.data();
    let count = i32::from_le_bytes(data[offset + SEMAPHORE_COUNT_OFFSET..offset + SEMAPHORE_COUNT_OFFSET + 4].try_into().unwrap());
    let waiting = i32::from_le_bytes(data[offset + SEMAPHORE_WAITING_OFFSET..offset + SEMAPHORE_WAITING_OFFSET + 4].try_into().unwrap());
    let max_count = i32::from_le_bytes(data[offset + SEMAPHORE_MAX_COUNT_OFFSET..offset + SEMAPHORE_MAX_COUNT_OFFSET + 4].try_into().unwrap());

    println!("Mutex: {}", mtx_name);
    println!("  Offset:  {}", entry.offset);
    println!("  Locked:  {}", count == 0);
    println!("  Waiting: {}", waiting);

    if max_count != 1 {
        println!("  Warning: max_count={} (expected 1 for mutex)", max_count);
    }

    Ok(())
}

/// Inspect a barrier
fn cmd_barrier(shm: &str, barrier_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(barrier_name)
        .map_err(|e| format!("failed to find barrier: {}", e))?
        .ok_or_else(|| format!("barrier '{}' not found", barrier_name))?;

    // Read barrier header
    let offset = entry.offset as usize;
    let data = mem.data();
    let arrived = i32::from_le_bytes(data[offset + BARRIER_ARRIVED_OFFSET..offset + BARRIER_ARRIVED_OFFSET + 4].try_into().unwrap());
    let generation = i32::from_le_bytes(data[offset + BARRIER_GENERATION_OFFSET..offset + BARRIER_GENERATION_OFFSET + 4].try_into().unwrap());
    let num_participants = i32::from_le_bytes(data[offset + BARRIER_NUM_PARTICIPANTS_OFFSET..offset + BARRIER_NUM_PARTICIPANTS_OFFSET + 4].try_into().unwrap());

    println!("Barrier: {}", barrier_name);
    println!("  Offset:       {}", entry.offset);
    println!("  Arrived:      {} / {}", arrived, num_participants);
    println!("  Generation:   {}", generation);
    println!("  Participants: {}", num_participants);

    Ok(())
}

/// Inspect a latch
fn cmd_latch(shm: &str, latch_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(latch_name)
        .map_err(|e| format!("failed to find latch: {}", e))?
        .ok_or_else(|| format!("latch '{}' not found", latch_name))?;

    // Read latch header
    let offset = entry.offset as usize;
    let data = mem.data();
    let count = i32::from_le_bytes(data[offset + LATCH_COUNT_OFFSET..offset + LATCH_COUNT_OFFSET + 4].try_into().unwrap());
    let initial_count = i32::from_le_bytes(data[offset + LATCH_INITIAL_COUNT_OFFSET..offset + LATCH_INITIAL_COUNT_OFFSET + 4].try_into().unwrap());

    println!("Latch: {}", latch_name);
    println!("  Offset:        {}", entry.offset);
    println!("  Count:         {} / {}", count, initial_count);
    println!("  Initial Count: {}", initial_count);
    println!("  Released:      {}", count == 0);

    Ok(())
}

/// Inspect a once flag
fn cmd_once(shm: &str, once_name: &str) -> Result<(), String> {
    let name = normalize_name(shm);

    let mem = Memory::open(&name, 64)
        .map_err(|e| format!("failed to open {}: {}", name, e))?;

    let entry = mem.table().find(once_name)
        .map_err(|e| format!("failed to find once flag: {}", e))?
        .ok_or_else(|| format!("once flag '{}' not found", once_name))?;

    // Read once state
    let offset = entry.offset as usize;
    let data = mem.data();
    let state = u32::from_le_bytes(data[offset + ONCE_STATE_OFFSET..offset + ONCE_STATE_OFFSET + 4].try_into().unwrap());

    println!("Once: {}", once_name);
    println!("  Offset: {}", entry.offset);
    println!("  State:    {} ({} )", state, if state == 0 { "pending" } else if state == 1 { "executing" } else { "done" });
    println!("  Called:   {}", state != 0);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("/mydata"), "/mydata");
        assert_eq!(normalize_name("mydata"), "/mydata");
        assert_eq!(normalize_name("/"), "/");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1024 * 1024), "1.0M");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("1K").unwrap(), 1024);
        assert_eq!(parse_size("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("512K").unwrap(), 512 * 1024);
        assert!(parse_size("invalid").is_err());
    }

    #[test]
    fn test_guess_type() {
        assert_eq!(guess_type(4), "once");
        assert_eq!(guess_type(8), "array/signal header");
        assert_eq!(guess_type(12), "stack header");
        assert_eq!(guess_type(16), "semaphore/barrier/latch/queue header");
        assert_eq!(guess_type(32), "data structure");
        assert_eq!(guess_type(2), "unknown");
    }
}

