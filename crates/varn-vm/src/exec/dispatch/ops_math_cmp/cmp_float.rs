use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::super::{hi, lo};

impl ExecCtx {
    pub(super) fn exec_float_cmp(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let (r1, r2) = (hi(w1), lo(w1));
        match op {
            OpCode::LtFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val < b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) < self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::lt_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::GtFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val > b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) > self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::gt_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::LteFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val <= b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) <= self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::lte_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::GteFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val >= b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) >= self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::gte_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::EqFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val == b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) == self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::eq(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::NeqFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val != b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) != self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::neq(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
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
            | OpCode::BuildRecord
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
            | OpCode::GetSymbol
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
            | OpCode::AddImm
            | OpCode::SubImm
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
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::Intrinsic
            | OpCode::LoadStaticFn
            | OpCode::CallSelf
            | OpCode::Nop
            | OpCode::ArrayGetIndex
            | OpCode::ArraySetIndex
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
