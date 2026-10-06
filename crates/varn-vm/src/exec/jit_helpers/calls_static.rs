use super::construct::jit_propagate_error;
use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::frame::CallFrame;
use crate::value::VmValue;

pub(super) enum Handoff {
    Entry(usize),
    Ran(VmResult<VmValue>),
}

pub(super) unsafe fn hand_off(ctx: &mut ExecCtx, caller_depth: usize, out: *mut usize) -> Handoff {
    if ctx.frames.len() > crate::frame::MAX_CALL_DEPTH {
        return Handoff::Ran(Err(crate::error::RuntimeError::new(
            "stack overflow: call depth exceeded 10000",
        )));
    }
    if ctx.frames.len() == caller_depth + 1
        && ctx
            .pending_constructors
            .last()
            .is_none_or(|(depth, _)| *depth != caller_depth)
    {
        let top = &ctx.frames[caller_depth];
        if let Some(entry) = direct_entry(top.closure()) {
            out.write(top.closure_ptr as usize);
            out.add(1).write(top.base);
            return Handoff::Entry(entry);
        }
    }
    Handoff::Ran(ctx.run_until(caller_depth))
}

pub(super) unsafe fn settle(ctx: &mut ExecCtx, handoff: Handoff) -> usize {
    match handoff {
        Handoff::Entry(entry) => entry,
        Handoff::Ran(Ok(v)) => {
            ctx.jit_native_result = v;
            0
        }
        Handoff::Ran(Err(e)) => jit_propagate_error(ctx, e),
    }
}

fn direct_entry(closure: &VmClosure) -> Option<usize> {
    let proto = &closure.proto;
    if proto.is_async || proto.is_generator || proto.resumes_in_interpreter() {
        return None;
    }
    closure.jit_fn().map(|f| f as usize)
}

#[varn_op_macros::jit_slow(field = "jit_call_leave")]
pub(crate) extern "C" fn jit_call_leave(ctx: *mut ExecCtx, alloc: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.frames.pop();
        ctx_ref.drop_frame_storage(alloc);
    }
}

#[varn_op_macros::jit_slow(field = "jit_call_self_window")]
pub(crate) extern "C" fn jit_call_self_window(
    ctx: *mut ExecCtx,
    window: *const VmValue,
    argc: usize,
    out: *mut usize,
) -> usize {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let closure_ref = &*ctx_ref.frames[caller_depth - 1].closure_ptr;
        let proto = closure_ref.proto.clone();
        let callee_alloc = ctx_ref.stack.push_frame(&proto);
        for (i, v) in std::slice::from_raw_parts(window, argc).iter().enumerate() {
            let addr = ctx_ref.stack.addr_of(callee_alloc, i);
            if let Err(e) = ctx_ref.stack.set_addr(addr, *v) {
                ctx_ref.stack.pop_frame();
                jit_propagate_error(ctx_ref, e);
            }
        }
        ctx_ref
            .frames
            .push(CallFrame::new(closure_ref, callee_alloc));
        let handoff = hand_off(ctx_ref, caller_depth, out);
        settle(ctx_ref, handoff)
    }
}



#[varn_op_macros::jit_slow(field = "jit_push_native_frame")]
pub(crate) extern "C" fn jit_push_native_frame(ctx: *mut ExecCtx, closure: *const VmClosure) {
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.frames.len() >= crate::frame::MAX_CALL_DEPTH {
            jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new("stack overflow: call depth exceeded 10000"),
            );
        }
        std::rc::Rc::increment_strong_count(closure);
        let owned = std::rc::Rc::from_raw(closure);
        ctx_ref
            .frames
            .push(CallFrame::new_owned(owned, CallFrame::NO_ACTIVATION));
    }
}



#[varn_op_macros::jit_slow(field = "jit_release_closure")]
pub(crate) extern "C" fn jit_release_closure(closure: *const VmClosure) {
    unsafe { drop(std::rc::Rc::from_raw(closure)) }
}
