use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::OpCode;
use varn_types::Value;

/// See [`ExecCtx::enum_variant_template`].
pub(crate) struct EnumVariantTemplate {
    pub enum_name: Rc<str>,
    pub variant_name: Rc<str>,
    pub variant_tag: i64,
    pub fields: Vec<Rc<str>>,
}

impl ExecCtx {
    pub(in crate::exec::dispatch) fn exec_class_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::MakeClass => {
                let super_reg = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let dest = first_reg;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("MakeClass: non-string const"))?;
                let cls = crate::exec::class::op_class(&name, &mut self.heap);
                self.stack[base + dest] = cls;
                if super_reg != 0 {
                    let super_nv = self.stack[base + super_reg];
                    crate::exec::class::op_inherit(cls, super_nv, &mut self.heap)?;
                }
            }
            OpCode::Inherit => {
                let w1 = code[*ip];
                *ip += 1;
                let (class_reg, super_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let super_nv = self.stack[base + super_reg];
                crate::exec::class::op_inherit(class_nv, super_nv, &mut self.heap)?;
            }
            OpCode::Method => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("Method: non-string const"))?;
                crate::exec::class::op_method(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineStatic => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineStatic: non-string const"))?;
                crate::exec::class::op_define_static(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineGetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineGetter: non-string const"))?;
                crate::exec::class::op_define_getter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineSetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineSetter: non-string const"))?;
                crate::exec::class::op_define_setter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineStaticGetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineStaticGetter: non-string const"))?;
                crate::exec::class::op_define_static_getter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineStaticSetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineStaticSetter: non-string const"))?;
                crate::exec::class::op_define_static_setter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DeclareField => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let class_reg = (w1 >> 8) as usize;
                let class_nv = self.stack[base + class_reg];
                let key_nv = closure.constants[name_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DeclareField: non-string const"))?;
                crate::exec::class::op_declare_field(class_nv, &key, &mut self.heap)?;
            }
            OpCode::BindMethod => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let (dest, obj_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let obj_nv = self.stack[base + obj_reg];
                let key_nv = closure.constants[name_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("BindMethod: non-string const"))?;
                let method = crate::exec::props::get_property(obj_nv, &key, &mut self.heap)?;

                let receiver = self.heap.extract(obj_nv);
                let method_val = self.heap.extract(method);
                let bound =
                    varn_types::Value::BoundMethod(Box::new(varn_types::value::BoundMethod {
                        receiver,
                        target: match method_val {
                            varn_types::Value::NativeFn(b) => {
                                varn_types::value::BoundMethodTarget::Native {
                                    func: b.0,
                                    name: b.1,
                                }
                            }
                            varn_types::Value::VmValue(payload) => {
                                varn_types::value::BoundMethodTarget::Vm {
                                    closure: payload,
                                    owner_class: None,
                                }
                            }
                            _ => {
                                return Err(RuntimeError::new("BindMethod: method is not callable"))
                            }
                        },
                    }));
                self.stack[base + dest] = self.heap.intern(bound);
            }
            _ => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(in crate::exec::dispatch) fn exec_make_enum_variant_reg(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let name_idx = code[*ip] as usize;
        *ip += 1;
        let (dest, tag_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("MakeEnumVariant: non-string const"))?;
        let tag = self.stack[base + tag_reg].as_int();

        let name_str = name.as_ref();
        let (name_part, fields_part) = match name_str.find(':') {
            Some(idx) => (&name_str[..idx], &name_str[idx + 1..]),
            None => (name_str, ""),
        };
        let (enum_name_str, variant_name_str) = match name_part.rfind('.') {
            Some(idx) => (&name_part[..idx], &name_part[idx + 1..]),
            None => ("", name_part),
        };
        let fields: Vec<Rc<str>> = if fields_part.is_empty() {
            vec![]
        } else {
            fields_part.split(',').map(Rc::from).collect()
        };

        let variant =
            varn_types::Value::EnumVariant(Box::new(varn_types::value::EnumVariantData {
                enum_name: Rc::from(enum_name_str),
                variant_name: Rc::from(variant_name_str),
                variant_tag: tag,
                fields,
                payload: varn_types::Value::Object(varn_types::value::ObjRef::empty()),
            }));
        self.stack[base + dest] = self.heap.intern(variant);
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

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
    /// `&mut self`. All three fields are cheap to copy — two `Rc<str>` bumps and
    /// a field-name list that is empty for tuple-shaped variants.
    pub(crate) fn enum_variant_template(
        &self,
        receiver: VmValue,
        name: &str,
    ) -> Option<EnumVariantTemplate> {
        if !receiver.is_heap() {
            return None;
        }
        let crate::heap::HeapObj::Class(cls) = self.heap.get(receiver.as_heap_idx())? else {
            return None;
        };
        let statics = cls.statics.borrow();
        let Some(Value::EnumVariant(t)) = statics.get(name) else {
            return None;
        };
        Some(EnumVariantTemplate {
            enum_name: t.enum_name.clone(),
            variant_name: t.variant_name.clone(),
            variant_tag: t.variant_tag,
            fields: t.fields.clone(),
        })
    }

    pub(crate) fn construct_enum_variant(
        &mut self,
        template: &EnumVariantTemplate,
        base: usize,
        arg_start: usize,
        arg_count: usize,
    ) -> Option<VmValue> {
        if template.fields.is_empty() && arg_count == 0 {
            return None;
        }

        let payload = if !template.fields.is_empty() {
            Value::Object(varn_types::value::ObjRef::from_pairs(
                template.fields.iter().enumerate().map(|(idx, field_name)| {
                    let nv = if idx < arg_count {
                        self.stack[base + arg_start + idx]
                    } else {
                        VmValue::null()
                    };
                    (field_name.clone(), nv)
                }),
            ))
        } else if arg_count == 1 {
            let arg = self.stack[base + arg_start];
            self.heap.extract(arg)
        } else if arg_count > 1 {
            Value::Array(varn_types::value::ArrayRef::new(
                (0..arg_count)
                    .map(|i| {
                        let arg = self.stack[base + arg_start + i];
                        self.heap.extract(arg)
                    })
                    .collect(),
            ))
        } else {
            Value::Null
        };

        let data = varn_types::value::EnumVariantData {
            enum_name: template.enum_name.clone(),
            variant_name: template.variant_name.clone(),
            variant_tag: template.variant_tag,
            fields: template.fields.clone(),
            payload,
        };
        Some(VmValue::from_heap_idx(
            self.heap
                .alloc(crate::heap::HeapObj::EnumVariant(Box::new(data))),
        ))
    }

    pub(in crate::exec::dispatch) fn exec_get_enum_tag(&mut self, v: VmValue) -> VmResult<VmValue> {
        let val = self.heap.extract(v);
        match val {
            Value::EnumVariant(ev) => Ok(VmValue::from_i32(ev.variant_tag as i32)),
            _ => Ok(VmValue::from_i32(0)),
        }
    }

    pub(crate) fn exec_spawn(&mut self, task_val: VmValue) -> VmResult<VmValue> {
        let task = self.heap.extract(task_val);
        Ok(self.heap.intern(task))
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
        let flag = (w4 & 0xFF) as u16;

        let name_nv = closure.constants[method_idx];
        let name = self.heap.str_val(name_nv).ok_or_else(|| {
            RuntimeError::new(format!(
                "InvokeRuntimeStatic: const[{}] not a string",
                method_idx
            ))
        })?;

        if arg_count == 2 {
            let s = self.stack[base + arg_start];
            let e = self.stack[base + end_reg];
            self.stack.push(s);
            self.stack.push(e);
        } else {
            for i in 0..arg_count {
                let v = self.stack[base + arg_start + i];
                self.stack.push(v);
            }
        }

        let result = crate::exec::advanced::invoke_runtime_static(
            &name,
            &mut self.stack,
            &mut self.heap,
            flag,
        )?;
        self.stack[base + dest] = result;
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }
}
