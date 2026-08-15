//! Char-index <-> byte-offset utilities shared by the VM string intrinsics
//! and the native `str` builtins. Varn string ops are char-indexed (Unicode
//! scalar values); for ASCII strings char index == byte index, which these
//! helpers exploit to avoid per-call scans.

/// Char count, O(1) when the string is known ASCII.
#[inline]
pub fn char_len(s: &str, ascii: bool) -> usize {
    if ascii {
        s.len()
    } else {
        s.chars().count()
    }
}

/// Map a clamped char-index range (`si <= ei <= char_len`) to byte offsets.
/// O(1) for ASCII; a single forward scan otherwise — never collects.
#[inline]
pub fn char_range_to_bytes(s: &str, ascii: bool, si: usize, ei: usize) -> (usize, usize) {
    if ascii {
        return (si, ei);
    }
    let mut bs = s.len();
    let mut be = s.len();
    for (chars_seen, (byte_idx, _)) in s.char_indices().enumerate() {
        if chars_seen == si {
            bs = byte_idx;
        }
        if chars_seen == ei {
            be = byte_idx;
            break;
        }
    }
    (bs, be)
}

/// Byte offset of the first occurrence of `needle` in `haystack`.
///
/// `memmem` rather than `str::find`: the standard library's Two-Way search is
/// scalar, and these are the string ops that run over a whole buffer. Both
/// answer in byte offsets and both are exact-match, so the result is
/// identical — only the scan is different.
#[inline]
pub fn find_bytes(haystack: &str, needle: &str) -> Option<usize> {
    memchr::memmem::find(haystack.as_bytes(), needle.as_bytes())
}

/// Byte offset of the LAST occurrence of `needle` in `haystack`.
#[inline]
pub fn rfind_bytes(haystack: &str, needle: &str) -> Option<usize> {
    memchr::memmem::rfind(haystack.as_bytes(), needle.as_bytes())
}

/// Char index of a byte offset (as produced by `str::find`/`rfind`).
#[inline]
pub fn byte_to_char_idx(s: &str, ascii: bool, byte_idx: usize) -> i64 {
    if ascii {
        byte_idx as i64
    } else {
        s[..byte_idx].chars().count() as i64
    }
}
