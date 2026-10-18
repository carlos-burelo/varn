use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

#[varn_macros::jit_slow(field = "dispatch_intrinsic")]
pub(crate) extern "C" fn jit_dispatch_intrinsic(
    ctx: *mut ExecCtx,
    wire_byte: usize,
    act_id: usize,
    reg_start: usize,
    arg_count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = ctx_ref.stack.box_range(act_id, reg_start, arg_count);
        if crate::home_trace::enabled() {
            let fname = ctx_ref
                .frames
                .last()
                .and_then(|f| f.closure().proto.name.clone());
            let tags: Vec<String> = args
                .iter()
                .map(|v| format!("{:#x}/{:#x}", v.raw_tag(), v.raw_payload()))
                .collect();
            eprintln!("INTRINSIC {fname:?} wire={wire_byte:#x} args={tags:?}");
        }
        match crate::exec::intrinsics::dispatch(wire_byte as u8, &args) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

#[varn_macros::jit_slow(field = "intrinsic_window")]
pub(crate) extern "C" fn jit_intrinsic_window(
    ctx: *mut ExecCtx,
    wire_byte: usize,
    window: *const VmValue,
    count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = std::slice::from_raw_parts(window, count);
        match crate::exec::intrinsics::dispatch(wire_byte as u8, args) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

#[varn_macros::jit_slow(field = "str_char_code_at")]
pub(crate) extern "C" fn jit_str_char_code_at(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    pos_tag: u64,
    pos_payload: u64,
) -> i64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &mut ctx_ref.heap;
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        let pos = VmValue::from_raw_parts(pos_tag, pos_payload);
        let signed = heap.as_int(pos);
        if signed < 0 {
            return -1;
        }
        let idx = signed as usize;

        if receiver.is_sso() {
            let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
            let s = receiver.sso_as_str(&mut buf);
            if s.is_ascii() {
                return s.as_bytes().get(idx).map(|&b| b as i64).unwrap_or(-1);
            }
            return s.chars().nth(idx).map(|c| c as i64).unwrap_or(-1);
        }

        if receiver.is_heap()
            && let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap())
        {
            let s = h.as_str();
            let code = if h.is_ascii_cached() {
                s.as_bytes().get(idx).map(|&b| b as i64)
            } else {
                h.is_ascii();
                if h.is_ascii_cached() {
                    s.as_bytes().get(idx).map(|&b| b as i64)
                } else {
                    s.chars().nth(idx).map(|c| c as i64)
                }
            };
            return code.unwrap_or(-1);
        }
        -1
    }
}

#[varn_macros::jit_slow(field = "str_char_at")]
pub(crate) extern "C" fn jit_str_char_at(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    pos_tag: u64,
    pos_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let text = VmValue::from_raw_parts(recv_tag, recv_payload);
        let index = ctx_ref
            .heap
            .as_int(VmValue::from_raw_parts(pos_tag, pos_payload));
        match crate::exec::collections::char_index(&ctx_ref.heap, text, index) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}

#[varn_macros::jit_slow(field = "str_ascii_bytes")]
pub(crate) extern "C" fn jit_str_ascii_bytes(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
) -> *const u8 {
    unsafe {
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        match ascii_view(&(*ctx).heap, receiver) {
            Some(s) => s.as_ptr(),
            None => std::ptr::null(),
        }
    }
}

#[varn_macros::jit_slow(field = "str_ascii_len")]
pub(crate) extern "C" fn jit_str_ascii_len(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
) -> i64 {
    unsafe {
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        ascii_view(&(*ctx).heap, receiver).map_or(0, |s| s.len() as i64)
    }
}

#[inline]
fn ascii_view(heap: &crate::heap::Heap, receiver: VmValue) -> Option<&str> {
    if !receiver.is_heap() {
        return None;
    }
    let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap()) else {
        return None;
    };

    h.is_ascii().then(|| h.as_str())
}

#[inline(always)]
pub(super) unsafe fn borrow_str_fast<'a>(
    v: VmValue,
    heap: &'a Heap,
    buf: &'a mut [u8; varn_types::vm_value::SSO_MAX_LEN],
) -> Option<&'a str> {
    if v.is_sso() {
        return Some(v.sso_as_str(buf));
    }
    if v.is_heap()
        && let Some(HeapObj::Str(h)) = heap.get(v.as_heap())
    {
        return Some(h.as_str());
    }
    None
}
