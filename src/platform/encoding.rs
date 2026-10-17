//! Wide <-> UTF-8 conversions. Source: `OSTPlatform/Encoding.cpp`
//!
//! The C++ version calls `WideCharToMultiByte` / `MultiByteToWideChar`
//! with `CP_UTF8` and default flags. In that mode, invalid input is
//! substituted with U+FFFD rather than rejected. Rust's
//! `String::from_utf16_lossy` and `str::encode_utf16` behave identically
//! without the FFI hop, so no `windows` crate binding is needed here.
//!
//! The C++ returns an empty string on failure. Its failure modes are
//! API-level (null pointers, oversized length) rather than content-based;
//! only the length guard survives the port. All other inputs succeed.

use crate::ostp_debug;
use crate::platform::logging::Level;

/// A UTF-16 wide string. On Windows `wchar_t` is 16 bits, one `u16`
/// per code unit.
pub type Wide = [u16];

/// Maximum length, matching the C++ guard against `int` overflow in the
/// Win32 length parameter. `WideCharToMultiByte` takes an `int` count,
/// so inputs above `INT_MAX` would be truncated silently.
const MAX_LEN: usize = i32::MAX as usize;

/// Convert a UTF-16 slice to a UTF-8 `String`.
///
/// Unpaired surrogates become U+FFFD, matching `WideCharToMultiByte` with
/// `CP_UTF8` and no `WC_ERR_INVALID_CHARS` flag.
pub fn wide_to_utf8(value: &[u16]) -> String {
    if value.is_empty() {
        return String::new();
    }
    if value.len() > MAX_LEN {
        ostp_debug!("wide_to_utf8: input too large ({} code units)", value.len());
        return String::new();
    }
    String::from_utf16_lossy(value)
}

/// Convert a UTF-8 `&str` to a UTF-16 `Vec<u16>`.
///
/// Rust `&str` is always valid UTF-8, so this never fails on content.
pub fn utf8_to_wide(value: &str) -> Vec<u16> {
    if value.is_empty() {
        return Vec::new();
    }
    if value.len() > MAX_LEN {
        ostp_debug!("utf8_to_wide: input too large ({} bytes)", value.len());
        return Vec::new();
    }
    value.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_roundtrip() {
        assert_eq!(wide_to_utf8(&[]), "");
        assert_eq!(utf8_to_wide(""), Vec::<u16>::new());
    }

    #[test]
    fn ascii_roundtrip() {
        let s = "hello world";
        let w = utf8_to_wide(s);
        assert_eq!(w.len(), s.len()); // ASCII: 1 byte = 1 code unit
        assert_eq!(wide_to_utf8(&w), s);
    }

    #[test]
    fn non_ascii_roundtrip() {
        // 'é' = 1 BMP unit, '日' = 1 BMP unit, '🦀' = 1 surrogate pair
        let s = "caf\u{00E9} \u{2014} \u{65E5}\u{672C}\u{8A9E} \u{2014} \u{1F980}";
        let w = utf8_to_wide(s);
        assert_eq!(wide_to_utf8(&w), s);
        // c a f é (4) + ' — ' (3) + 日本語 (3) + ' — ' (3) + 🦀 (2) = 15
        assert_eq!(w.len(), 15);
    }

    #[test]
    fn lone_high_surrogate_becomes_replacement() {
        // Unpaired D800. C++ substitutes U+FFFD.
        let wide = [0xD800u16, 0x0041, 0x0042];
        let utf8 = wide_to_utf8(&wide);
        assert!(utf8.starts_with('\u{FFFD}'));
        assert!(utf8.ends_with("AB"));
    }

    #[test]
    fn lone_low_surrogate_becomes_replacement() {
        let wide = [0xDC00u16, 0x0041];
        let utf8 = wide_to_utf8(&wide);
        assert!(utf8.starts_with('\u{FFFD}'));
    }

    #[test]
    fn valid_surrogate_pair_preserved() {
        let wide = [0xD83Du16, 0xDE00u16]; // U+1F600
        assert_eq!(wide_to_utf8(&wide), "\u{1F600}");
    }

    #[test]
    fn empty_utf16_input_gives_empty_string() {
        assert_eq!(wide_to_utf8(&[]), "");
    }
}
