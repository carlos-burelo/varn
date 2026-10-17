use crate::exec::ctx::ExecCtx;
use crate::exec::ctx_tasks::AwaitStep;
use crate::value::VmValue;

pub(super) unsafe fn jit_suspend_at(ctx: &mut ExecCtx, frame_idx: usize, resume_ip: usize) -> ! {
    ctx.frames[frame_idx].ip = resume_ip;

    ctx.jit_panic_suspend_resume_ip = None;

    let buf = ctx.jit_suspend_buf;
    if buf.is_null() {
        panic!("JIT suspend triggered but no jump buffer registered");
    }
    unsafe { crate::exec::ctx::my_longjmp(buf, 2) }
}

#[varn_macros::jit_slow(field = "spawn")]
pub(crate) extern "C" fn jit_spawn(ctx: *mut ExecCtx, task_tag: u64, task_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let task_val = VmValue::from_raw_parts(task_tag, task_payload);
        ctx_ref.jit_native_result = ctx_ref.exec_spawn(task_val).unwrap();
    }
}

#[varn_macros::jit_slow(field = "await_helper")]
pub(crate) extern "C" fn jit_await(
    ctx: *mut ExecCtx,
    fut_tag: u64,
    fut_payload: u64,
    dest_reg: u32,
    resume_ip: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let fut = VmValue::from_raw_parts(fut_tag, fut_payload);
        let caller_depth = ctx_ref.frames.len();
        ctx_ref.frames[caller_depth - 1].ip = resume_ip;
        let dest = dest_reg as u16;
        let step = ctx_ref.begin_await(fut, dest);
        settle_await(ctx_ref, step, caller_depth, dest, resume_ip);
    }
}

#[varn_macros::jit_slow(field = "await_call")]
pub(crate) extern "C" fn jit_await_call(
    ctx: *mut ExecCtx,
    window: *const VmValue,
    total: usize,
    dest_reg: u32,
    resume_ip: usize,
    out: *mut usize,
) -> usize {
    unsafe {
        let ctx_ref = &mut *ctx;
        let window = std::slice::from_raw_parts(window, total);
        let caller_depth = ctx_ref.frames.len();
        ctx_ref.frames[caller_depth - 1].ip = resume_ip;
        let dest = dest_reg as u16;
        let step = ctx_ref.await_call(window[0], window, dest);
        if let Ok(AwaitStep::Entered) = step
            && let Some(entry) = mounted_entry(ctx_ref, caller_depth)
        {
            let top = &ctx_ref.frames[caller_depth];
            out.write(top.closure_ptr as usize);
            out.add(1).write(top.base);
            return entry;
        }
        settle_await(ctx_ref, step, caller_depth, dest, resume_ip);
        0
    }
}

fn mounted_entry(ctx: &ExecCtx, caller_depth: usize) -> Option<usize> {
    if ctx.frames.len() != caller_depth + 1 {
        return None;
    }
    let closure = ctx.frames[caller_depth].closure();
    if closure.proto.resumes_in_interpreter() {
        return None;
    }
    closure.jit_fn().map(|f| f as usize)
}

unsafe fn settle_await(
    ctx: &mut ExecCtx,
    step: crate::error::VmResult<AwaitStep>,
    caller_depth: usize,
    dest: u16,
    resume_ip: usize,
) {
    let resume_at = match step {
        Ok(AwaitStep::Ready(v)) => {
            ctx.resume_with_awaited(dest, v);
            return;
        }
        Ok(AwaitStep::Thrown(v)) => {
            ctx.jit_native_result = v;
            ctx.jit_error_flag = 1;
            return;
        }
        Ok(AwaitStep::Entered) => {
            if !unsafe { run_entered(ctx, caller_depth) } {
                return;
            }
            None
        }
        Ok(AwaitStep::Pending(cell)) => {
            ctx.vm_suspend = Some(crate::exec::VmSuspend::Await {
                cell,
                dest_reg: dest,
            });
            Some(resume_ip)
        }
        Err(e) => {
            unsafe { super::construct::jit_propagate_error(ctx, e) };
            return;
        }
    };
    ctx.jit_panic_suspend_resume_ip = resume_at;
    let buf = ctx.jit_suspend_buf;
    if buf.is_null() {
        panic!("JIT suspend triggered but no jump buffer registered");
    }
    unsafe { crate::exec::ctx::my_longjmp(buf, 2) }
}

unsafe fn run_entered(ctx: &mut ExecCtx, caller_depth: usize) -> bool {
    let outer = std::mem::replace(&mut ctx.jit_suspend_buf, std::ptr::null_mut());
    let ran = ctx.run_until(caller_depth);
    ctx.jit_suspend_buf = outer;
    match ran {
        Ok(_) => ctx.vm_suspend.is_some(),
        Err(e) => {
            unsafe { super::construct::jit_propagate_error(ctx, e) };
            false
        }
    }
}

#[varn_macros::jit_slow(field = "yield_helper")]
pub(crate) extern "C" fn jit_yield(
    ctx: *mut ExecCtx,
    val_tag: u64,
    val_payload: u64,
    dest_reg: u32,
    resume_ip: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        let frame_idx = ctx_ref.frames.len() - 1;
        ctx_ref.frames[frame_idx].ip = resume_ip;
        ctx_ref.vm_suspend = Some(crate::exec::VmSuspend::Yield {
            value: val,
            dest_reg: dest_reg as u8,
        });
    }
}
