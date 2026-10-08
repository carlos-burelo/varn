use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::OpCode;

pub(crate) struct EnumVariantTemplate {
    pub enum_class_id: Option<u32>,
    pub enum_name: Arc<str>,
    pub variant_name: Arc<str>,
    pub variant_tag: i64,
    pub fields: Vec<Arc<str>>,
}

impl ExecCtx {
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
            Some(crate::heap::HeapObj::Str(_) | crate::heap::HeapObj::Array(_) | crate::heap::HeapObj::Tuple(_) | crate::heap::HeapObj::Object(_) | crate::heap::HeapObj::Record(_) | crate::heap::HeapObj::Buffer(_) | crate::heap::HeapObj::Module(_) | crate::heap::HeapObj::FrozenModule(_) | crate::heap::HeapObj::VmClosure(_) | crate::heap::HeapObj::Class(_) | crate::heap::HeapObj::NativeFn(..) | crate::heap::HeapObj::BoundMethod(_) | crate::heap::HeapObj::Map(_) | crate::heap::HeapObj::Set(_) | crate::heap::HeapObj::Task(_) | crate::heap::HeapObj::TaskHandle(_) | crate::heap::HeapObj::Range(_) | crate::heap::HeapObj::Symbol(_) | crate::heap::HeapObj::BigInt(_) | crate::heap::HeapObj::Decimal(_) | crate::heap::HeapObj::Char(_) | crate::heap::HeapObj::Generator(_) | crate::heap::HeapObj::Spread(_)) | None => Ok(VmValue::from_i32(0)),
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
            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse | OpCode::LoadInt | OpCode::Move | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal | OpCode::DefineGlobalIdx | OpCode::LoadGlobalIdx | OpCode::StoreGlobalIdx | OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue | OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow | OpCode::Negate | OpCode::Not | OpCode::ToString | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::Ushr | OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop | OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::CallSpread | OpCode::Return | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildObject | OpCode::BuildObjectWithShape | OpCode::BuildRecord | OpCode::GetIndex | OpCode::SetIndex | OpCode::ObjectRest | OpCode::ObjectKeys | OpCode::ObjectMerge | OpCode::GetProperty | OpCode::GetPropertyMaybe | OpCode::SetProperty | OpCode::GetFixedField | OpCode::SetFixedField | OpCode::GetSuper | OpCode::GetSymbol | OpCode::MakeClosure | OpCode::MakeClass | OpCode::Inherit | OpCode::Method | OpCode::DefineStatic | OpCode::DefineGetter | OpCode::DefineSetter | OpCode::DefineStaticGetter | OpCode::DefineStaticSetter | OpCode::DeclareLayout | OpCode::AllocInstance | OpCode::BindMethod | OpCode::Typeof | OpCode::Instanceof | OpCode::In | OpCode::IsNull | OpCode::IsArray | OpCode::AssertNotNull | OpCode::StrConcat | OpCode::StrLength | OpCode::StrSlice | OpCode::ArrayLength | OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend | OpCode::WrapSpread | OpCode::MakeEnumVariant | OpCode::GetEnumTag | OpCode::Await | OpCode::Spawn | OpCode::Yield | OpCode::Try | OpCode::Throw | OpCode::PopTry | OpCode::InvokeRuntimeStatic | OpCode::AddImm | OpCode::SubImm | OpCode::BuildStr | OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt | OpCode::EqInt | OpCode::NeqInt | OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat | OpCode::ModFloat | OpCode::PowFloat | OpCode::LtFloat | OpCode::GtFloat | OpCode::LteFloat | OpCode::GteFloat | OpCode::EqFloat | OpCode::NeqFloat | OpCode::ModInt | OpCode::PowInt | OpCode::Intrinsic | OpCode::LoadStaticFn | OpCode::CallSelf | OpCode::Nop | OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::CallNativeOp | OpCode::IntrinsicDirect | OpCode::LoadNativeGlobalIdx | OpCode::BuildMap | OpCode::MapGetIndex | OpCode::MapSetIndex | OpCode::Convert | OpCode::BytesLength => {}
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
