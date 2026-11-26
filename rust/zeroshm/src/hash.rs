//! Hash function for ZeroSHM table lookups.
//!
//! Uses xxhash3-64 with seed 0x2FE7 to ensure identical hash values
//! across all language implementations (C++, Go, Rust).

use crate::layout::XXHASH_SEED;

/// Compute the xxhash3-64 hash of a name with the ZeroSHM seed.
///
/// This must produce bit-identical results to the Go implementation's
/// `xxh3.HashStringSeed(name, 0x2FE7)` and the C++ equivalent.
pub fn hash_name(name: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64_with_seed(name.as_bytes(), XXHASH_SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_matches_go_implementation() {
        // Expected values computed with Go's xxh3.HashStringSeed(name, 0x2FE7)
        // These must match bit-for-bit for cross-language compatibility.

        assert_eq!(hash_name(""), 0x6F6B80A7AF229771, "empty string");
        assert_eq!(hash_name("a"), 0x05CB3C57576994DB, "single char");
        assert_eq!(hash_name("sensor_data"), 0x9DEAF1F38845EB61, "sensor_data");
        assert_eq!(hash_name("event_queue"), 0x6C2B019BFB7F12B2, "event_queue");
        assert_eq!(hash_name("temperature"), 0xDC2A53514B2A6094, "temperature");
        assert_eq!(hash_name("Sensor_Data"), 0x822E2FB840715B33, "Sensor_Data");

        let name = "a".repeat(255);
        assert_eq!(hash_name(&name), 0x9414F720B685E0A8, "255-char name");
    }

    #[test]
    fn test_hash_name_deterministic() {
        let h1 = hash_name("sensor_data");
        let h2 = hash_name("sensor_data");
        assert_eq!(h1, h2, "hash must be deterministic");
    }

    #[test]
    fn test_hash_different_names_different_hashes() {
        let h1 = hash_name("sensor_data");
        let h2 = hash_name("event_queue");
        let h3 = hash_name("temperature");
        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }
}

