//! Call and return sequencing for interpreter frames.
//!
//! `dispatch_prepared_call` is the one place a prepared call becomes a live
//! frame — VM closure, native fn, bound method and constructor all converge
//! here so the frame protocol is written once.

use super::calls::PreparedCall;
use super::ctx::ExecCtx;
use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;
use varn_types::NativeCtx;

/// What a constructor frame's return actually yields.
///
/// A `constructor` returns the instance, not whatever its body returned —
/// unless the body returned something non-null, which overrides it. The
/// pending instance is found by frame index rather than by position, because
/// a constructor can be re-entered (a ctor calling a ctor) and only the entry
/// belonging to THIS frame may be consumed.
///
/// This is a language rule, not a tier detail, and it has THREE callers: the
/// interpreter's `Return`, the exit of a compiled frame, and the JIT→JIT call
/// fast path. It lived inline in all three, byte for byte. Changing it in one
/// place and not the others is a tier-divergence bug — the interpreter and the
/// compiled code would disagree about what `new X()` evaluates to — so it is
/// written once here, where return sequencing lives, and called from all of
/// them.
pub(crate) fn resolve_constructor_return(
    ctx: &mut ExecCtx,
    returning_frame_idx: usize,
    val: VmValue,
) -> VmValue {
    if ctx.pending_constructors.is_empty() {
        return val;
    }
    let ctor_pos = ctx
        .pending_constructors
        .iter()
        .rposition(|(idx, _)| *idx == returning_frame_idx);

    match ctor_pos {
        Some(pos) => {
            let (_, instance_nv) = ctx.pending_constructors.remove(pos);
            if val.is_null() {
                instance_nv
            } else {
                val
            }
        }
        None => val,
    }
}

/// Unwind to `handler` and leave its frame ready to run the catch block.
///
/// Pops frames down to the handler's depth (closing each one's upvalues),
/// truncates the stack to the receiving frame's register window, writes
/// `thrown` into the handler's error register, and points that frame's ip at
/// the catch block.
///
/// `handler` is taken BY VALUE because every caller has already removed it from
/// wherever it lived — `try_handlers` for the interpreter and task paths,
/// `jit_panic_exception_handler` for compiled code, which `jit_propagate_error`
/// pops on the way out. Taking it by value is what makes that non-negotiable.
///
/// Like [`resolve_constructor_return`], this is a language rule rather than a
/// tier detail, and it had three byte-identical copies: the interpreter's
/// `Throw`, the exit of a compiled frame, and the task fork in `ctx_tasks`. A
/// `catch` that behaves differently depending on which tier the frame came from
/// is a bug no single-tier test can find, and exception handling is precisely
/// what a user relies on to reason about their program.
pub(crate) fn unwind_to_handler(
    ctx: &mut ExecCtx,
    handler: crate::frame::TryHandler,
    thrown: VmValue,
) {
    while ctx.frames.len() > handler.frame_depth {
        ctx.record_frame_pop();
        let f = ctx.frames.pop().unwrap();
        ctx.close_upvalues_above(f.base);
    }

    // The handler's own frame is now on top. Everything above its register
    // window is dead: the frames that owned it are gone.
    let target = ctx.frames.len() - 1;
    let base = ctx.frames[target].base;
    let required_depth = base + ctx.frames[target].closure().proto.register_count as usize;
    ctx.stack.truncate(required_depth);

    // `truncate` can leave the stack SHORTER than the error slot when the
    // handler's frame declares fewer registers than its err_reg index — grow
    // rather than index out of bounds.
    let slot = base + handler.err_reg as usize;
    if slot >= ctx.stack.len() {
        ctx.stack.resize(slot + 1, VmValue::null());
    }
    ctx.stack[slot] = thrown;

    ctx.frames[target].ip = handler.catch_ip;
}

impl ExecCtx {
    pub(crate) fn dispatch_prepared_call(&mut self, call: PreparedCall) -> VmResult<()> {
        match call {
            PreparedCall::Frame(frame) => {
                if self.frames.len() >= 10000 {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.record_frame_push();
                self.frames.push(frame);

                if !self.gc_inhibited && self.heap.needs_minor_gc() {
                    self.run_minor_gc();
                }

                if !self.gc_inhibited && self.heap.needs_gc() {
                    let mut roots: Vec<u32> = Vec::with_capacity(256);
                    for v in &self.stack {
                        if v.is_heap() {
                            roots.push(v.as_heap_idx());
                        }
                    }
                    for v in &self.globals.values {
                        if v.is_heap() {
                            roots.push(v.as_heap_idx());
                        }
                    }

                    for frame in &self.frames {
                        for c in frame.closure().constants.iter() {
                            if c.is_heap() {
                                roots.push(c.as_heap_idx());
                            }
                        }
                    }
                    for (_k, v) in &self.modules {
                        if v.is_heap() {
                            roots.push(v.as_heap_idx());
                        }
                    }
                    let _ = self.heap.collect(&roots);
                }
            }
            PreparedCall::Constructor(frame, instance_nv) => {
                if self.frames.len() >= 10000 {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();
                let ctor_frame_idx = self.frames.len();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.record_frame_push();
                self.frames.push(frame);
                self.pending_constructors
                    .push((ctor_frame_idx, instance_nv));
                if self.jit_frame_prepushed != 0 {
                    let _ = self.run_until(ctor_frame_idx)?;
                }
            }
            PreparedCall::Native(f, args) => {
                self.record_call_native();
                let result =
                    (f)(self as &mut dyn NativeCtx, &args).map_err(|e| RuntimeError::new(e))?;
                self.stack.pop();
                self.push(result);
            }
            PreparedCall::NativeImmediate(f, arg_count) => {
                self.record_call_native();
                let args_start = self.stack.len() - arg_count;

                let result = if arg_count <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    buf[..arg_count]
                        .copy_from_slice(&self.stack[args_start..args_start + arg_count]);
                    self.stack.truncate(args_start - 1);
                    (f)(self as &mut dyn NativeCtx, &buf[..arg_count])
                } else {
                    let vm_args: Vec<VmValue> =
                        self.stack[args_start..args_start + arg_count].to_vec();
                    self.stack.truncate(args_start - 1);
                    (f)(self as &mut dyn NativeCtx, &vm_args)
                }
                .map_err(|e| RuntimeError::new(e))?;

                self.push(result);
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                self.record_call_native();
                let args_start = self.stack.len() - arg_count;

                let result = if arg_count <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    buf[..arg_count]
                        .copy_from_slice(&self.stack[args_start..args_start + arg_count]);
                    self.stack.truncate(args_start - 1);
                    let slice = if arg_count > 0 {
                        &buf[1..arg_count]
                    } else {
                        &buf[..0]
                    };
                    (f)(self as &mut dyn NativeCtx, slice)
                } else {
                    let vm_args: Vec<VmValue> =
                        self.stack[args_start..args_start + arg_count].to_vec();
                    self.stack.truncate(args_start - 1);
                    let slice = if arg_count > 0 {
                        &vm_args[1..]
                    } else {
                        &vm_args[..]
                    };
                    (f)(self as &mut dyn NativeCtx, slice)
                }
                .map_err(|e| RuntimeError::new(e))?;

                self.push(result);
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                self.record_call_native();
                let result =
                    (f)(self as &mut dyn NativeCtx, &args).map_err(|e| RuntimeError::new(e))?;
                self.stack.pop();
                let nv = if result.is_null() {
                    instance_nv
                } else {
                    result
                };
                self.push(nv);
            }
            PreparedCall::PushValue(nv) => {
                self.stack.pop();
                self.push(nv);
            }
        }
        Ok(())
    }
}
