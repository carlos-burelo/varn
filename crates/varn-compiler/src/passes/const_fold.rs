use super::const_fold_values::{const_inst_ty, fold_binary, fold_convert, fold_unary};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashMap;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let mut const_map = FxHashMap::default();

    for block_idx in 0..func.blocks.len() {
        let b_id = BlockId(block_idx as u32);

        for inst_idx in 0..func.blocks[b_id.0 as usize].insts.len() {
            let inst = &func.blocks[b_id.0 as usize].insts[inst_idx];
            if let Some(dest) = inst.dest {
                if is_constant_kind(&inst.kind) {
                    const_map.insert(dest, inst.kind.clone());
                    continue;
                }

                if let Some(folded_kind) = fold_inst(&inst.kind, &const_map) {
                    if let Some(ty) = const_inst_ty(&folded_kind) {
                        func.values[dest.0 as usize].ty = ty;
                    }
                    func.blocks[b_id.0 as usize].insts[inst_idx].kind = folded_kind.clone();
                    const_map.insert(dest, folded_kind);
                    changed = true;
                }
            }
        }

        let term = func.blocks[b_id.0 as usize].term.clone();
        if let Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } = term
            && let Some(InstKind::ConstBool(b)) = const_map.get(&cond)
        {
            if *b {
                func.blocks[b_id.0 as usize].term = Terminator::Jump {
                    target: then_blk,
                    args: then_args,
                };
                let preds = &mut func.blocks[else_blk.0 as usize].preds;
                if let Some(pos) = preds.iter().position(|p| *p == b_id) {
                    preds.remove(pos);
                }
            } else {
                func.blocks[b_id.0 as usize].term = Terminator::Jump {
                    target: else_blk,
                    args: else_args,
                };
                let preds = &mut func.blocks[then_blk.0 as usize].preds;
                if let Some(pos) = preds.iter().position(|p| *p == b_id) {
                    preds.remove(pos);
                }
            }
            changed = true;
        }
    }

    changed
}

pub(super) fn is_constant_kind(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::ConstInt(_)
            | InstKind::ConstFloat(_)
            | InstKind::ConstBool(_)
            | InstKind::ConstStr(_)
            | InstKind::ConstChar(_)
            | InstKind::ConstDecimal(_)
            | InstKind::ConstBigInt(_)
            | InstKind::ConstNull
    )
}

