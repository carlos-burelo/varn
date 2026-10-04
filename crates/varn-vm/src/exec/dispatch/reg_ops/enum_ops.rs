use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::OpCode;

/// See [`ExecCtx::enum_variant_template`].
pub(crate) struct EnumVariantTemplate {
    pub enum_class_id: Option<u32>,
    pub enum_name: Arc<str>,
    pub variant_name: Arc<str>,
    pub variant_tag: i64,
    pub fields: Vec<Arc<str>>,
}

impl ExecCtx {
    /// Build `Enum.Variant(args...)` straight from the variant template.
    ///
    /// A variant constructor reaches the VM as a method call on the enum's class
    /// object, so it used to take the generic `CallMethod` path: resolve the
    /// property (a hash lookup returning a CLONE of the template), `heap.intern`
    /// that clone into a fresh heap object, hand it to `prepare_call`, which
    /// clones the box a second time, drains the arguments into a `Vec`, and
    /// finally allocates the variant that was wanted all along. Two heap
    /// allocations, two deep clones and a `Vec` per construction — and no inline
    /// cache could shorten it, because a variant is not in the class's
    /// `method_map` and so was never cacheable.
    ///
    /// Measured on 2M constructions: 3.015 s through the generic path against
    /// 181 ms for the equivalent `new Class(i)`, a 16.6x gap.
    ///
    /// Arguments are read in place from `stack[base + arg_start ..][..arg_count]`
    /// — the same window `call_native_with_receiver` reads.
    ///
    /// Returns `None` for a fieldless variant called with no arguments: that
    /// case yields the template value itself rather than a new object, and only
    /// the generic path holds the `VmValue` identity to return.
    /// The part of a variant template that a construction needs: everything but
    /// the payload, which is rebuilt from the arguments anyway.
    ///
    /// Lifted out from under the heap borrow so the build below can take
    /// `&mut self`. All three fields are cheap to copy — two `Arc<str>` bumps and
    /// a field-name list that is empty for tuple-shaped variants.
    pub(crate) fn enum_variant_template(
        &self,
        receiver: VmValue,
        name: &str,
    ) -> Option<EnumVariantTemplate> {
        if !receiver.is_heap() {
            return None;
        }
        let crate::heap::HeapObj::Class(cls) = self.heap.get(receiver.as_heap())? else {
            return None;
        };
        let template = cls.statics.borrow().get(name).copied()?;
        if !template.is_heap() {
            return None;
        }
        let Some(crate::heap::HeapObj::EnumVariant(t)) = self.heap.get(template.as_heap()) else {
            return None;
        };
        Some(EnumVariantTemplate {
            enum_class_id: t.enum_class_id,
            enum_name: t.enum_name.clone(),
            variant_name: t.variant_name.clone(),
            variant_tag: t.variant_tag,
            fields: t.fields.clone(),
        })
    }

    pub(crate) fn construct_enum_variant(
        &mut self,
        template: &EnumVariantTemplate,
        args: crate::exec::method_args::MethodArgs<'_>,
    ) -> Option<VmValue> {
        let arg_count = args.len();
        if template.fields.is_empty() && arg_count == 0 {
            return None;
        }

        let payload = if !template.fields.is_empty() {
            let fields: Vec<(varn_types::RuntimeString, VmValue)> = template
                .fields
                .iter()
                .enumerate()
                .map(|(idx, field_name)| {
                    let nv = if idx < arg_count {
                        args.get(&self.stack, idx)
                    } else {
                        VmValue::null()
                    };
                    (field_name.clone(), nv)
                })
                .collect();
            self.heap.alloc_object_pairs(fields)
        } else if arg_count == 1 {
            args.get(&self.stack, 0)
        } else if arg_count > 1 {
            let items: Vec<VmValue> = (0..arg_count).map(|i| args.get(&self.stack, i)).collect();
            self.heap.alloc_array_vm(items)
        } else {
            VmValue::null()
        };

        let data = varn_types::value::EnumVariantData {
            enum_class_id: template.enum_class_id,
            enum_name: template.enum_name.clone(),
            variant_name: template.variant_name.clone(),
            variant_tag: template.variant_tag,
            fields: template.fields.clone(),
            payload,
        };
        Some(VmValue::from_heap(
            self.heap
                .alloc(crate::heap::HeapObj::EnumVariant(Box::new(data))),
        ))
    }

    pub(in crate::exec::dispatch) fn exec_get_enum_tag(&mut self, v: VmValue) -> VmResult<VmValue> {
        match v.is_heap().then(|| self.heap.get(v.as_heap())).flatten() {
            Some(crate::heap::HeapObj::EnumVariant(ev)) => {
                Ok(VmValue::from_i32(ev.variant_tag as i32))
            }
            _ => Ok(VmValue::from_i32(0)),
        }
    }

    pub(crate) fn exec_spawn(&mut self, task_val: VmValue) -> VmResult<VmValue> {
        Ok(task_val)
    }

    pub(in crate::exec::dispatch) fn exec_module_op_reg(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::LoadModule => {
                self.op_load_module(code, ip, closure, frame_idx, first_reg)?;
            }
            OpCode::LoadModuleSlot => {
                self.op_load_module_slot(code, ip, frame_idx, first_reg)?;
            }
            OpCode::StoreModuleSlot => {
                self.op_store_module_slot(code, ip, frame_idx, first_reg)?;
            }
            _ => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(in crate::exec::dispatch) fn exec_invoke_runtime_static_reg(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let method_idx = code[*ip] as usize;
        *ip += 1;
        let w3 = code[*ip];
        *ip += 1;
        let w4 = code[*ip];
        *ip += 1;
        let dest = (w1 >> 8) as usize;
        let arg_count = (w3 >> 8) as usize;
        let arg_start = (w3 & 0xFF) as usize;
        let end_reg = (w4 >> 8) as usize;
        let flag = w4 & 0xFF;

        let name_nv = closure.constants[method_idx];
        let name = self.heap.str_val(name_nv).ok_or_else(|| {
            RuntimeError::new(format!(
                "InvokeRuntimeStatic: const[{}] not a string",
                method_idx
            ))
        })?;

        if arg_count == 2 {
            let s = self.stack.box_reg(base, arg_start);
            let e = self.stack.box_reg(base, end_reg);
            self.stage.clear();
            self.stage.push(s);
            self.stage.push(e);
        } else {
            self.stage.clear();
            for i in 0..arg_count {
                self.stage.push(self.stack.box_reg(base, arg_start + i));
            }
        }

        let result = crate::exec::advanced::invoke_runtime_static(
            &name,
            &mut self.stage,
            &mut self.heap,
            flag,
        )?;
        self.stage.clear();
        self.stack.unbox_into_reg(base, dest, result)?;
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }
}
