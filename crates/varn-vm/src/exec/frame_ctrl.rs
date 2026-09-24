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

impl ExecCtx {
    /// Give a generator body its own execution context.
    ///
    /// The context is a FORK of this one, and the globals are the reason.
    /// `crate::globals::resolve` rewrites every global access into
    /// `LoadGlobalIdx <slot>`, and those slot indices belong to exactly one
    /// `GlobalStore`. The generator's proto was resolved against THIS
    /// context's store; handing the body a fresh empty one made every such
    /// load read past the end of an empty vector. The symptom was
    /// `value is not callable: 0` for any global function the inliner had not
    /// already folded into the generator body — which is why `yield f()`
    /// worked for a small `f` and died for a large one, an `async` one, or
    /// another generator.
    ///
    /// Forking also carries `modules`, `precompiled`, `loader` and the JIT
    /// linker, so a generator body reaches the same code the caller does. It
    /// is the same move `fork_for_task` already makes for an async task.
    ///
    /// The store is cloned, not shared: `GlobalStore::clone` deliberately
    /// keeps its id, so indices handed out before the clone still name the
    /// same globals. A global DEFINED after the generator was created is
    /// therefore not visible to it, and one the body defines does not escape —
    /// the same boundary an async task already has.
    pub(crate) fn build_generator(
        &mut self,
        closure: std::rc::Rc<crate::closure::VmClosure>,
        args: Vec<VmValue>,
        current_class: Option<std::rc::Rc<varn_types::ClassObj>>,
    ) -> VmValue {
        let mut gen_ctx = Box::new(self.fork_for_task());
        gen_ctx.gc_inhibited = true;
        // El frame 0 del generador adopta los args preparados (el resto se
        // ignora: son duplicados nunca leídos, igual que el trailing que el
        // protocolo anterior dejaba en el `Vec`).
        let alloc = gen_ctx.stack.push_frame(&closure.proto);
        let nregs = closure.proto.register_count as usize;
        gen_ctx.stack.adopt_values(alloc, 0, &args, nregs);
        let is_async = closure.proto.is_async;
        let mut frame = crate::frame::CallFrame::new_owned(closure, alloc);
        frame.current_class = current_class;
        gen_ctx.frames.push(frame);

        let driver = crate::generator::NanGenDriver::new(gen_ctx, is_async);
        self.heap.intern(varn_types::value::Value::Generator(
            varn_types::generator::GeneratorObj(driver),
        ))
    }
}

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
pub fn unwind_to_handler(
    ctx: &mut ExecCtx,
    handler: crate::frame::TryHandler,
    thrown: VmValue,
) -> VmResult<()> {
    // Cerrar ANTES de liberar: el close lee el valor vivo del slot.
    while ctx.frames.len() > handler.frame_depth {
        let f = ctx.frames.pop().unwrap();
        ctx.close_upvalues_in(f.base);
        ctx.stack.pop_frame();
    }

    // El frame receptor queda arriba con su región intacta (el pop solo
    // recorta lo superior). Asegura tamaño por si `err_reg` cae en trailing.
    let target = ctx.frames.len() - 1;
    let base = ctx.frames[target].base;
    let nregs = ctx.frames[target].closure().proto.register_count as usize;
    ctx.stack.ensure_frame_size(base, nregs);
    // `err_reg` lo emite el compilador como `Dynamic` (`CatchParam`), así que
    // esto no falla en programas bien tipados; si falla, el error prosigue
    // como un throw más (el `?` lo deja en manos del siguiente handler).
    ctx.stack
        .unbox_into_reg(base, handler.err_reg as usize, thrown)?;

    ctx.frames[target].ip = handler.catch_ip;
    Ok(())
}

impl ExecCtx {
    pub(crate) fn dispatch_prepared_call(&mut self, call: PreparedCall) -> VmResult<()> {
        match call {
            PreparedCall::Generator {
                closure,
                args,
                current_class,
            } => {
                let value = self.build_generator(closure, args, current_class);
                // `describe_generator` pudo dejar el callee en staging
                // (camino `CallSelf`): se descarta, el resultado manda.
                self.stage.clear();
                self.stage.push(value);
            }
            PreparedCall::Frame(frame) => {
                if self.frames.len() >= 10000 {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();
                // El frame ya trae su región tipada (`materialize_frame`):
                // solo entra en la lista. Sin `resize`: el almacén dimensiona
                // exacto y `pop_frame` restaura las cimas al retornar.
                self.frames.push(frame);

                if !self.gc_inhibited && self.heap.needs_minor_gc() {
                    self.run_minor_gc();
                }

                if !self.gc_inhibited && self.heap.needs_gc() {
                    let roots = self.major_roots();
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
                self.frames.push(frame);
                self.pending_constructors
                    .push((ctor_frame_idx, instance_nv));
                if self.jit_frame_prepushed != 0 {
                    let _ = self.run_until(ctor_frame_idx)?;
                }
            }
            PreparedCall::NativeImmediate(f, arg_count) => {
                self.record_call_native(f, None);
                // Staging trae la ventana al FINAL del buffer (los caminos de
                // método anteponen el `method_nv` fuera de la ventana): se
                // consumen los últimos `arg_count`, como el `stack.len() - n`
                // anterior, a un buffer propio ANTES de invocar (reentrancia
                // host→VM).
                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let args: Vec<VmValue> = self.stage.drain(start..).collect();
                let result = if args.len() <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    buf[..args.len()].copy_from_slice(&args);
                    (f)(self as &mut dyn NativeCtx, &buf[..args.len()])
                } else {
                    (f)(self as &mut dyn NativeCtx, &args)
                }
                .map_err(RuntimeError::from)?;

                self.stage.clear();
                self.stage.push(result);
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                self.record_call_native(f, None);
                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let args: Vec<VmValue> = self.stage.drain(start..).collect();
                let slice = if args.len() > 1 { &args[1..] } else { &[] };
                let result = (f)(self as &mut dyn NativeCtx, slice).map_err(RuntimeError::from)?;

                self.stage.clear();
                self.stage.push(result);
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                self.record_call_native(f, None);
                let result = (f)(self as &mut dyn NativeCtx, &args).map_err(RuntimeError::from)?;
                let nv = if result.is_null() {
                    instance_nv
                } else {
                    result
                };
                self.stage.clear();
                self.stage.push(nv);
            }
            PreparedCall::PushValue(nv) => {
                self.stage.clear();
                self.stage.push(nv);
            }
        }
        Ok(())
    }
}