fn fold_inst(kind: &InstKind, const_map: &FxHashMap<Value, InstKind>) -> Option<InstKind> {
    match kind {
        InstKind::Unary { op, operand, ty } => {
            let operand_const = const_map.get(operand)?;
            fold_unary(*op, operand_const, *ty)
        }
        InstKind::Binary { op, lhs, rhs, ty } => {
            let lhs_const = const_map.get(lhs)?;
            let rhs_const = const_map.get(rhs)?;
            fold_binary(*op, lhs_const, rhs_const, *ty)
        }
        InstKind::Convert { operand, conv } => fold_convert(*conv, const_map.get(operand)?),
        InstKind::IsNull { operand } => {
            let operand_const = const_map.get(operand)?;
            match operand_const {
                InstKind::ConstNull => Some(InstKind::ConstBool(true)),
                InstKind::ConstInt(_)
                | InstKind::ConstFloat(_)
                | InstKind::ConstBool(_)
                | InstKind::ConstStr(_)
                | InstKind::ConstChar(_)
                | InstKind::ConstDecimal(_)
                | InstKind::ConstBigInt(_)
                | InstKind::Binary { .. }
                | InstKind::Unary { .. }
                | InstKind::LoadGlobal(_)
                | InstKind::LoadGlobalIdx(_)
                | InstKind::LoadNativeGlobalIdx(_)
                | InstKind::LoadUpvalue(_)
                | InstKind::StoreGlobal { .. }
                | InstKind::StoreGlobalIdx { .. }
                | InstKind::StoreUpvalue { .. }
                | InstKind::Call { .. }
                | InstKind::AllocInstance { .. }
                | InstKind::SelfCall { .. }
                | InstKind::GetProperty { .. }
                | InstKind::GetFixedField { .. }
                | InstKind::GetIndex { .. }
                | InstKind::ArrayGetIndex { .. }
                | InstKind::BytesGetIndex { .. }
                | InstKind::MapGetIndex { .. }
                | InstKind::SetProperty { .. }
                | InstKind::SetFixedField { .. }
                | InstKind::SetIndex { .. }
                | InstKind::ArraySetIndex { .. }
                | InstKind::BytesSetIndex { .. }
                | InstKind::MapSetIndex { .. }
                | InstKind::ArrayPush { .. }
                | InstKind::ObjectMerge { .. }
                | InstKind::MethodCall { .. }
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
                | InstKind::MakeClosure { .. }
                | InstKind::LoadCaptured { .. }
                | InstKind::StoreCaptured { .. }
                | InstKind::MakeClass { .. }
                | InstKind::DeclareLayout { .. }
                | InstKind::DefineStatic { .. }
                | InstKind::DefineMethod { .. }
                | InstKind::DefineAccessor { .. }
                | InstKind::MakeEnumVariant { .. }
                | InstKind::Try { .. }
                | InstKind::PopTry
                | InstKind::CatchParam { .. }
                | InstKind::CloseUpvalues { .. }
                | InstKind::Dispose { .. }
                | InstKind::LoadModule { .. }
                | InstKind::StoreModuleSlot { .. }
                | InstKind::Await { .. }
                | InstKind::Spawn { .. }
                | InstKind::Yield { .. }
                | InstKind::IntrinsicCall { .. }
                | InstKind::CallNativeOp { .. }
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
                | InstKind::GetSymbol { .. }
                | InstKind::IterCall { .. }
                | InstKind::GetSuper { .. }
                | InstKind::SuperCall { .. }
                | InstKind::SuperMethodCall { .. }
                | InstKind::ExtensionCall { .. }
                | InstKind::CallSpread { .. }
                | InstKind::BuildArraySpread { .. }
                | InstKind::BuildObjectSpread { .. } => Some(InstKind::ConstBool(false)),
            }
        }
        InstKind::ConstInt(_)
        | InstKind::ConstFloat(_)
        | InstKind::ConstBool(_)
        | InstKind::ConstStr(_)
        | InstKind::ConstChar(_)
        | InstKind::ConstDecimal(_)
        | InstKind::ConstBigInt(_)
        | InstKind::ConstNull
        | InstKind::LoadGlobal(_)
        | InstKind::LoadGlobalIdx(_)
        | InstKind::LoadNativeGlobalIdx(_)
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::BytesGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::BytesSetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::MethodCall { .. }
        | InstKind::Cast { .. }
        | InstKind::BuildArray { .. }
        | InstKind::BuildTuple { .. }
        | InstKind::BuildObject { .. }
        | InstKind::BuildRecord { .. }
        | InstKind::BuildMap { .. }
        | InstKind::ObjectRest { .. }
        | InstKind::ToString { .. }
        | InstKind::BuildStr { .. }
        | InstKind::MakeClosure { .. }
        | InstKind::LoadCaptured { .. }
        | InstKind::StoreCaptured { .. }
        | InstKind::MakeClass { .. }
        | InstKind::DeclareLayout { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. }
        | InstKind::MakeEnumVariant { .. }
        | InstKind::Try { .. }
        | InstKind::PopTry
        | InstKind::CatchParam { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::LoadModule { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::Await { .. }
        | InstKind::Spawn { .. }
        | InstKind::Yield { .. }
        | InstKind::IntrinsicCall { .. }
        | InstKind::CallNativeOp { .. }
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
        | InstKind::GetSymbol { .. }
        | InstKind::IterCall { .. }
        | InstKind::GetSuper { .. }
        | InstKind::SuperCall { .. }
        | InstKind::SuperMethodCall { .. }
        | InstKind::ExtensionCall { .. }
        | InstKind::CallSpread { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => None,
    }
}
