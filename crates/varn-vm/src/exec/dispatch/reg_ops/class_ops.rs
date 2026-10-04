use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

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
                self.stack.unbox_into_reg(base, dest, cls)?;
                if super_reg != 0 {
                    let super_nv = self.stack.box_reg(base, super_reg);
                    crate::exec::class::op_inherit(cls, super_nv, &mut self.heap)?;
                }
            }
            OpCode::Inherit => {
                let w1 = code[*ip];
                *ip += 1;
                let (class_reg, super_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack.box_reg(base, class_reg);
                let super_nv = self.stack.box_reg(base, super_reg);
                crate::exec::class::op_inherit(class_nv, super_nv, &mut self.heap)?;
            }
            OpCode::Method => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack.box_reg(base, class_reg);
                let fn_nv = self.stack.box_reg(base, fn_reg);
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
                let class_nv = self.stack.box_reg(base, class_reg);
                let fn_nv = self.stack.box_reg(base, fn_reg);
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
                let class_nv = self.stack.box_reg(base, class_reg);
                let fn_nv = self.stack.box_reg(base, fn_reg);
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
                let class_nv = self.stack.box_reg(base, class_reg);
                let fn_nv = self.stack.box_reg(base, fn_reg);
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
                let class_nv = self.stack.box_reg(base, class_reg);
                let fn_nv = self.stack.box_reg(base, fn_reg);
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
                let class_nv = self.stack.box_reg(base, class_reg);
                let fn_nv = self.stack.box_reg(base, fn_reg);
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
                let class_nv = self.stack.box_reg(base, class_reg);
                let key_nv = closure.constants[name_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DeclareField: non-string const"))?;
                let tag = varn_core::RuntimeKind::from_u8((w1 & 0xFF) as u8);
                crate::exec::class::op_declare_field(class_nv, &key, tag, &mut self.heap)?;
            }
            OpCode::BindMethod => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let (dest, obj_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let obj_nv = self.stack.box_reg(base, obj_reg);
                let key_nv = closure.constants[name_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("BindMethod: non-string const"))?;
                let method = crate::exec::props::get_property(obj_nv, &key, &mut self.heap)?;

                let target = if let Some((func, name)) = self.heap.native_of(method) {
                    varn_types::value::BoundMethodTarget::Native { func, name }
                } else if self.heap.closure_of(method).is_some() {
                    varn_types::value::BoundMethodTarget::Vm {
                        closure: method,
                        owner_class: None,
                    }
                } else {
                    return Err(RuntimeError::new("BindMethod: method is not callable"));
                };
                let bound = varn_types::value::BoundMethod {
                    receiver: obj_nv,
                    target,
                };
                let bound_nv = VmValue::from_heap(
                    self.heap
                        .alloc(crate::heap::HeapObj::BoundMethod(Box::new(bound))),
                );
                self.stack.unbox_into_reg(base, dest, bound_nv)?;
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
        let tag = self.stack.box_reg(base, tag_reg).as_int();
        let iv = self.make_enum_variant(tag, name.as_ref());
        self.stack.unbox_into_reg(base, dest, iv)?;
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }
}
