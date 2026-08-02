//! Probed layout for the JIT's inline string-slot allocation (Stage B).
//!
//! Split out of `heap.rs` (already over the project's file-size ceiling)
//! rather than added to it, the same way `heap_str_alloc.rs` was. `Heap`,
//! `HeapInner`, `HeapObj`, `HeapStr`, `INLINE_STR_CAP` and `ascii_flag` are
//! all already `pub` within the crate, so this module needs no visibility
//! widening — it is an ordinary `impl Heap` block living in a second file,
//! which Rust allows freely within one crate.

use crate::heap::{ascii_flag, Heap, HeapInner, HeapObj, HeapStr, INLINE_STR_CAP};

impl Heap {
    /// Probed layout facts for the JIT's inline string allocation (see
    /// [`varn_jit::JitStrLayout`]).
    ///
    /// Nothing here is hardcoded. The template is a real
    /// `Some(HeapObj::Str(HeapStr::Inline { .. }))` captured as bytes, and the
    /// field offsets come from taking references into that same value — so a
    /// change to `HeapObj`, to `HeapStr`, or to the compiler's niche
    /// placement moves the emitted code with it instead of silently
    /// invalidating it. `HeapStr::Inline` holds no `Rc` (its bytes live
    /// directly in the heap slot), so the probe value below owns nothing that
    /// needs a matching drop or forget — it can be built, read through raw
    /// offsets, and dropped normally like any other plain data.
    pub fn jit_str_layout() -> varn_jit::JitStrLayout {
        let slot: Option<HeapObj> = Some(HeapObj::Str(HeapStr::Inline {
            len: 0,
            ascii: std::cell::Cell::new(ascii_flag::UNKNOWN),
            bytes: [0u8; INLINE_STR_CAP],
        }));
        let size = std::mem::size_of::<Option<HeapObj>>();
        assert!(
            size <= varn_jit::STR_TEMPLATE_MAX,
            "Option<HeapObj> ({size} B) outgrew the JIT template buffer"
        );

        let base = &slot as *const _ as usize;
        let (len_off, bytes_off) = match &slot {
            Some(HeapObj::Str(HeapStr::Inline { len, bytes, .. })) => (
                (len as *const u8 as usize) - base,
                (bytes.as_ptr() as usize) - base,
            ),
            _ => unreachable!("just built as Inline"),
        };

        let mut template = [0u8; varn_jit::STR_TEMPLATE_MAX];
        let raw = unsafe { std::slice::from_raw_parts(base as *const u8, size) };
        template[..size].copy_from_slice(raw);
        let str_tag = raw[0] as usize;

        // The niche assumption: None and non-Str variants must differ in the
        // tag byte the emitted guard reads, exactly as `jit_array_layout`
        // checks for `HeapObj::Array`.
        let none_slot: Option<HeapObj> = None;
        let none_tag = unsafe { *(&none_slot as *const _ as *const u8) } as usize;
        assert_ne!(str_tag, none_tag, "Option<HeapObj> niche probe failed");

        varn_jit::JitStrLayout {
            str_tag,
            template,
            slot_size: size,
            len_off,
            bytes_off,
            inline_cap: INLINE_STR_CAP,
            nursery_fwd_vec_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, nursery)
                + crate::nursery::Nursery::forwarding_vec_byte_offset(),
            alloc_count_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, nursery)
                + std::mem::offset_of!(crate::nursery::Nursery, alloc_count),
            nursery_capacity: crate::nursery::NURSERY_CAPACITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout the JIT will write through must reconstruct a string the
    /// VM can read back. This is the test that catches a representation
    /// change before it becomes a segfault in generated code.
    #[test]
    fn jit_str_layout_round_trips() {
        let lay = Heap::jit_str_layout();
        assert!(lay.slot_size <= varn_jit::STR_TEMPLATE_MAX);
        assert_eq!(lay.inline_cap, INLINE_STR_CAP);
        assert_eq!(lay.nursery_capacity, crate::nursery::NURSERY_CAPACITY);

        // Build a slot the way emitted code will: template, then len, then
        // bytes — and nothing else.
        let mut raw = [0u8; varn_jit::STR_TEMPLATE_MAX];
        raw[..lay.slot_size].copy_from_slice(&lay.template[..lay.slot_size]);
        let payload = b"gc_400000";
        raw[lay.len_off] = payload.len() as u8;
        raw[lay.bytes_off..lay.bytes_off + payload.len()].copy_from_slice(payload);

        let slot: Option<HeapObj> =
            unsafe { std::ptr::read(raw.as_ptr() as *const Option<HeapObj>) };
        match &slot {
            Some(HeapObj::Str(hs)) => {
                assert_eq!(hs.as_str(), "gc_400000");
                assert_eq!(hs.len(), 9);
            }
            _ => panic!("layout-built slot did not read back as a string"),
        }
        // `slot` was reconstructed from raw bytes via `ptr::read`, which
        // means the bytes still logically belong to `raw` too — dropping
        // `slot` normally would be sound here (`Inline` owns no `Rc`), but
        // forgetting it is the same discipline the JIT relies on: the slot
        // this probe models is not this test's to free, the nursery's is.
        std::mem::forget(slot);

        // The tag byte the emitted guard reads must distinguish it from None.
        let none: Option<HeapObj> = None;
        let none_tag = unsafe { *(&none as *const _ as *const u8) } as usize;
        assert_ne!(lay.str_tag, none_tag, "Option<HeapObj> niche probe failed");
    }
}
