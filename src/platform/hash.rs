//! SHA-256. Source: `OSTPlatform/include/Hash.h`
//!
//! The C++ uses BCrypt. Rust's `sha2` crate produces byte-identical
//! digests, and the implementation is portable. The chunked read loop
//! matches the C++ exactly (64 KiB per read).

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Lower-case hex SHA-256 of a file, or `""` on failure.
pub fn sha256_of_file(path: &Path) -> String {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return String::new(),
        }
    }
    hex::encode(hasher.finalize())
}

/// Lower-case hex SHA-256 of a buffer.
pub fn sha256_of_buffer(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_empty_matches_known_value() {
        // SHA-256 of the empty string.
        assert_eq!(
            sha256_of_buffer(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn buffer_abc_matches_known_value() {
        assert_eq!(
            sha256_of_buffer(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn buffer_long_input() {
        // 1 MiB of 0x00. Known SHA-256 value.
        let data = vec![0u8; 1024 * 1024];
        assert_eq!(
            sha256_of_buffer(&data),
            "30e14955ebf1352266dc2ff8067e68104607e750abb9d3b36582b8af909fcb58"
        );
    }

    #[test]
    fn file_missing_returns_empty() {
        assert_eq!(sha256_of_file(Path::new("does-not-exist-xyz")), "");
    }

    #[test]
    fn file_of_abc() {
        let tmp = std::env::temp_dir().join("ost_hash_test_abc.bin");
        std::fs::write(&tmp, b"abc").unwrap();
        let h = sha256_of_file(&tmp);
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
