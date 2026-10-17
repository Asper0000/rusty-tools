//! Strict integer parsers. Source: `OSTPlatform/Numbers.cpp`
//!
//! Every parser is strict in the sense of C++ `std::from_chars`:
//!  * the entire input must be consumed,
//!  * no leading or trailing whitespace,
//!  * no leading `+` (rejected for both signed and unsigned by `from_chars`),
//!  * `-` is accepted only for signed types,
//!  * overflow is a failure, not a clamp,
//!  * for hex, an optional `0x` / `0X` prefix is stripped first.
//!
//! Rust's `from_str_radix` matches most of these rules natively. The one
//! delta is the leading `+`: Rust accepts it for signed types, the C++
//! `from_chars` does not. Both base-10 and base-16 helpers reject it
//! explicitly.

use super::encoding::wide_to_utf8;

// ─── TextSlice ──────────────────────────────────────────────────────

/// A byte-range descriptor, mirroring the C++ `TextSlice { offset; count }`.
/// `count == usize::MAX` means "to the end of the string" (the C++
/// `std::string_view::npos` sentinel).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TextSlice {
    pub offset: usize,
    pub count: usize,
}

impl TextSlice {
    /// Whole string.
    pub const FULL: TextSlice = TextSlice { offset: 0, count: usize::MAX };
    /// Explicit range.
    pub const fn new(offset: usize, count: usize) -> Self {
        Self { offset, count }
    }
}

fn apply_slice<'a>(value: &'a str, slice: TextSlice) -> Option<&'a str> {
    if slice.offset > value.len() {
        return None;
    }
    // `count == usize::MAX` saturates and then gets capped below.
    let end = slice.offset.saturating_add(slice.count).min(value.len());
    value.get(slice.offset..end)
}

fn strip_hex_prefix(value: &str) -> &str {
    let b = value.as_bytes();
    if b.len() >= 2 && b[0] == b'0' && (b[1] == b'x' || b[1] == b'X') {
        return &value[2..];
    }
    value
}

// ─── Base helpers ───────────────────────────────────────────────────

macro_rules! define_base10 {
    ($name:ident, $t:ty) => {
        fn $name(value: &str) -> Option<$t> {
            if value.is_empty() || value.starts_with('+') {
                return None;
            }
            <$t>::from_str_radix(value, 10).ok()
        }
    };
}

define_base10!(base10_u64, u64);
define_base10!(base10_u32, u32);
define_base10!(base10_i64, i64);
define_base10!(base10_i32, i32);

macro_rules! define_base16 {
    ($name:ident, $t:ty) => {
        fn $name(value: &str) -> Option<$t> {
            let s = strip_hex_prefix(value);
            if s.is_empty() || s.starts_with('+') {
                return None;
            }
            <$t>::from_str_radix(s, 16).ok()
        }
    };
}

define_base16!(base16_u64, u64);
define_base16!(base16_u32, u32);
define_base16!(base16_u8,  u8);

// ─── Public API ─────────────────────────────────────────────────────
//
// One family per type: plain, sliced, and wide (UTF-16) variants.
// The wide variants convert via `wide_to_utf8` and delegate, matching
// the C++ overload set exactly.

macro_rules! define_family {
    ($parse:ident, $parse_slice:ident, $parse_wide:ident, $t:ty, $base:ident) => {
        pub fn $parse(value: &str) -> Option<$t> {
            $base(value)
        }
        pub fn $parse_slice(value: &str, slice: TextSlice) -> Option<$t> {
            $parse(apply_slice(value, slice)?)
        }
        pub fn $parse_wide(value: &[u16]) -> Option<$t> {
            $parse(&wide_to_utf8(value))
        }
    };
}

define_family!(parse_u64,     parse_u64_slice,     parse_u64_wide,     u64, base10_u64);
define_family!(parse_u32,     parse_u32_slice,     parse_u32_wide,     u32, base10_u32);
define_family!(parse_i64,     parse_i64_slice,     parse_i64_wide,     i64, base10_i64);
define_family!(parse_i32,     parse_i32_slice,     parse_i32_wide,     i32, base10_i32);

