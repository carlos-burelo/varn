mod ic;
mod intrinsic;
mod invoke;

use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::exec::method_args::{MethodArgs, MethodOutcome};
use crate::heap::HeapObj;
use crate::value::VmValue;
use ic::{IcHit, IcSite};
use std::rc::Rc;
use varn_types::chunk::ICKind;
use varn_types::value::{BoundMethodTarget, ClassObj};

impl ExecCtx {
    pub(crate) fn exec_call_method_reg(
        &mut self,
        this_val: VmValue,
        base: usize,
        name_idx: usize,
        cs: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<bool> {
        let args = MethodArgs::Regs {
            base,
            start: arg_start,
            count: arg_count,
        };
        match self.call_method(this_val, name_idx, cs, args, frame_idx, closure)? {
            MethodOutcome::Value(v) => {
                self.stack.unbox_into_reg(base, dest, v)?;
                Ok(false)
            }
            MethodOutcome::FramePushed => {
                self.frames.last_mut().expect("a pushed frame").return_reg = dest as u16;
                Ok(true)
            }
        }
    }

    pub(crate) fn call_method(
        &mut self,
        this_val: VmValue,
        name_idx: usize,
        cs: usize,
        args: MethodArgs<'_>,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<MethodOutcome> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("CallMethod: non-string name"))?;

        if let Some(template) = self.enum_variant_template(this_val, name.as_ref()) {
            if let Some(built) = self.construct_enum_variant(&template, args) {
                return Ok(MethodOutcome::Value(built));
            }
        }

        if let Some(v) = self.intrinsic_method(this_val, name.as_ref(), args) {
            return Ok(MethodOutcome::Value(v));
        }

        let site = IcSite::of(closure, cs);
        let receiver_class = crate::exec::props::get_class(this_val, &self.heap);
        if let Some(cls) = &receiver_class {
            if site.usable() {
                match ic::probe(&self.heap, closure, site, cls, args.len()) {
                    Some(IcHit::Native(f)) => {
                        self.record_ic_hit_callmethod();
                        return self.call_native_method(f, this_val, args, name.as_ref());
                    }
                    Some(IcHit::Vm(nc, owner)) => {
                        self.record_ic_hit_callmethod();
                        return self.invoke_vm_method_fast(
                            nc,
                            Some(owner),
                            this_val,
                            args,
                            name.as_ref(),
                        );
                    }
                    None => self.record_ic_miss_callmethod(),
                }
            }
            if let Some(outcome) =
                self.call_class_method(cls, this_val, name.as_ref(), args, closure, site)?
            {
                return Ok(outcome);
            }
        }

        let receiver_class = receiver_class.as_ref();
        let method_nv = crate::exec::props::get_property(this_val, &name, &mut self.heap)?;

        if method_nv.is_heap() {
            if let Some(HeapObj::BoundMethod(bm)) = self.heap.get(method_nv.as_heap()) {
                match &bm.target {
                    BoundMethodTarget::Native { func, .. } => {
                        let f = *func;
                        ic::record(
                            closure,
                            site,
                            receiver_class,
                            name.as_ref(),
                            ICKind::NATIVE_VTABLE_METHOD,
                        );
                        return self.call_native_method(f, this_val, args, name.as_ref());
                    }
                    BoundMethodTarget::Vm {
                        closure: method_closure,
                        ..
                    } => {
                        if let Some(nc) = self.heap.closure_of(*method_closure) {
                            if !nc.proto.is_generator && !nc.proto.is_async {
                                ic::record(
                                    closure,
                                    site,
                                    receiver_class,
                                    name.as_ref(),
                                    ICKind::VM_VTABLE_METHOD,
                                );
                            }
                        }
                    }
                }
            }
        }

        self.finish_generic_method_call(method_nv, this_val, args, frame_idx)
    }

    fn call_class_method(
        &mut self,
        cls: &Rc<ClassObj>,
        this_val: VmValue,
        name: &str,
        args: MethodArgs<'_>,
        closure: &VmClosure,
        site: IcSite,
    ) -> VmResult<Option<MethodOutcome>> {
        let Some((method_val, owner_cls)) = varn_types::find_method_with_owner(cls, name) else {
            return Ok(None);
        };
        if let Some(nc) = self.heap.closure_of(method_val).cloned() {
            if nc.proto.is_generator || nc.proto.is_async || args.len() > nc.proto.arity {
                return Ok(None);
            }
            ic::record(closure, site, Some(cls), name, ICKind::VM_VTABLE_METHOD);
            return self
                .invoke_vm_method_fast(nc, Some(owner_cls), this_val, args, name)
                .map(Some);
        }
        if let Some((f, _)) = self.heap.native_of(method_val) {
            ic::record(closure, site, Some(cls), name, ICKind::NATIVE_VTABLE_METHOD);
            return self.call_native_method(f, this_val, args, name).map(Some);
        }
        Ok(None)
    }

    pub(crate) fn call_native_method(
        &mut self,
        f: varn_types::NativeFn,
        this_val: VmValue,
        args: MethodArgs<'_>,
        name: &str,
    ) -> VmResult<MethodOutcome> {
        self.record_call_native(f, Some(name));
        let result = self.call_native_with_receiver(f, this_val, args)?;
        Ok(MethodOutcome::Value(result))
    }
}
