//! Byte-pattern scanning. Source: `OSTPlatform/Windows/ByteSearch.cpp`
//!
//! * [`find`] - BMH scan over an in-memory buffer.
//! * [`find_in_file`] - whole-file scan via [`find_in_file_range`].
//! * [`find_in_file_range`] - scan a sub-range of a file with a
//!   sliding window, re-reading `pattern.len() - 1` overlapping bytes
//!   between chunks so a match that straddles a boundary is still
//!   found.
//!
//! ## Simplification vs C++
//!
//! The C++ uses overlapped (async) reads with double buffering so the
//! scan of chunk i overlaps the read of chunk i+1. This port uses
//! synchronous positioned reads. The observable behaviour is
//! identical - same results, same overlap logic - but the port does
//! not exploit disk/CPU concurrency. If throughput becomes a
//! bottleneck (e.g. while scanning a ~100 MB module in ProtectionScan),
//! swap the read loop for `windows-sys` `ReadFile` with `OVERLAPPED`;
//! the public API will not change.

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;

pub const DEFAULT_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// BMH pattern scanner. Matches the C++ `FixedPatternScanner` exactly.
struct FixedPatternScanner {
    pattern: Vec<u8>,
    skip: [usize; 256],
}

impl FixedPatternScanner {
    fn new(pattern: &[u8]) -> Self {
        let ps = pattern.len();
        // C++ fills with pattern size (or 1 for empty), then overwrites
        // all positions except the last.
        let fill = if ps == 0 { 1 } else { ps };
        let mut skip = [fill; 256];
        if ps > 1 {
            for i in 0..(ps - 1) {
                skip[pattern[i] as usize] = ps - 1 - i;
            }
        }
        Self { pattern: pattern.to_vec(), skip }
    }

    fn find(&self, bytes: &[u8]) -> Option<usize> {
        let ps = self.pattern.len();
        if ps == 0 || bytes.len() < ps {
            return None;
        }
        let mut offset = 0usize;
        while offset <= bytes.len() - ps {
            let tail = bytes[offset + ps - 1];
            if tail == self.pattern[ps - 1] {
                let mut i = ps - 1;
                while i > 0 && bytes[offset + i - 1] == self.pattern[i - 1] {
                    i -= 1;
                }
                if i == 0 {
                    return Some(offset);
                }
            }
            offset += self.skip[tail as usize];
        }
        None
    }
}

/// Find `needle` in `haystack`. Returns the byte offset of the first
/// match, or `None`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    FixedPatternScanner::new(needle).find(haystack)
}