define_family!(parse_hex_u64, parse_hex_u64_slice, parse_hex_u64_wide, u64, base16_u64);
define_family!(parse_hex_u32, parse_hex_u32_slice, parse_hex_u32_wide, u32, base16_u32);
define_family!(parse_hex_u8,  parse_hex_u8_slice,  parse_hex_u8_wide,  u8,  base16_u8);

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_full_and_offsets() {
        assert_eq!(apply_slice("abc", TextSlice::FULL), Some("abc"));
        assert_eq!(apply_slice("abcdef", TextSlice::new(2, 3)), Some("cde"));
        assert_eq!(apply_slice("abcdef", TextSlice::new(4, 100)), Some("ef"));
        assert_eq!(apply_slice("abc", TextSlice::new(10, 1)), None);
    }

    #[test]
    fn parse_u64_basic() {
        assert_eq!(parse_u64("0"), Some(0));
        assert_eq!(parse_u64("123"), Some(123));
        assert_eq!(parse_u64("18446744073709551615"), Some(u64::MAX));
    }

    #[test]
    fn parse_u64_rejects() {
        assert_eq!(parse_u64(""), None);
        assert_eq!(parse_u64("+1"), None);
        assert_eq!(parse_u64("-1"), None);
        assert_eq!(parse_u64(" 1"), None);
        assert_eq!(parse_u64("1 "), None);
        assert_eq!(parse_u64("0x10"), None);
        assert_eq!(parse_u64("18446744073709551616"), None);
        assert_eq!(parse_u64("1.5"), None);
        assert_eq!(parse_u64("abc"), None);
    }

    #[test]
    fn parse_u32_bounds() {
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
    }

    #[test]
    fn parse_i64_basic() {
        assert_eq!(parse_i64("0"), Some(0));
        assert_eq!(parse_i64("-1"), Some(-1));
        assert_eq!(parse_i64("-0"), Some(0));
        assert_eq!(parse_i64("9223372036854775807"), Some(i64::MAX));
        assert_eq!(parse_i64("-9223372036854775808"), Some(i64::MIN));
    }

    #[test]
    fn parse_i64_rejects() {
        assert_eq!(parse_i64("+1"), None);
        assert_eq!(parse_i64(" 1"), None);
        assert_eq!(parse_i64("9223372036854775808"), None);
        assert_eq!(parse_i64("-9223372036854775809"), None);
    }

    #[test]
    fn parse_i32_bounds() {
        assert_eq!(parse_i32("-5"), Some(-5));
        assert_eq!(parse_i32("2147483647"), Some(i32::MAX));
        assert_eq!(parse_i32("-2147483648"), Some(i32::MIN));
        assert_eq!(parse_i32("2147483648"), None);
    }

    #[test]
    fn parse_hex_u64_basic() {
        assert_eq!(parse_hex_u64("0"), Some(0));
        assert_eq!(parse_hex_u64("10"), Some(16));
        assert_eq!(parse_hex_u64("0x10"), Some(16));
        assert_eq!(parse_hex_u64("0X10"), Some(16));
        assert_eq!(parse_hex_u64("ff"), Some(255));
        assert_eq!(parse_hex_u64("FF"), Some(255));
        assert_eq!(parse_hex_u64("deadbeef"), Some(0xDEAD_BEEF));
    }

    #[test]
    fn parse_hex_u64_rejects() {
        assert_eq!(parse_hex_u64(""), None);
        assert_eq!(parse_hex_u64("0x"), None);
        assert_eq!(parse_hex_u64("+1"), None);
        assert_eq!(parse_hex_u64("-1"), None);
        assert_eq!(parse_hex_u64("0xgg"), None);
        assert_eq!(parse_hex_u64("0x 1"), None);
    }

    #[test]
    fn parse_hex_u32_overflow() {
        assert_eq!(parse_hex_u32("ffffffff"), Some(u32::MAX));
        assert_eq!(parse_hex_u32("100000000"), None);
    }

    #[test]
    fn parse_hex_u8_bounds() {
        assert_eq!(parse_hex_u8("00"), Some(0));
        assert_eq!(parse_hex_u8("ff"), Some(255));
        assert_eq!(parse_hex_u8("0xFF"), Some(255));
        assert_eq!(parse_hex_u8("100"), None);
    }

    #[test]
    fn slice_variants() {
        assert_eq!(parse_u64_slice("12abc34", TextSlice::new(0, 2)), Some(12));
        assert_eq!(parse_u64_slice("12abc34", TextSlice::new(5, 2)), Some(34));
        assert_eq!(parse_u64_slice("12abc34", TextSlice::new(10, 2)), None);
        assert_eq!(parse_u64_slice("12abc34", TextSlice::new(2, 3)), None);
    }

    #[test]
    fn wide_variants() {
        let w: Vec<u16> = "123".encode_utf16().collect();
        assert_eq!(parse_u64_wide(&w), Some(123));

        let w: Vec<u16> = "ff".encode_utf16().collect();
        assert_eq!(parse_hex_u8_wide(&w), Some(255));

        let w: Vec<u16> = "abc".encode_utf16().collect();
        assert_eq!(parse_u64_wide(&w), None);

        let w: Vec<u16> = "-5".encode_utf16().collect();
        assert_eq!(parse_i32_wide(&w), Some(-5));
    }
}
