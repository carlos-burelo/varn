#[inline]
pub fn char_len(s: &str, ascii: bool) -> usize {
    if ascii {
        s.len()
    } else {
        s.chars().count()
    }
}

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

#[inline]
pub fn find_bytes(haystack: &str, needle: &str) -> Option<usize> {
    memchr::memmem::find(haystack.as_bytes(), needle.as_bytes())
}

#[inline]
pub fn rfind_bytes(haystack: &str, needle: &str) -> Option<usize> {
    memchr::memmem::rfind(haystack.as_bytes(), needle.as_bytes())
}

#[inline]
pub fn byte_to_char_idx(s: &str, ascii: bool, byte_idx: usize) -> i64 {
    if ascii {
        byte_idx as i64
    } else {
        s[..byte_idx].chars().count() as i64
    }
}

pub fn split_ranges(text: &str, separator: Option<&str>, out: &mut Vec<(usize, usize)>) {
    let base = text.as_ptr() as usize;
    match separator {
        Some(sep) if sep.len() == 1 => {
            let byte = sep.as_bytes()[0];
            out.reserve(text.as_bytes().iter().filter(|&&b| b == byte).count() + 1);
            for part in text.split(byte as char) {
                out.push((part.as_ptr() as usize - base, part.len()));
            }
        }
        Some(sep) if !sep.is_empty() => {
            for part in text.split(sep) {
                out.push((part.as_ptr() as usize - base, part.len()));
            }
        }
        Some(_) | None => {
            out.reserve(text.len());
            for (offset, c) in text.char_indices() {
                out.push((offset, c.len_utf8()));
            }
        }
    }
}