/// Read the full buffer, looping on short reads. Stops early on EOF;
/// returns the number of bytes actually read.
fn read_fully(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0usize;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// Scan an entire file for `needle`. Returns the absolute file offset
/// of the first match.
pub fn find_in_file(path: &Path, needle: &[u8], chunk_bytes: usize) -> Option<u64> {
    // A whole-file scan is a range scan over [0, EOF). The range variant
    // clamps `size` to the real file size, so u64::MAX means "to EOF".
    find_in_file_range(path, 0, u64::MAX, needle, chunk_bytes)
}

/// Scan a sub-range of a file for `needle`.
///
/// `offset` and `size` describe a half-open `[offset, offset+size)`
/// window. If `offset + size` exceeds the file length, the window is
/// clamped to EOF. Returns the absolute file offset of the first match
/// in the window.
pub fn find_in_file_range(
    path: &Path,
    offset: u64,
    size: u64,
    needle: &[u8],
    chunk_bytes: usize,
) -> Option<u64> {
    let scanner = FixedPatternScanner::new(needle);
    let ps = scanner.pattern.len();
    if ps == 0 || size == 0 {
        return None;
    }

    let mut file = File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();
    if file_size == 0 {
        return None;
    }
    if offset >= file_size {
        return None;
    }

    let range_end = offset + size.min(file_size - offset);
    let chunk = chunk_bytes.max(ps);
    // Consecutive chunks overlap by (ps - 1) bytes so a match that
    // straddles a chunk boundary is fully contained at the head of the
    // next chunk.
    let stride = (chunk - (ps - 1)) as u64;

    let mut buffer = vec![0u8; chunk];
    let mut cur_start = offset;

    while cur_start < range_end {
        let remaining = range_end - cur_start;
        let to_read = chunk.min(remaining as usize);

        if file.seek(SeekFrom::Start(cur_start)).is_err() {
            return None;
        }
        let n = read_fully(&mut file, &mut buffer[..to_read]).ok()?;
        if n == 0 {
            break;
        }

        if let Some(local) = scanner.find(&buffer[..n]) {
            return Some(cur_start + local as u64);
        }

        let next_start = cur_start + stride;
        if next_start >= range_end || next_start <= cur_start {
            break;
        }
        cur_start = next_start;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ─── in-memory BMH ──────────────────────────────────────────────

    #[test]
    fn find_empty_needle_none() {
        assert_eq!(find(b"abc", b""), None);
    }

    #[test]
    fn find_needle_longer_than_haystack() {
        assert_eq!(find(b"ab", b"abc"), None);
    }

    #[test]
    fn find_exact_match() {
        assert_eq!(find(b"abc", b"abc"), Some(0));
    }

    #[test]
    fn find_at_start() {
        assert_eq!(find(b"abcdef", b"abc"), Some(0));
    }

    #[test]
    fn find_at_end() {
        assert_eq!(find(b"abcdef", b"def"), Some(3));
    }

    #[test]
    fn find_in_middle() {
        assert_eq!(find(b"abcdefgh", b"de"), Some(3));
    }

    #[test]
    fn find_returns_first_match() {
        assert_eq!(find(b"abcabcabc", b"abc"), Some(0));
    }

    #[test]
    fn find_no_match() {
        assert_eq!(find(b"hello world", b"xyz"), None);
    }

    #[test]
    fn find_single_byte() {
        assert_eq!(find(b"\x00\x01\x02", b"\x01"), Some(1));
    }

    #[test]
    fn find_binary_pattern() {
        let haystack = [0u8, 1, 2, 3, 4, 5];
        let needle = [3u8, 4];
        assert_eq!(find(&haystack, &needle), Some(3));
    }

    #[test]
    fn find_pattern_that_almost_matches() {
        // BMH skip table must not miss a match after a partial overlap.
        assert_eq!(find(b"abcabxabcaby", b"abcaby"), Some(6));
    }

    #[test]
    fn find_overlapping_candidates() {
        assert_eq!(find(b"aaab", b"aab"), Some(1));
    }

    // ─── file scanning ──────────────────────────────────────────────

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        let mut f = File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn file_scan_finds_at_start() {
        let p = write_temp("ost_bs_start.bin", b"MATCH_and_rest_of_bytes");
        let r = find_in_file(&p, b"MATCH", 1024);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, Some(0));
    }

    #[test]
    fn file_scan_finds_in_middle() {
        let p = write_temp("ost_bs_mid.bin", b"xxxxxHELLOxxxxx");
        let r = find_in_file(&p, b"HELLO", 1024);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, Some(5));
    }

    #[test]
    fn file_scan_matches_across_chunk_boundary() {
        // 16-byte chunk, 6-byte pattern. Place the pattern so it
        // straddles the boundary between chunk 0 and chunk 1.
        let mut data = vec![b'a'; 13];
        data.extend_from_slice(b"NEEDLE");
        data.extend_from_slice(&[b'b'; 32]);
        let p = write_temp("ost_bs_boundary.bin", &data);
        // chunk = 16, ps = 6, stride = 16 - 5 = 11. So chunk 0 = [0,16),
        // chunk 1 = [11, 27), etc. The match at offset 13 straddles.
        let r = find_in_file_range(&p, 0, data.len() as u64, b"NEEDLE", 16);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, Some(13));
    }

    #[test]
    fn file_scan_respects_range_start() {
        let p = write_temp("ost_bs_range.bin", b"HELLO_WORLD_HELLO");
        // Only search [10, 20) - should find the second HELLO at 12.
        let r = find_in_file_range(&p, 10, 10, b"HELLO", 1024);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, Some(12));
    }

    #[test]
    fn file_scan_range_clamps_to_eof() {
        let p = write_temp("ost_bs_clamp.bin", b"abcdef");
        // Ask for 1000 bytes from offset 3; file is only 6 bytes.
        let r = find_in_file_range(&p, 3, 1000, b"def", 1024);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, Some(3));
    }

    #[test]
    fn file_scan_no_match_returns_none() {
        let p = write_temp("ost_bs_miss.bin", b"abcdefghij");
        let r = find_in_file(&p, b"xyz", 4);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, None);
    }

    #[test]
    fn file_scan_offset_past_eof_returns_none() {
        let p = write_temp("ost_bs_past.bin", b"abc");
        let r = find_in_file_range(&p, 100, 10, b"a", 4);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, None);
    }

    #[test]
    fn file_scan_missing_file_returns_none() {
        let r = find_in_file(Path::new("definitely-not-a-real-file-xyz"), b"a", 4);
        assert_eq!(r, None);
    }
}
