use super::super::{Captured, Site};
use crate::ssa::ir::{InstKind, VarId};
use varn_types::ssa::{SsaOp, SsaUpvalue};

pub(super) fn try_project(kind: &InstKind, site: &Site, captured: &mut Captured) -> Option<SsaOp> {
    match kind {
        InstKind::Call { callee, args } => Some(SsaOp::Call {
            callee: callee.0,
            args: args.iter().map(|v| v.0).collect(),
        }),
        InstKind::SelfCall { args } => Some(SsaOp::SelfCall {
            args: args.iter().map(|v| v.0).collect(),
        }),
        InstKind::MethodCall { recv, name, args } => Some(SsaOp::MethodCall {
            recv: recv.0,
            name: name.as_ref().into(),
            args: args.iter().map(|v| v.0).collect(),
            cs: u16::from(site.ic_slot?),
        }),
        InstKind::CallNativeOp {
            object,
            args,
            op_id,
        } => Some(SsaOp::CallNativeOp {
            object: object.0,
            args: args.iter().map(|v| v.0).collect(),
            op_id: *op_id,
        }),
        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } => Some(SsaOp::IntrinsicCall {
            object: object.0,
            args: args.iter().map(|v| v.0).collect(),
            wire: *wire_byte,
        }),
        InstKind::IterCall { callee, recv } => Some(SsaOp::IterCall {
            callee: callee.0,
            recv: recv.0,
        }),
        InstKind::GetSymbol { object, is_async } => Some(SsaOp::GetSymbol {
            object: object.0,
            is_async: *is_async,
        }),
        InstKind::SuperCall { args } => Some(SsaOp::SuperCall {
            args: args.iter().map(|v| v.0).collect(),
        }),
        InstKind::SuperMethodCall { name, args } => Some(SsaOp::SuperMethodCall {
            name: name.as_ref().into(),
            args: args.iter().map(|v| v.0).collect(),
        }),
        InstKind::ExtensionCall {
            func,
            slot,
            recv,
            args,
        } => Some(SsaOp::ExtensionCall {
            func: func.as_ref().into(),
            slot: *slot,
            recv: recv.0,
            args: args.iter().map(|v| v.0).collect(),
        }),
        InstKind::CallSpread { callee, args } => Some(SsaOp::CallSpread {
            callee: callee.0,
            args: args
                .iter()
                .map(|(v, s)| varn_types::ssa::SsaSpread {
                    value: v.0,
                    spread: *s,
                })
                .collect(),
        }),
        InstKind::MakeClosure { upvalues_src, .. } => Some(SsaOp::MakeClosure {
            proto: u32::from(site.closure_const?),
            upvalues: upvalues_src
                .iter()
                .map(|src| match src {
                    crate::hir::HirUpvalueSrc::ParentLocal(id) => {
                        SsaUpvalue::Captured(captured.index(VarId::Local(*id)))
                    }
                    crate::hir::HirUpvalueSrc::ParentParam(i) => {
                        SsaUpvalue::Captured(captured.index(VarId::Param(*i)))
                    }
                    crate::hir::HirUpvalueSrc::ParentUpvalue(idx) => SsaUpvalue::Inherited(*idx),
                })
                .collect(),
        }),
        InstKind::LoadCaptured { var } => Some(SsaOp::LoadCaptured {
            var: captured.index(*var),
        }),
        InstKind::StoreCaptured { var, value } => Some(SsaOp::StoreCaptured {
            var: captured.index(*var),
            value: value.0,
        }),
        InstKind::LoadUpvalue(index) => Some(SsaOp::LoadUpvalue(*index)),
        InstKind::StoreUpvalue { index, value } => Some(SsaOp::StoreUpvalue {
            index: *index,
            value: value.0,
        }),
        InstKind::CloseUpvalues { targets } => Some(SsaOp::CloseUpvalues {
            vars: targets.iter().map(|t| captured.index(*t)).collect(),
        }),
        InstKind::ConstInt(_)
        | InstKind::ConstFloat(_)
        | InstKind::ConstBool(_)
        | InstKind::ConstStr(_)
        | InstKind::ConstChar(_)
        | InstKind::ConstDecimal(_)
        | InstKind::ConstBigInt(_)
        | InstKind::ConstNull
        | InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::LoadGlobal(_)
        | InstKind::LoadGlobalIdx(_)
        | InstKind::LoadNativeGlobalIdx(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::IsNull { .. }
        | InstKind::Cast { .. }
        | InstKind::Convert { .. }
        | InstKind::BuildArray { .. }
        | InstKind::BuildTuple { .. }
        | InstKind::BuildObject { .. }
        | InstKind::BuildRecord { .. }
        | InstKind::BuildMap { .. }
        | InstKind::ObjectRest { .. }
        | InstKind::ToString { .. }
        | InstKind::BuildStr { .. }
        | InstKind::MakeClass { .. }
        | InstKind::DeclareLayout { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. }
        | InstKind::MakeEnumVariant { .. }
        | InstKind::Try { .. }
        | InstKind::PopTry
        | InstKind::CatchParam { .. }
        | InstKind::Dispose { .. }
        | InstKind::LoadModule { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::Await { .. }
        | InstKind::Spawn { .. }
        | InstKind::Yield { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::GetPropertyMaybe { .. }
        | InstKind::ModuleSlot { .. }
        | InstKind::GetEnumTag { .. }
        | InstKind::IsArray { .. }
        | InstKind::StrLength { .. }
        | InstKind::ArrayLength { .. }
        | InstKind::BytesLength { .. }
        | InstKind::This
        | InstKind::Range { .. }
        | InstKind::ObjectKeys { .. }
        | InstKind::GetSuper { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => None,
    }
}
