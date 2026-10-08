use crate::hir::HirType;
use crate::ssa::ir::{InstKind, SsaFunc};
use rustc_hash::FxHashSet;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let mut array_values: FxHashSet<u32> = FxHashSet::default();
    let mut map_values: FxHashSet<u32> = FxHashSet::default();

    for (i, vdef) in func.values.iter().enumerate() {
        if matches!(vdef.ty, HirType::Array(_)) {
            array_values.insert(i as u32);
        } else if matches!(vdef.ty, HirType::Map(_, _)) {
            map_values.insert(i as u32);
        }
    }

    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                if matches!(inst.kind, InstKind::BuildArray { .. }) {
                    array_values.insert(d.0);
                } else if matches!(inst.kind, InstKind::BuildMap { .. }) {
                    map_values.insert(d.0);
                }
            }
        }
    }

    if array_values.is_empty() && map_values.is_empty() {
        return false;
    }

    for block in &mut func.blocks {
        for inst in &mut block.insts {
            match &inst.kind {
                InstKind::GetIndex { object, index } if array_values.contains(&object.0) => {
                    inst.kind = InstKind::ArrayGetIndex {
                        object: *object,
                        index: *index,
                    };
                    changed = true;
                }
                InstKind::SetIndex {
                    object,
                    index,
                    value,
                } if array_values.contains(&object.0) => {
                    inst.kind = InstKind::ArraySetIndex {
                        object: *object,
                        index: *index,
                        value: *value,
                    };
                    changed = true;
                }
                InstKind::GetIndex { object, index } if map_values.contains(&object.0) => {
                    inst.kind = InstKind::MapGetIndex {
                        object: *object,
                        index: *index,
                    };
                    changed = true;
                }
                InstKind::SetIndex {
                    object,
                    index,
                    value,
                } if map_values.contains(&object.0) => {
                    inst.kind = InstKind::MapSetIndex {
                        object: *object,
                        index: *index,
                        value: *value,
                    };
                    changed = true;
                }
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
                | InstKind::MapGetIndex { .. }
                | InstKind::SetProperty { .. }
                | InstKind::SetFixedField { .. }
                | InstKind::SetIndex { .. }
                | InstKind::ArraySetIndex { .. }
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
                | InstKind::BuildObjectSpread { .. } => {}
            }
        }
    }

    changed
}
