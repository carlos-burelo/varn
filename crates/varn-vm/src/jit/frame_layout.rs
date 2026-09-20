//! Probes [`varn_jit::JitFrameLayout`] — everything the fully-inlined
//! frame-aware `Call` fast path needs to walk a callee value down to a
//! compiled entry and push/pop a `CallFrame` by hand, no Rust call, when
//! `ctx.frames`/`ctx.stack` already have room for it. See the type's own
//! doc in `varn-jit` for what each field is and isn't here for.

use crate::closure::VmClosure;
use crate::exec::ctx::ExecCtx;
use crate::frame::CallFrame;
use crate::heap::HeapObj;
use std::rc::Rc;
use varn_types::FunctionProto;

/// `Vec<T>`'s raw (ptr, len, cap) word offsets. Element-type-independent —
/// same reasoning `Heap::jit_array_layout`'s doc gives for reusing one
/// `Vec<Option<HeapObj>>` probe for `Vec<VmValue>` too — so one run of this
/// covers `ExecCtx.frames: Vec<CallFrame>` and `ExecCtx.stack: Vec<VmValue>`
/// alike.
fn probe_vec_words() -> (usize, usize, usize) {
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
    // `RcBox = { strong: Cell<usize>, weak: Cell<usize>, value: T }` — the
    // same assumption `Heap::rcbox_ptr_for_validation` and
    // `Heap::nursery_len_byte_offset_from_rcbox` already make unprobed. Used
    // here only for the read-back sanity checks below, not stored in the
    // returned layout (the CLIF codegen restates the same `-16` directly,
    // rather than plumbing a redundant field through `JitFrameLayout` for a
    // constant every other Rc-walking probe in this file already assumes).
    const RC_CTRL: usize = 2 * std::mem::size_of::<usize>();

    // --- Option<HeapObj>'s VmClosure variant: discriminant + payload. ---
    let dummy_proto: Rc<FunctionProto> = Rc::new(FunctionProto::default());
    let dummy_closure = Rc::new(VmClosure::new(
        dummy_proto,
        Vec::new(),
        crate::settings::ExecSettings::default(),
    ));
    let closure_rcbox = Rc::as_ptr(&dummy_closure) as usize - RC_CTRL;
    let slot: Option<HeapObj> = Some(HeapObj::VmClosure(dummy_closure.clone()));
    let size = std::mem::size_of::<Option<HeapObj>>();
    let bytes = unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
    let closure_tag = bytes[0] as usize;
    let closure_payload_off = (0..=size - 8)
        .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == closure_rcbox)
        .expect("closure payload probe failed");
    let none_tag = unsafe { *(&(None::<HeapObj>) as *const _ as *const u8) } as usize;
    assert_ne!(
        closure_tag, none_tag,
        "Option<HeapObj> niche probe failed for VmClosure"
    );

    // --- VmClosure -> proto: a plain Rc pointer field, offset_of! exact. ---
    // A field's raw stored bytes are `Rc<T>`'s own internal pointer, which
    // points at the RCBOX BASE (`{strong, weak, value}`'s start) — NOT what
    // `Rc::as_ptr()` returns, which is already adjusted past the header to
    // `value`. Same convention the payload-slot probes above and elsewhere
    // in this codebase (`Heap::rcbox_ptr_for_validation`) already use; this
    // read-back check has to match it too, or it is comparing two different
    // things.
    let closure_proto_off = std::mem::offset_of!(VmClosure, proto);
    let expected_proto_rcbox = Rc::as_ptr(&dummy_closure.proto) as usize - RC_CTRL;
    let read_back = unsafe {
        *((Rc::as_ptr(&dummy_closure) as *const u8).add(closure_proto_off) as *const usize)
    };
    assert_eq!(
        read_back, expected_proto_rcbox,
        "closure_proto_off probe read-back mismatch"
    );

    // --- FunctionProto fields: pub, offset_of! exact, no probe needed. ---
    let proto_jit_entry_off = std::mem::offset_of!(FunctionProto, jit_entry);
    let proto_jit_epoch_off = std::mem::offset_of!(FunctionProto, jit_epoch);
    let proto_register_count_off = std::mem::offset_of!(FunctionProto, register_count);
    let proto_has_rest_off = std::mem::offset_of!(FunctionProto, has_rest);
    let proto_is_async_off = std::mem::offset_of!(FunctionProto, is_async);
    let proto_is_generator_off = std::mem::offset_of!(FunctionProto, is_generator);

    // --- CallFrame fields: repr(C), pub, offset_of! exact. ---
    let frame_size = std::mem::size_of::<CallFrame>();
    let frame_closure_ptr_off = std::mem::offset_of!(CallFrame, closure_ptr);
    let frame_owned_closure_off = std::mem::offset_of!(CallFrame, _owned_closure);
    let frame_ip_off = std::mem::offset_of!(CallFrame, ip);
    let frame_base_off = std::mem::offset_of!(CallFrame, base);
    let frame_current_class_off = std::mem::offset_of!(CallFrame, current_class);
    let frame_return_reg_off = std::mem::offset_of!(CallFrame, return_reg);

    // The two `Option<Rc<T>>` fields DO need probing: this codebase does not
    // own their layout, and unlike `usize`/`u16` fields they might (in
    // principle) not be pointer-sized if the niche optimization ever failed
    // to apply. Verified, not assumed.
    {
        let some: Option<Rc<VmClosure>> = Some(dummy_closure.clone());
        let some_bits = unsafe { *(&some as *const _ as *const usize) };
        assert_eq!(
            some_bits,
            Rc::as_ptr(&dummy_closure) as usize - RC_CTRL,
            "Option<Rc<VmClosure>> is not a bare (RcBox-base) pointer for Some"
        );
        let none: Option<Rc<VmClosure>> = None;
        let none_bits = unsafe { *(&none as *const _ as *const usize) };
        assert_eq!(
            none_bits, 0,
            "Option<Rc<VmClosure>> is not null-niched for None"
        );
        assert_eq!(
            std::mem::size_of::<Option<Rc<VmClosure>>>(),
            std::mem::size_of::<usize>(),
            "Option<Rc<VmClosure>> is not pointer-sized"
        );
    }
    {
        let cls = varn_types::ClassObj::new_rc("__jit_frame_layout_probe");
        let some: Option<Rc<varn_types::ClassObj>> = Some(cls.clone());
        let some_bits = unsafe { *(&some as *const _ as *const usize) };
        assert_eq!(
            some_bits,
            Rc::as_ptr(&cls) as usize - RC_CTRL,
            "Option<Rc<ClassObj>> is not a bare (RcBox-base) pointer for Some"
        );
        let none: Option<Rc<varn_types::ClassObj>> = None;
        let none_bits = unsafe { *(&none as *const _ as *const usize) };
        assert_eq!(
            none_bits, 0,
            "Option<Rc<ClassObj>> is not null-niched for None"
        );
        assert_eq!(
            std::mem::size_of::<Option<Rc<varn_types::ClassObj>>>(),
            std::mem::size_of::<usize>(),
            "Option<Rc<ClassObj>> is not pointer-sized"
        );
    }

    // --- ExecCtx.frames: Vec<CallFrame>, ExecCtx.stack: Vec<VmValue>. ---
    let (vec_ptr_off, vec_len_off, vec_cap_off) = probe_vec_words();
    let frames_ptr_offset = std::mem::offset_of!(ExecCtx, frames) + vec_ptr_off;
    let frames_len_offset = std::mem::offset_of!(ExecCtx, frames) + vec_len_off;
    let frames_cap_offset = std::mem::offset_of!(ExecCtx, frames) + vec_cap_off;
    let stack_len_offset = std::mem::offset_of!(ExecCtx, stack) + vec_len_off;
    let stack_cap_offset = std::mem::offset_of!(ExecCtx, stack) + vec_cap_off;

    varn_jit::JitFrameLayout {
        closure_tag,
        closure_payload_off,
        closure_proto_off,
        proto_jit_entry_off,
        proto_jit_epoch_off,
        proto_register_count_off,
        proto_has_rest_off,
        proto_is_async_off,
        proto_is_generator_off,
        frame_size,
        frame_closure_ptr_off,
        frame_owned_closure_off,
        frame_ip_off,
        frame_base_off,
        frame_current_class_off,
        frame_return_reg_off,
        frames_ptr_offset,
        frames_len_offset,
        frames_cap_offset,
        stack_len_offset,
        stack_cap_offset,
    }
}
