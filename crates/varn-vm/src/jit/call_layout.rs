//! Probes [`varn_jit::JitCallLayout`]: every offset a compiled call site walks
//! to enter a native activation and to push and pop its `CallFrame` by hand.
//! Each one is measured on a real value, not assumed.

use std::rc::Rc;

use crate::closure::VmClosure;
use crate::exec::ctx::ExecCtx;
use crate::frame::CallFrame;
use crate::heap::HeapObj;
use varn_types::FunctionProto;

const RCBOX_PREFIX: usize = 2 * std::mem::size_of::<usize>();

fn word_at(base: usize, off: usize) -> usize {
    unsafe { *((base + off) as *const usize) }
}

pub(crate) fn probe() -> varn_jit::JitCallLayout {
    let proto = Rc::new(FunctionProto::default());
    let closure = Rc::new(VmClosure::new(
        Rc::clone(&proto),
        Vec::new(),
        crate::settings::ExecSettings::default(),
    ));
    let control = Rc::as_ptr(&closure) as usize - RCBOX_PREFIX;

    let slot: Option<HeapObj> = Some(HeapObj::VmClosure(Rc::clone(&closure)));
    let size = std::mem::size_of::<Option<HeapObj>>();
    let bytes = unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
    let closure_tag = bytes[0] as usize;
    let closure_payload_off = (0..=size - 8)
        .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == control)
        .expect("VmClosure payload probe failed")
        + crate::heap::cells::HEADER_BYTES;

    let strong = Rc::strong_count(&closure);
    let rc_strong_off = (0..RCBOX_PREFIX)
        .step_by(8)
        .find(|&off| word_at(control, off) == strong)
        .expect("Rc strong count probe failed");
    let extra = Rc::clone(&closure);
    assert_eq!(
        word_at(control, rc_strong_off),
        strong + 1,
        "Rc strong count probe ambiguous"
    );
    drop(extra);

    let closure_proto_off = std::mem::offset_of!(VmClosure, proto);
    assert_eq!(
        word_at(Rc::as_ptr(&closure) as usize, closure_proto_off),
        Rc::as_ptr(&proto) as usize - RCBOX_PREFIX,
        "VmClosure.proto does not hold the proto's Rc control pointer"
    );

    let owned = CallFrame::new_owned(Rc::clone(&closure), CallFrame::NO_ACTIVATION);
    let frame_owned_off = std::mem::offset_of!(CallFrame, _owned_closure);
    let frame_class_off = std::mem::offset_of!(CallFrame, current_class);
    let frame_addr = &owned as *const CallFrame as usize;
    assert_eq!(
        word_at(frame_addr, frame_owned_off),
        control,
        "Option<Rc<VmClosure>> does not store the control pointer"
    );
    assert_eq!(
        word_at(frame_addr, frame_class_off),
        0,
        "Option<Rc<ClassObj>>::None is not a null word"
    );
    drop(owned);
    drop(slot);

    let (vec_ptr_off, vec_len_off, vec_cap_off) = super::frame_layout::probe_vec_words();
    let class = varn_types::ClassObj::new_rc("__probe_vtable");
    let class_addr = Rc::as_ptr(&class) as usize;
    let vtable_vec_off = class.vtable.as_ptr() as usize - class_addr;
    class.vtable.borrow_mut().push(varn_types::VmValue::null());
    assert_eq!(
        word_at(class_addr, vtable_vec_off + vec_ptr_off),
        class.vtable.borrow().as_ptr() as usize,
        "ClassObj vtable data pointer probe failed"
    );
    let frames_off = std::mem::offset_of!(ExecCtx, frames)
        + crate::frame_stack::FrameStack::frames_field_offset();

    varn_jit::JitCallLayout {
        closure_tag,
        closure_payload_off,
        rc_value_off: RCBOX_PREFIX,
        rc_strong_off,
        closure_proto_off,
        proto_native_off: std::mem::offset_of!(FunctionProto, jit_native),
        proto_native_sig_off: std::mem::offset_of!(FunctionProto, jit_native_sig),
        proto_epoch_off: std::mem::offset_of!(FunctionProto, jit_epoch),
        frames_ptr_off: frames_off + vec_ptr_off,
        frames_len_off: frames_off + vec_len_off,
        frames_cap_off: frames_off + vec_cap_off,
        frame_size: std::mem::size_of::<CallFrame>(),
        frame_closure_ptr_off: std::mem::offset_of!(CallFrame, closure_ptr),
        frame_owned_off,
        frame_ip_off: std::mem::offset_of!(CallFrame, ip),
        frame_base_off: std::mem::offset_of!(CallFrame, base),
        frame_class_off,
        frame_return_reg_off: std::mem::offset_of!(CallFrame, return_reg),
        class_vtable_ptr_off: vtable_vec_off + vec_ptr_off,
        class_vtable_len_off: vtable_vec_off + vec_len_off,
        class_vtable_version_off: std::mem::offset_of!(varn_types::ClassObj, vtable_version),
        no_activation: CallFrame::NO_ACTIVATION,
        no_return_reg: CallFrame::NO_RETURN_REG as usize,
        max_call_depth: crate::frame::MAX_CALL_DEPTH,
    }
}
