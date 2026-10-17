use super::calls::PreparedCall;
use super::ctx::ExecCtx;
use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;

impl ExecCtx {
    pub(crate) fn build_generator(
        &mut self,
        closure: std::rc::Rc<crate::closure::VmClosure>,
        args: Vec<VmValue>,
        current_class: Option<std::rc::Rc<varn_types::ClassObj>>,
    ) -> VmValue {
        let state = crate::generator::GenState::new(closure, args, current_class);
        VmValue::from_heap(self.heap.alloc(crate::heap::HeapObj::Generator(state)))
    }
}

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

pub fn unwind_to_handler(
    ctx: &mut ExecCtx,
    handler: crate::frame::TryHandler,
    thrown: VmValue,
) -> VmResult<()> {
    while ctx.frames.len() > handler.frame_depth {
        let f = ctx.frames.pop().unwrap();
        ctx.drop_frame_storage(f.base);
    }

    let target = ctx.frames.len() - 1;
    let base = ctx.frames[target].base;
    let nregs = ctx.frames[target].closure().proto.register_count as usize;
    ctx.stack.ensure_frame_size(base, nregs);

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

                self.stage.clear();
                self.stage.push(value);
            }
            PreparedCall::Frame(frame) => {
                if self.frames.len() >= crate::frame::MAX_CALL_DEPTH {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();

                self.frames.push(frame);

                if self.heap.needs_minor_gc() {
                    self.run_minor_gc();
                }

                if self.heap.needs_gc() {
                    self.trigger_gc();
                }
            }
            PreparedCall::Constructor(frame, instance_nv) => {
                if self.frames.len() >= crate::frame::MAX_CALL_DEPTH {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();
                let ctor_frame_idx = self.frames.len();
                self.frames.push(frame);
                self.pending_constructors
                    .push((ctor_frame_idx, instance_nv));
            }
            PreparedCall::NativeImmediate(f, arg_count) => {
                self.record_call_native(f, None);

                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let args: Vec<VmValue> = self.stage.drain(start..).collect();
                let result = if args.len() <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    buf[..args.len()].copy_from_slice(&args);
                    self.invoke_native(f, &buf[..args.len()])
                } else {
                    self.invoke_native(f, &args)
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
                let result = self.invoke_native(f, slice).map_err(RuntimeError::from)?;

                self.stage.clear();
                self.stage.push(result);
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                self.record_call_native(f, None);
                let result = self.invoke_native(f, &args).map_err(RuntimeError::from)?;
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
            PreparedCall::Async(parts) => {
                let nv = crate::task::new_lazy(&mut self.heap, parts);
                self.stage.clear();
                self.stage.push(nv);
            }
        }
        Ok(())
    }
}
