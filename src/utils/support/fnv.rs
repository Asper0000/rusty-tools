//! FNV-1a 32-bit hash. Source: `Utils/Support/FnvHash.h`

/// Compile-time FNV-1a hash of a byte string.
pub const fn fnv1a_hash(s: &str) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x01000193);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        // Empty string hash is the FNV-1a offset basis.
        assert_eq!(fnv1a_hash(""), 0x811c9dc5);
        assert_eq!(fnv1a_hash("a"), 0xe40c292c);
        assert_eq!(fnv1a_hash("foobar"), 0xbf9cf968);
    }

    #[test]
    fn const_evaluable() {
        const H: u32 = fnv1a_hash("BBuildAndAsyncSendFrame");
        // Value taken from the C++ static_assert in PatternLoader.cpp.
        assert_eq!(H, 0x82428E37);
    }
}
