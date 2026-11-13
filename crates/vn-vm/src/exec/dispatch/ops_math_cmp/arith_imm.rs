use super::super::{hi, lo};
use super::int_overflow;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use vn_core::OpCode;

macro_rules! imm_compare {
    ($ctx:ident, $base:ident, $src:ident, $dest:ident, $imm:ident, $int_cmp:tt, $heap_cmp:path) => {{
        let verdict = if $ctx.stack.reg_class($base, $src) == crate::frame_store::SlotClass::Gpr {
            $ctx.stack.g($base, $src) $int_cmp $imm
        } else {
            let v = $ctx.stack.box_reg($base, $src);
            if $ctx.heap.is_int(v) {
                $ctx.heap.as_int(v) $int_cmp $imm
            } else {
                $heap_cmp(v, VmValue::from_int($imm), &$ctx.heap)
            }
        };
        $ctx.stack
            .unbox_into_reg($base, $dest, VmValue::from_bool(verdict))?;
    }};
}

impl ExecCtx {
    pub(super) fn exec_imm(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let src = hi(w1);
        let imm = lo(w1) as i8 as i64;
        match op {
            OpCode::AddImm => {
                if self.stack.reg_class(base, src) == crate::frame_store::SlotClass::Gpr {
                    let a_val = self.stack.g(base, src);
                    match vn_core::add_int(a_val, imm) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("+", a_val, imm)),
                    }
                } else {
                    let v = self.stack.box_reg(base, src);
                    if self.heap.is_int(v) {
                        let a_val = self.heap.as_int(v);
                        match vn_core::add_int(a_val, imm) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("+", a_val, imm)),
                        };
                    } else {
                        let imm_v = VmValue::from_int(imm);
                        let r = crate::exec::arith::add(v, imm_v, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    }
                }
            }
            OpCode::SubImm => {
                if self.stack.reg_class(base, src) == crate::frame_store::SlotClass::Gpr {
                    let a_val = self.stack.g(base, src);
                    match vn_core::sub_int(a_val, imm) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("-", a_val, imm)),
                    }
                } else {
                    let v = self.stack.box_reg(base, src);
                    if self.heap.is_int(v) {
                        let a_val = self.heap.as_int(v);
                        match vn_core::sub_int(a_val, imm) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("-", a_val, imm)),
                        };
                    } else {
                        let imm_v = VmValue::from_int(imm);
                        let r = crate::exec::arith::sub(v, imm_v, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    }
                }
            }
            OpCode::LtImm => {
                imm_compare!(self, base, src, first_reg, imm, <, crate::exec::compare::lt_heap)
            }
            OpCode::LteImm => {
                imm_compare!(self, base, src, first_reg, imm, <=, crate::exec::compare::lte_heap)
            }
            OpCode::GtImm => {
                imm_compare!(self, base, src, first_reg, imm, >, crate::exec::compare::gt_heap)
            }
            OpCode::GteImm => {
                imm_compare!(self, base, src, first_reg, imm, >=, crate::exec::compare::gte_heap)
            }
            OpCode::EqImm => {
                imm_compare!(self, base, src, first_reg, imm, ==, crate::exec::compare::eq)
            }
            OpCode::NeqImm => {
                imm_compare!(self, base, src, first_reg, imm, !=, crate::exec::compare::neq)
            }
            OpCode::LoadConst
            | OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadInt
            | OpCode::Move
            | OpCode::LoadGlobal
            | OpCode::StoreGlobal
            | OpCode::DefineGlobal
            | OpCode::DefineGlobalIdx
            | OpCode::LoadGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::CloseUpvalue
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Negate
            | OpCode::Not
            | OpCode::ToString
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Loop
            | OpCode::Call
            | OpCode::CallMethod
            | OpCode::InvokeVirtual
            | OpCode::CallSpread
            | OpCode::Return
            | OpCode::BuildArray
            | OpCode::BuildTuple
            | OpCode::BuildObject
            | OpCode::BuildObjectWithShape
            | OpCode::GetIndex
            | OpCode::SetIndex
            | OpCode::ObjectRest
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::GetProperty
            | OpCode::GetPropertyMaybe
            | OpCode::SetProperty
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetSuper
            | OpCode::MakeClosure
            | OpCode::MakeClass
            | OpCode::Inherit
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::DeclareLayout
            | OpCode::AllocInstance
            | OpCode::BindMethod
            | OpCode::Typeof
            | OpCode::Instanceof
            | OpCode::In
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::AssertNotNull
            | OpCode::StrConcat
            | OpCode::StrLength
            | OpCode::StrSlice
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::WrapSpread
            | OpCode::MakeEnumVariant
            | OpCode::GetEnumTag
            | OpCode::Await
            | OpCode::Spawn
            | OpCode::Yield
            | OpCode::Try
            | OpCode::Throw
            | OpCode::PopTry
            | OpCode::LoadModule
            | OpCode::LoadModuleSlot
            | OpCode::StoreModuleSlot
            | OpCode::InvokeRuntimeStatic
            | OpCode::BuildStr
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::EqStr
            | OpCode::NeqStr
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::Intrinsic
            | OpCode::LoadStaticFn
            | OpCode::CallSelf
            | OpCode::Nop
            | OpCode::ArrayGetIndex
            | OpCode::BytesGetIndex
            | OpCode::ArraySetIndex
            | OpCode::BytesSetIndex
            | OpCode::CallNativeOp
            | OpCode::IntrinsicDirect
            | OpCode::LoadNativeGlobalIdx
            | OpCode::BuildMap
            | OpCode::MapGetIndex
            | OpCode::MapSetIndex
            | OpCode::Convert
            | OpCode::BytesLength => unreachable!(),
        }
        Ok(())
    }
}
