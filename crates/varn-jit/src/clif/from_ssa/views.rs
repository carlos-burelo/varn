use std::collections::BTreeMap;

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaUnOp};

#[derive(Clone, Copy)]
pub(crate) struct View {
    pub data: Variable,
    pub len: Variable,
    pub disc: Variable,
}

pub(crate) struct Views {
    by_value: BTreeMap<u32, View>,
}

impl Views {
    pub(super) fn declare(b: &mut FunctionBuilder, ssa: &SsaProto) -> Self {
        let mut by_value = BTreeMap::new();
        for inst in ssa.blocks.iter().flat_map(|blk| &blk.insts) {
            let object = match &inst.op {
                SsaOp::ArrayGetIndex { object, .. } | SsaOp::ArraySetIndex { object, .. } => {
                    *object
                }
                SsaOp::ConstInt(_) | SsaOp::ConstFloat(_) | SsaOp::ConstBool(_) | SsaOp::ConstNull | SsaOp::ConstStr(_) | SsaOp::Binary { .. } | SsaOp::Unary { .. } | SsaOp::SelfCall { .. } | SsaOp::Call { .. } | SsaOp::AllocInstance { .. } | SsaOp::LoadGlobalIdx(_) | SsaOp::ConstChar(_) | SsaOp::ConstBigInt(_) | SsaOp::ConstDecimal(_) | SsaOp::MakeEnumVariant { .. } | SsaOp::IntrinsicCall { .. } | SsaOp::LoadNativeGlobalIdx(_) | SsaOp::Try { .. } | SsaOp::PopTry | SsaOp::CatchParam { .. } | SsaOp::StoreGlobalIdx { .. } | SsaOp::MakeClosure { .. } | SsaOp::LoadCaptured { .. } | SsaOp::StoreCaptured { .. } | SsaOp::LoadUpvalue(_) | SsaOp::StoreUpvalue { .. } | SsaOp::CloseUpvalues { .. } | SsaOp::Cast { .. } | SsaOp::Convert { .. } | SsaOp::IsNull { .. } | SsaOp::Typeof { .. } | SsaOp::ToString { .. } | SsaOp::IsArray { .. } | SsaOp::GetEnumTag { .. } | SsaOp::ObjectKeys { .. } | SsaOp::BuildStr { .. } | SsaOp::BuildArray { .. } | SsaOp::BuildMap { .. } | SsaOp::BuildObject { .. } | SsaOp::GetProperty { .. } | SsaOp::SetProperty { .. } | SsaOp::GetIndex { .. } | SsaOp::SetIndex { .. } | SsaOp::ArrayLength { .. } | SsaOp::StrLength { .. } | SsaOp::BytesLength { .. } | SsaOp::MethodCall { .. } | SsaOp::CallNativeOp { .. } | SsaOp::ArrayPush { .. } | SsaOp::This | SsaOp::GetFixedField { .. } | SsaOp::SetFixedField { .. } | SsaOp::MakeClass { .. } | SsaOp::DeclareLayout { .. } | SsaOp::DefineMethod { .. } | SsaOp::GetSuper { .. } | SsaOp::LoadGlobal(_) | SsaOp::StoreGlobal { .. } | SsaOp::BuildTuple { .. } | SsaOp::BuildArraySpread { .. } | SsaOp::BuildObjectSpread { .. } | SsaOp::ObjectMerge { .. } | SsaOp::ObjectRest { .. } | SsaOp::GetPropertyMaybe { .. } | SsaOp::AssertNotNull { .. } | SsaOp::BindMethod { .. } | SsaOp::ArrayExtend { .. } | SsaOp::WrapSpread { .. } | SsaOp::Range { .. } | SsaOp::GetSymbol { .. } | SsaOp::IterCall { .. } | SsaOp::SuperCall { .. } | SsaOp::SuperMethodCall { .. } | SsaOp::ExtensionCall { .. } | SsaOp::CallSpread { .. } | SsaOp::LoadModule { .. } | SsaOp::ModuleSlot { .. } | SsaOp::StoreModuleSlot { .. } | SsaOp::Await { .. } | SsaOp::Spawn { .. } | SsaOp::Yield { .. } | SsaOp::Dispose { .. } => continue,
            };
            by_value.entry(object).or_insert_with(|| View {
                data: b.declare_var(types::I64),
                len: b.declare_var(types::I64),
                disc: b.declare_var(types::I64),
            });
        }
        let views = Views { by_value };
        views.clear(b);
        views
    }

    pub(super) fn of(&self, object: u32) -> Option<View> {
        self.by_value.get(&object).copied()
    }

    pub(super) fn clear(&self, b: &mut FunctionBuilder) {
        if self.by_value.is_empty() {
            return;
        }
        let zero = b.ins().iconst(types::I64, 0);
        for view in self.by_value.values() {
            b.def_var(view.data, zero);
            b.def_var(view.len, zero);
            b.def_var(view.disc, zero);
        }
    }
}

