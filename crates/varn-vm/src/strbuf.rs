//! Stack-first scratch buffer for building strings, and a `core::fmt`-free
//! integer formatter.
//!
//! Concatenation used to build its result in a `String` and then copy it into
//! the final `Rc<str>` — two allocations and a copy for a result like
//! `"User_" + 12345`, which is eleven bytes. [`StrBuf`] holds that on the stack
//! and only reaches for the heap if the result actually outgrows it, so the
//! common case pays one allocation: the `Rc<str>` the caller keeps.

use std::fmt;

/// `i64::MIN` in decimal — `-9223372036854775808` — is 20 bytes, the longest
/// an integer can render to.
pub const INT_MAX_DIGITS: usize = 20;

/// Renders `v` in decimal into `buf`, returning the written slice.
///
/// `write!(out, "{}", v)` drags in `core::fmt`'s formatting machinery, which
/// dominated integer concatenation. This is the same output, digit by digit.
pub(crate) fn itoa(v: i64, buf: &mut [u8; INT_MAX_DIGITS]) -> &str {
    let negative = v < 0;
    // Via `u64`: `i64::MIN` has no positive counterpart, and `wrapping_neg`
    // on its bit pattern yields exactly its magnitude.
    let mut n = if negative {
        (v as u64).wrapping_neg()
    } else {
        v as u64
    };

    let mut i = INT_MAX_DIGITS;
    if n == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
    }
    if negative {
        i -= 1;
        buf[i] = b'-';
    }

    // Only ASCII digits and '-' were written.
    std::str::from_utf8(&buf[i..]).expect("itoa writes ASCII only")
}

/// How much a `StrBuf` holds before it spills to the heap. Sized so a short
/// prefix plus a formatted scalar — the shape of nearly every concat — fits.
pub const INLINE_CAP: usize = 64;

/// A string being built: inline until it outgrows [`INLINE_CAP`], then heap.
pub struct StrBuf {
    inline: [u8; INLINE_CAP],
    len: usize,
    spilled: Option<String>,
}

impl Default for StrBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl StrBuf {
    pub(crate) fn new() -> Self {
        Self {
            inline: [0; INLINE_CAP],
            len: 0,
            spilled: None,
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &self.spilled {
            Some(s) => s.len(),
            None => self.len,
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        match &self.spilled {
            Some(s) => s,
            // Only whole `&str`s are ever appended, so the inline bytes are
            // always a complete UTF-8 sequence.
            None => std::str::from_utf8(&self.inline[..self.len])
                .expect("StrBuf only ever appends whole &str"),
        }
    }

    pub(crate) fn push_str(&mut self, s: &str) {
        if let Some(spilled) = &mut self.spilled {
            spilled.push_str(s);
            return;
        }
        if self.len + s.len() <= INLINE_CAP {
            self.inline[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
            return;
        }
        // Outgrew the stack. Move what we have to the heap once, then continue
        // there; the buffer never goes back to being inline.
        let mut spilled = String::with_capacity((self.len + s.len()) * 2);
        spilled.push_str(
            std::str::from_utf8(&self.inline[..self.len])
                .expect("StrBuf only ever appends whole &str"),
        );
        spilled.push_str(s);
        self.spilled = Some(spilled);
    }

    /// Takes the contents as an owned `String`, for callers that need to keep
    /// growing it (the extensible-buffer path in `str_concat`).
    pub(crate) fn into_string(self) -> String {
        match self.spilled {
            Some(s) => s,
            None => self.as_str().to_owned(),
        }
    }
}

impl fmt::Write for StrBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s);
        Ok(())
    }
}