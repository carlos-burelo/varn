//! Probes [`varn_jit::JitFrameLayout`] — everything the fully-inlined
//! frame-aware `Call` fast path needs to walk a callee value down to a
//! compiled entry and push/pop a `CallFrame` by hand, no Rust call, when
//! `ctx.frames`/`ctx.stack` already have room for it. See the type's own
//! doc in `varn-jit` for what each field is and isn't here for.

use crate::exec::ctx::ExecCtx;
use crate::frame_store::FrameStore;

/// `Vec<T>`'s raw (ptr, len, cap) word offsets. Element-type-independent, so
/// one run covers every class vector plus `allocs`.
pub(super) fn probe_vec_words() -> (usize, usize, usize) {
    let mut v: Vec<u64> = Vec::with_capacity(7);
    v.extend([0, 0, 0]);
    let words: [usize; 3] = unsafe { std::mem::transmute_copy(&v) };
    let ptr = v.as_ptr() as usize;
    let mut ptr_off = usize::MAX;
    let mut len_off = usize::MAX;
    let mut cap_off = usize::MAX;
    for (i, w) in words.iter().enumerate() {
        if *w == ptr {
            ptr_off = i * 8;
        } else if *w == 3 {
            len_off = i * 8;
        } else if *w == 7 {
            cap_off = i * 8;
        }
    }
    assert!(
        ptr_off != usize::MAX && len_off != usize::MAX && cap_off != usize::MAX,
        "Vec<T> layout probe failed"
    );
    (ptr_off, len_off, cap_off)
}

pub(crate) fn probe() -> varn_jit::JitFrameLayout {
    let (vec_ptr_off, _vec_len_off, _vec_cap_off) = probe_vec_words();

    let stack_off = std::mem::offset_of!(ExecCtx, stack);
    let gpr_ptr_offset = stack_off + std::mem::offset_of!(FrameStore, gpr) + vec_ptr_off;
    let fpr_ptr_offset = stack_off + std::mem::offset_of!(FrameStore, fpr) + vec_ptr_off;
    let refs_ptr_offset = stack_off + std::mem::offset_of!(FrameStore, refs) + vec_ptr_off;
    let dyn_ptr_offset = stack_off + std::mem::offset_of!(FrameStore, dyn_) + vec_ptr_off;

    let allocs_ptr_offset = stack_off + std::mem::offset_of!(FrameStore, allocs) + vec_ptr_off;
    let alloc_size = std::mem::size_of::<crate::frame_store::FrameAlloc>();
    let alloc_bases_offset = std::mem::offset_of!(crate::frame_store::FrameAlloc, bases);
    assert_eq!(
        alloc_bases_offset, 0,
        "FrameAlloc::bases must be the first repr(C) field for the JIT ABI"
    );

    varn_jit::JitFrameLayout {
        gpr_ptr_offset,
        fpr_ptr_offset,
        refs_ptr_offset,
        dyn_ptr_offset,
        allocs_ptr_offset,
        alloc_size,
        alloc_bases_offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four class data-pointer offsets must land exactly on the matching
    /// `FrameStore` vectors' data words. This is the ABI contract the fase-B
    /// lowering will emit against; a stale/duplicated offset here is a
    /// generated code store into the wrong class vector.
    #[test]
    fn class_ptr_offsets_point_at_the_class_vectors() {
        let mut store = crate::frame_store::FrameStore::new();
        store.gpr.push(7);
        store.fpr.push(1.5);
        store.refs.push(None);
        store.dyn_.push(varn_types::VmValue::null());

        let lay = probe();
        let stack_off = std::mem::offset_of!(ExecCtx, stack);
        let base = &store as *const crate::frame_store::FrameStore as *const u8;

        unsafe {
            let gpr = *(base.add(lay.gpr_ptr_offset - stack_off) as *const *const i64);
            let fpr = *(base.add(lay.fpr_ptr_offset - stack_off) as *const *const f64);
            let refs = *(base.add(lay.refs_ptr_offset - stack_off)
                as *const *const Option<varn_types::HeapRef>);
            let dyn_ =
                *(base.add(lay.dyn_ptr_offset - stack_off) as *const *const varn_types::VmValue);
            assert_eq!(gpr, store.gpr.as_ptr());
            assert_eq!(fpr, store.fpr.as_ptr());
            assert_eq!(refs, store.refs.as_ptr());
            assert_eq!(dyn_, store.dyn_.as_ptr());
        }

        // `FrameAlloc.bases` is the first `#[repr(C)]` field: the JIT reads it
        // at offset 0 from an activation's `FrameAlloc`.
        assert_eq!(
            std::mem::offset_of!(crate::frame_store::FrameAlloc, bases),
            0
        );
    }

    /// `allocs[act_id].bases` resolved through the probed offsets must match
    /// `FrameStore::alloc_bases(act_id)` — the activation's four class bases.
    #[test]
    fn alloc_bases_offsets_resolve_to_the_activation_bases() {
        use varn_types::register_meta::{RegisterMeta, SlotKind};
        let proto = std::rc::Rc::new(varn_types::FunctionProto {
            register_count: 4,
            register_meta: [
                SlotKind::Dynamic,
                SlotKind::Int,
                SlotKind::Float,
                SlotKind::Ref,
            ]
            .iter()
            .map(|&kind| RegisterMeta { kind })
            .collect(),
            ..varn_types::FunctionProto::default()
        });

        let mut store = crate::frame_store::FrameStore::new();
        let id = store.push_frame(&proto);
        let expected = store.alloc_bases(id);

        let lay = probe();
        let stack_off = std::mem::offset_of!(ExecCtx, stack);
        let base = &store as *const crate::frame_store::FrameStore as *const u8;
        unsafe {
            let allocs = *(base.add(lay.allocs_ptr_offset - stack_off) as *const *const u8);
            let fap = allocs.add(id * lay.alloc_size + lay.alloc_bases_offset);
            let mut got = [0u32; 4];
            for (c, slot) in got.iter_mut().enumerate() {
                *slot = *(fap.add(c * 4) as *const u32);
            }
            assert_eq!(got, expected);
        }
    }
}