pub(super) fn keeps_views(ssa: &SsaProto, op: &SsaOp) -> bool {
    match op {
        SsaOp::ConstInt(_)
        | SsaOp::ConstFloat(_)
        | SsaOp::ConstBool(_)
        | SsaOp::ConstNull
        | SsaOp::Cast { .. }
        | SsaOp::LoadCaptured { .. }
        | SsaOp::StoreCaptured { .. }
        | SsaOp::ArrayGetIndex { .. }
        | SsaOp::ArraySetIndex { .. }
        | SsaOp::GetFixedField { .. }
        | SsaOp::ArrayLength { .. }
        | SsaOp::StrLength { .. }
        | SsaOp::BytesLength { .. }
        | SsaOp::LoadGlobal(_)
        | SsaOp::StoreGlobal { .. }
        | SsaOp::AssertNotNull { .. }
        | SsaOp::ModuleSlot { .. } => true,

        SsaOp::Binary { op, .. } => match op {
            SsaBinOp::IntAdd
            | SsaBinOp::IntSub
            | SsaBinOp::IntMul
            | SsaBinOp::IntEq
            | SsaBinOp::IntNe
            | SsaBinOp::IntLt
            | SsaBinOp::IntLe
            | SsaBinOp::IntGt
            | SsaBinOp::IntGe
            | SsaBinOp::IntAnd
            | SsaBinOp::IntOr
            | SsaBinOp::IntXor
            | SsaBinOp::IntShl
            | SsaBinOp::IntShr
            | SsaBinOp::IntUshr
            | SsaBinOp::FloatAdd
            | SsaBinOp::FloatSub
            | SsaBinOp::FloatMul
            | SsaBinOp::FloatDiv
            | SsaBinOp::FloatEq
            | SsaBinOp::FloatNe
            | SsaBinOp::FloatLt
            | SsaBinOp::FloatLe
            | SsaBinOp::FloatGt
            | SsaBinOp::FloatGe => true,
            SsaBinOp::IntDiv
            | SsaBinOp::IntMod
            | SsaBinOp::IntPow
            | SsaBinOp::FloatMod
            | SsaBinOp::FloatPow
            | SsaBinOp::StrConcat
            | SsaBinOp::Dyn(_) => false,
        },
        SsaOp::Unary { op, .. } => match op {
            SsaUnOp::NegInt | SsaUnOp::NegFloat | SsaUnOp::Not | SsaUnOp::BitNotInt => true,
            SsaUnOp::Dyn(_) => false,
        },
        SsaOp::Convert { operand, conv } => super::numeric::is_inline_convert(ssa, *operand, *conv),
        SsaOp::ConstStr(_) | SsaOp::SelfCall { .. } | SsaOp::Call { .. } | SsaOp::AllocInstance { .. } | SsaOp::LoadGlobalIdx(_) | SsaOp::ConstChar(_) | SsaOp::ConstBigInt(_) | SsaOp::ConstDecimal(_) | SsaOp::MakeEnumVariant { .. } | SsaOp::IntrinsicCall { .. } | SsaOp::LoadNativeGlobalIdx(_) | SsaOp::Try { .. } | SsaOp::PopTry | SsaOp::CatchParam { .. } | SsaOp::StoreGlobalIdx { .. } | SsaOp::MakeClosure { .. } | SsaOp::LoadUpvalue(_) | SsaOp::StoreUpvalue { .. } | SsaOp::CloseUpvalues { .. } | SsaOp::IsNull { .. } | SsaOp::Typeof { .. } | SsaOp::ToString { .. } | SsaOp::IsArray { .. } | SsaOp::GetEnumTag { .. } | SsaOp::ObjectKeys { .. } | SsaOp::BuildStr { .. } | SsaOp::BuildArray { .. } | SsaOp::BuildMap { .. } | SsaOp::BuildObject { .. } | SsaOp::GetProperty { .. } | SsaOp::SetProperty { .. } | SsaOp::GetIndex { .. } | SsaOp::SetIndex { .. } | SsaOp::MethodCall { .. } | SsaOp::CallNativeOp { .. } | SsaOp::ArrayPush { .. } | SsaOp::This | SsaOp::SetFixedField { .. } | SsaOp::MakeClass { .. } | SsaOp::DeclareLayout { .. } | SsaOp::DefineMethod { .. } | SsaOp::GetSuper { .. } | SsaOp::BuildTuple { .. } | SsaOp::BuildArraySpread { .. } | SsaOp::BuildObjectSpread { .. } | SsaOp::ObjectMerge { .. } | SsaOp::ObjectRest { .. } | SsaOp::GetPropertyMaybe { .. } | SsaOp::BindMethod { .. } | SsaOp::ArrayExtend { .. } | SsaOp::WrapSpread { .. } | SsaOp::Range { .. } | SsaOp::GetSymbol { .. } | SsaOp::IterCall { .. } | SsaOp::SuperCall { .. } | SsaOp::SuperMethodCall { .. } | SsaOp::ExtensionCall { .. } | SsaOp::CallSpread { .. } | SsaOp::LoadModule { .. } | SsaOp::StoreModuleSlot { .. } | SsaOp::Await { .. } | SsaOp::Spawn { .. } | SsaOp::Yield { .. } | SsaOp::Dispose { .. } => false,
    }
}
