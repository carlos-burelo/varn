//! `"prefix" + <int>` built once instead of three times.
//!
//! Split out of `heap.rs` (already over the project's file-size ceiling)
//! rather than added to it. `HeapInner`, `HeapObj`, `HeapStr`,
//! `INLINE_STR_CAP` and `ascii_flag` are all already `pub` within the crate,
//! so this module needs no visibility widening — it is an ordinary
//! `impl HeapInner` block living in a second file, which Rust allows freely
//! within one crate.

use crate::heap::{ascii_flag, HeapInner, HeapObj, HeapStr, INLINE_STR_CAP};
use crate::value::VmValue;

impl HeapInner {
    /// Concatenate string `a` and integer `b` straight into one byte array,
    /// handed to the nursery once.
    ///
    /// `str_concat`'s general path copies the payload three times — into a
    /// `StrBuf`, into the zeroed `[u8; INLINE_STR_CAP]` that
    /// `HeapStr::inline` builds, and again moving the `HeapObj` into the
    /// slot — plus a `try_from_sso` scan of the assembled result. This writes
    /// the bytes once and knows the length before it starts.
    ///
    /// `None` means "not my shape, use the general path":
    ///
    /// * `b` is not an int — the digit fast path is the whole point;
    /// * `a` is `Ext` — it **must** fall through, or `str_concat`'s
    ///   accumulation path is bypassed and `s = s + x` goes quadratic;
    /// * the result exceeds `INLINE_STR_CAP` — it needs an `Rc`.
    ///
    /// A result that fits SSO is handed back as an SSO value directly rather
    /// than declined: the bytes are already assembled by the time the length
    /// is known, so returning them beats making `str_concat`'s general path
    /// re-render both operands into a `StrBuf` from scratch. Non-ASCII bytes
    /// can never be SSO (`try_from_sso` refuses anything over 127) and fall
    /// through to the heap-inline representation below instead.
    pub(crate) fn alloc_str_concat_inline(&mut self, a: VmValue, b: VmValue) -> Option<VmValue> {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};

        if !b.is_int() {
            return None;
        }

        // Resolve `a`'s bytes. SSO materializes into a local; a heap string
        // borrows. `Ext` declines here, before anything is written.
        let mut sso_buf = [0u8; 5];
        let a_bytes: &[u8] = if a.is_sso() {
            a.sso_as_str(&mut sso_buf).as_bytes()
        } else if a.is_heap() {
            match self.get(a.as_heap_idx()) {
                Some(HeapObj::Str(HeapStr::Ext { .. })) => return None,
                Some(HeapObj::Str(hs)) => hs.as_str().as_bytes(),
                _ => return None,
            }
        } else {
            return None;
        };

        // A prefix that already exceeds the inline capacity cannot produce an
        // inline result no matter how few digits `b` has — decline before
        // paying for the digits. Formatting first and discarding it is what
        // made the >37-byte shape measurably slower than not having this path
        // at all.
        if a_bytes.len() > INLINE_STR_CAP {
            return None;
        }

        let mut digits = [0u8; INT_MAX_DIGITS];
        let digits = itoa(b.as_int(), &mut digits).as_bytes();
        let total = a_bytes.len() + digits.len();

        // Above INLINE_STR_CAP the bytes cannot live in the slot.
        if total > INLINE_STR_CAP {
            return None;
        }

        let mut bytes = [0u8; INLINE_STR_CAP];
        bytes[..a_bytes.len()].copy_from_slice(a_bytes);
        bytes[a_bytes.len()..total].copy_from_slice(digits);

        // The bytes are built; declining here would make `str_concat` render
        // both operands again from scratch. `try_from_sso` refuses non-ASCII
        // at any length, so a short multibyte result falls through to the
        // inline heap representation below rather than being lost.
        if total <= 5 {
            if let Some(sso) = VmValue::try_from_sso(
                std::str::from_utf8(&bytes[..total]).expect("operands were valid UTF-8"),
            ) {
                return Some(sso);
            }
        }

        // ASCII is decided here for free: the digits always are, so the
        // answer is `a`'s. Recording it saves the first `.length` a scan.
        let ascii = if a_bytes.is_ascii() {
            ascii_flag::YES
        } else {
            ascii_flag::NO
        };

        Some(self.alloc_str_view(HeapStr::Inline {
            len: total as u8,
            ascii: std::cell::Cell::new(ascii),
            bytes,
        }))
    }
}
