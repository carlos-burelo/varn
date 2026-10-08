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
            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse | OpCode::LoadInt | OpCode::Move | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal | OpCode::DefineGlobalIdx | OpCode::LoadGlobalIdx | OpCode::StoreGlobalIdx | OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue | OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow | OpCode::Negate | OpCode::Not | OpCode::ToString | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::Ushr | OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop | OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::CallSpread | OpCode::Return | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildObject | OpCode::BuildObjectWithShape | OpCode::BuildRecord | OpCode::GetIndex | OpCode::SetIndex | OpCode::ObjectRest | OpCode::ObjectKeys | OpCode::ObjectMerge | OpCode::GetProperty | OpCode::GetPropertyMaybe | OpCode::SetProperty | OpCode::GetFixedField | OpCode::SetFixedField | OpCode::GetSuper | OpCode::GetSymbol | OpCode::MakeClosure | OpCode::DeclareLayout | OpCode::AllocInstance | OpCode::Typeof | OpCode::Instanceof | OpCode::In | OpCode::IsNull | OpCode::IsArray | OpCode::AssertNotNull | OpCode::StrConcat | OpCode::StrLength | OpCode::StrSlice | OpCode::ArrayLength | OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend | OpCode::WrapSpread | OpCode::MakeEnumVariant | OpCode::GetEnumTag | OpCode::Await | OpCode::Spawn | OpCode::Yield | OpCode::Try | OpCode::Throw | OpCode::PopTry | OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot | OpCode::InvokeRuntimeStatic | OpCode::AddImm | OpCode::SubImm | OpCode::BuildStr | OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt | OpCode::EqInt | OpCode::NeqInt | OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat | OpCode::ModFloat | OpCode::PowFloat | OpCode::LtFloat | OpCode::GtFloat | OpCode::LteFloat | OpCode::GteFloat | OpCode::EqFloat | OpCode::NeqFloat | OpCode::ModInt | OpCode::PowInt | OpCode::Intrinsic | OpCode::LoadStaticFn | OpCode::CallSelf | OpCode::Nop | OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::CallNativeOp | OpCode::IntrinsicDirect | OpCode::LoadNativeGlobalIdx | OpCode::BuildMap | OpCode::MapGetIndex | OpCode::MapSetIndex | OpCode::Convert | OpCode::BytesLength => {}
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
