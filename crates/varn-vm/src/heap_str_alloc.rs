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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Heap;

    /// The inline concat path must agree with the general path on every input
    /// it accepts, and must decline (rather than produce a wrong answer) on
    /// everything else. The `None` cases are as load-bearing as the `Some`
    /// ones: an `Ext` left operand taken here would bypass `str_concat`'s
    /// accumulation path and make `s = s + x` quadratic.
    #[test]
    fn inline_concat_agrees_with_the_general_path() {
        let heap = Heap::new();
        let h = unsafe { heap.inner_mut() };

        // (left, right) pairs whose combined length exceeds the 5-byte SSO
        // cap and must round-trip to the expected string through the inline
        // path.
        let cases: &[(&str, i64, &str)] = &[
            ("gc_", 123, "gc_123"),
            ("gc_", 400000, "gc_400000"),
            ("ab", -700, "ab-700"),
            ("prefix", -1, "prefix-1"),
            // Exactly INLINE_STR_CAP (37): 34 chars + "123".
            (
                "abcdefghijklmnopqrstuvwxyz01234567",
                123,
                "abcdefghijklmnopqrstuvwxyz01234567123",
            ),
        ];
        for (l, r, want) in cases {
            let a = h.alloc_str_dynamic(l);
            let b = VmValue::from_int(*r);
            let got = h
                .alloc_str_concat_inline(a, b)
                .expect("should have been built inline");
            assert_eq!(h.str_repr(got), *want, "inline concat of {l:?} + {r}");
        }

        // At or below the 5-byte SSO cap, the inline path now hands back the
        // bytes it already built as an SSO value instead of declining and
        // making the general path re-render both operands from scratch.
        for (l, r, want) in [
            ("", 0i64, "0"),
            ("gc_", 1, "gc_1"),
            ("ab", -7, "ab-7"),
            ("", -1, "-1"),
        ] {
            let a = h.alloc_str_dynamic(l);
            let b = VmValue::from_int(r);
            let got = h
                .alloc_str_concat_inline(a, b)
                .unwrap_or_else(|| panic!("SSO-sized result ({l:?} + {r}) must not decline"));
            assert!(
                got.is_sso(),
                "{want:?} should be an SSO value, not a heap string"
            );
            assert_eq!(h.str_repr(got), want);
        }

        // One past INLINE_STR_CAP must decline.
        let long = h.alloc_str_dynamic("abcdefghijklmnopqrstuvwxyz012345678");
        assert!(
            h.alloc_str_concat_inline(long, VmValue::from_int(123))
                .is_none(),
            "38-byte result must decline"
        );

        // A non-int right operand must decline.
        let s = h.alloc_str_dynamic("x");
        assert!(
            h.alloc_str_concat_inline(s, s).is_none(),
            "non-int rhs must decline"
        );

        // Non-ASCII left operand still round-trips (bytes are copied whole).
        let uni = h.alloc_str_dynamic("日本語のプレフィックス");
        let got = h
            .alloc_str_concat_inline(uni, VmValue::from_int(5))
            .expect("multibyte prefix fits in 37 bytes");
        assert_eq!(h.str_repr(got), "日本語のプレフィックス5");
    }

    /// A left operand whose bytes alone already exceed `INLINE_STR_CAP` must
    /// decline before `itoa` ever runs on `b` — that is the ordering fix
    /// this test pins down, distinct from the "one past the cap after
    /// adding digits" case above. And regardless of *why* it declined, the
    /// general path it falls through to must still produce the right string.
    #[test]
    fn oversized_prefix_declines_before_itoa_and_general_path_is_correct() {
        let mut heap = Heap::new();
        let h = unsafe { heap.inner_mut() };

        // 43 bytes on its own — already past INLINE_STR_CAP (37) with zero
        // digits appended, so the decline must not depend on `b` at all.
        let prefix = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";
        assert!(
            prefix.len() > INLINE_STR_CAP,
            "fixture must exceed INLINE_STR_CAP on its own"
        );

        let a = h.alloc_str_dynamic(prefix);
        assert!(
            h.alloc_str_concat_inline(a, VmValue::from_int(123))
                .is_none(),
            "oversized prefix must decline regardless of digit count"
        );

        // Falling through to the general path must still round-trip correctly.
        let want = format!("{prefix}123");
        let got = crate::exec::strings::str_concat(a, VmValue::from_int(123), &mut heap);
        assert_eq!(heap.str_repr(got), want);
    }

    /// A result that fits SSO used to be declined AFTER its bytes were built,
    /// making the general path re-render both operands. The bytes are already
    /// in hand; hand them back.
    #[test]
    fn sso_sized_results_come_back_from_the_inline_path() {
        let heap = Heap::new();
        let h = unsafe { heap.inner_mut() };

        for (l, r, want) in [("g", 1i64, "g1"), ("ab", 123, "ab123"), ("", 0, "0")] {
            let a = h.alloc_str_dynamic(l);
            let got = h
                .alloc_str_concat_inline(a, VmValue::from_int(r))
                .expect("sso-sized result must not be declined");
            assert!(
                got.is_sso(),
                "{want:?} should be an SSO value, not a heap string"
            );
            assert_eq!(h.str_repr(got), want);
        }

        // Non-ASCII cannot be SSO at any length: `try_from_sso` refuses bytes
        // over 127. It must still produce the right string.
        let uni = h.alloc_str_dynamic("é");
        let got = h
            .alloc_str_concat_inline(uni, VmValue::from_int(1))
            .expect("non-ascii short result must still be served");
        assert!(!got.is_sso());
        assert_eq!(h.str_repr(got), "é1");
    }
}
