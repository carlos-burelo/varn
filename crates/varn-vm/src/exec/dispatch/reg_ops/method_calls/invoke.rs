//! The invocations a resolved method call ends in: a native with its
//! receiver, a VM method's activation, or the generic call of whatever the
//! property lookup produced.

use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::exec::method_args::{MethodArgs, MethodOutcome};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::VmArray;

impl ExecCtx {
    /// The generic tail of [`Self::call_method`]: lay the callee and
    /// arguments out on the stack and dispatch, for every method shape that has
    /// no shorter path.
    pub(super) fn finish_generic_method_call(
        &mut self,
        method_nv: VmValue,
        this_val: VmValue,
        args: MethodArgs<'_>,
        frame_idx: usize,
    ) -> VmResult<MethodOutcome> {
        let arg_count = args.len();
        let is_bound = method_nv.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::BoundMethod(_))
            );
        let is_static = this_val.is_heap()
            && matches!(
                self.heap.get(this_val.as_heap_idx()),
                Some(crate::heap::HeapObj::Class(_))
            );
        let is_enum_variant = method_nv.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::EnumVariant(_))
            );
        let is_plain_closure_no_this = method_nv.is_heap() && {
            if let Some(crate::heap::HeapObj::VmClosure(nc)) =
                self.heap.get(method_nv.as_heap_idx())
            {
                !nc.proto.has_this
            } else {
                false
            }
        };
        let is_namespace_native = method_nv.is_heap()
            && this_val.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::NativeFn(..))
            )
            && matches!(
                self.heap.get(this_val.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        let is_plain_native = method_nv.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::NativeFn(..))
            );

        self.stage.clear();
        self.stage.push(method_nv);

        let skip_this = is_bound
            || is_static
            || is_enum_variant
            || is_plain_closure_no_this
            || is_namespace_native
            || is_plain_native;
        if !skip_this {
            self.stage.push(this_val);
        } else {
            self.stage.push(VmValue::null());
        }
        for i in 0..arg_count {
            self.stage.push(args.get(&self.stack, i));
        }
        let effective_count = arg_count + 1;

        let prepared = self.prepare_call(method_nv, effective_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            return Ok(MethodOutcome::FramePushed);
        }
        Ok(MethodOutcome::Value(self.stage_pop()))
    }

    #[inline(always)]
    pub(crate) fn call_native_with_receiver(
        &mut self,
        f: varn_types::NativeFn,
        receiver: VmValue,
        args: MethodArgs<'_>,
    ) -> VmResult<VmValue> {
        let arg_count = args.len();
        if crate::home_trace::enabled() {
            let kind = |v: VmValue| -> &'static str {
                if v.is_sso() {
                    ":sso"
                } else if v.is_heap() {
                    match self.heap.get(v.as_heap_idx()) {
                        Some(crate::heap::HeapObj::Str(_)) => ":str",
                        Some(crate::heap::HeapObj::Array(_)) => ":array",
                        Some(crate::heap::HeapObj::Object(_)) => ":object",
                        Some(crate::heap::HeapObj::Instance(_)) => ":instance",
                        Some(_) => ":heap-other",
                        None => ":heap-none",
                    }
                } else {
                    ""
                }
            };
            let fname = self
                .frames
                .last()
                .and_then(|fr| fr.closure().proto.name.clone());
            let args: Vec<String> = (0..arg_count)
                .map(|i| {
                    let v = args.get(&self.stack, i);
                    format!("{:#x}/{:#x}{}", v.raw_tag(), v.raw_payload(), kind(v))
                })
                .collect();
            eprintln!(
                "METHODNATIVE fn={fname:?} f={:#x} recv={:#x}/{:#x}{} args={args:?}",
                f as usize,
                receiver.raw_tag(),
                receiver.raw_payload(),
                kind(receiver)
            );
        }
        let result = if arg_count < 16 {
            let mut buf = [VmValue::null(); 17];
            buf[0] = receiver;
            for i in 0..arg_count {
                buf[1 + i] = args.get(&self.stack, i);
            }
            self.invoke_native(f, &buf[..arg_count + 1])
        } else {
            let mut all = Vec::with_capacity(arg_count + 1);
            all.push(receiver);
            for i in 0..arg_count {
                all.push(args.get(&self.stack, i));
            }
            self.invoke_native(f, &all)
        }
        .map_err(RuntimeError::from)?;
        Ok(result)
    }

    /// Push the activation of VM method `nc` with `this_val` in `r0` and the
    /// arguments after it; the caller decides what to do with the frame.
    pub(crate) fn invoke_vm_method_fast(
        &mut self,
        nc: Rc<VmClosure>,
        owner_class: Option<Rc<varn_types::value::ClassObj>>,
        this_val: VmValue,
        args: MethodArgs<'_>,
        name: &str,
    ) -> VmResult<MethodOutcome> {
        let arg_count = args.len();
        self.record_call_vm_fast();
        if self.hotspot_counters.is_some() {
            let method_key = format!(
                "{}.{}",
                owner_class.as_ref().map(|c| c.name.as_str()).unwrap_or("?"),
                name
            );
            let is_jit = nc.jit_fn().is_some();
            self.record_hotspot_method(&method_key, is_jit);
        }
        // `arity` counts register 0 (the receiver here) plus the declared
        // params. Missing trailing args keep the frame defaults (DYN slots
        // read `null`, exactly as the old null-padding; static classes read
        // their zero value — only reachable when the caller under-applies,
        // which the checker rejects for required params).
        if self.frames.len() >= 10000 {
            return Err(crate::error::RuntimeError::new(
                "stack overflow: call depth exceeded 10000",
            ));
        }
        let alloc = if !nc.proto.has_rest {
            // Receiver in r0 + typed args in r1.. — one materialisation,
            // shared with `exec_call_reg`'s bound-method fast path.
            self.push_call_frame_with_this(&nc.proto, this_val, args)?
        } else {
            let alloc = self.stack.push_frame(&nc.proto);
            if let Err(e) = self.stack.unbox_into_reg(alloc, 0, this_val) {
                self.stack.pop_frame();
                return Err(e);
            }
            let nparams = nc.proto.arity.saturating_sub(1);
            let rest_idx = nparams.saturating_sub(1);
            let regular_count = arg_count.min(rest_idx);
            let mut failed: Option<crate::error::RuntimeError> = None;
            if let Err(e) = args.copy_into(&mut self.stack, alloc, 1, regular_count) {
                failed = Some(e);
            }
            if failed.is_none() {
                let rest_items: Vec<VmValue> = if arg_count > rest_idx {
                    (rest_idx..arg_count)
                        .map(|i| args.get(&self.stack, i))
                        .collect()
                } else {
                    vec![]
                };
                let rest_nv = VmValue::from_heap_idx(
                    self.heap
                        .alloc(crate::heap::HeapObj::Array(VmArray::new(rest_items))),
                );
                if let Err(e) = self.stack.unbox_into_reg(alloc, 1 + rest_idx, rest_nv) {
                    failed = Some(e);
                }
            }
            if let Some(e) = failed {
                self.stack.pop_frame();
                return Err(e);
            }
            alloc
        };
        let mut frame = crate::frame::CallFrame::new_owned(nc, alloc);
        frame.current_class = owner_class;
        self.frames.push(frame);
        Ok(MethodOutcome::FramePushed)
    }
}
