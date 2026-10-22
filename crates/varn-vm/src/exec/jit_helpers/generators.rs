use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::generator::Entry;
use crate::value::VmValue;

#[varn_macros::jit_slow(field = "generator_enter")]
pub(crate) extern "C" fn jit_generator_enter(
    ctx: *mut ExecCtx,
    gen_tag: u64,
    gen_payload: u64,
    input_tag: u64,
    input_payload: u64,
    out: *mut usize,
) -> usize {
    unsafe {
        let ctx_ref = &mut *ctx;
        let receiver = VmValue::from_raw_parts(gen_tag, gen_payload);
        let input = VmValue::from_raw_parts(input_tag, input_payload);
        let Some(state) = ctx_ref.heap.generator_of(receiver) else {
            jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new("generator.next: invalid receiver"),
            );
            return 0;
        };
        let result = if state.is_async {
            Ok(ctx_ref.step_generator_task(&state))
        } else {
            match ctx_ref.enter_generator(&state, input) {
                Ok(Entry::Done(v)) => Ok(v),
                Ok(Entry::Mounted(depth)) => match ctx_ref.generator_entry(depth) {
                    Some(entry) => {
                        let frame = &ctx_ref.frames[depth];
                        out.write(frame.closure_ptr as usize);
                        out.add(1).write(frame.base);
                        ctx_ref.entered_generators.push(state);
                        return entry;
                    }
                    None => ctx_ref.run_generator(&state, depth),
                },
                Err(e) => Err(e),
            }
        };
        match result {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
        0
    }
}

#[varn_macros::jit_slow(field = "generator_leave")]
pub(crate) extern "C" fn jit_generator_leave(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let state = ctx_ref
            .entered_generators
            .pop()
            .expect("a generator entered by jit_generator_enter");
        let depth = ctx_ref.frames.len() - 1;
        if ctx_ref.jit_error_flag != 0 {
            ctx_ref.vm_suspend = None;
            ctx_ref.abandon_generator(&state, depth);
            return;
        }
        match ctx_ref.leave_generator(&state, depth) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
