use crate::exec::ctx::ExecCtx;
use crate::frame_store::FrameStore;

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
