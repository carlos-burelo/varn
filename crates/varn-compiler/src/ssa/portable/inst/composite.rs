use super::super::Site;
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use varn_types::ssa::SsaOp;

pub(super) fn try_project(kind: &InstKind, value_tys: &[HirType], site: &Site) -> Option<SsaOp> {
    match kind {
        InstKind::AllocInstance {
            class,
            payload_size,
            ..
        } => Some(SsaOp::AllocInstance {
            class: class.0,
            payload_size: *payload_size,
        }),
        InstKind::BuildStr { parts } => Some(SsaOp::BuildStr {
            parts: parts.iter().map(|v| v.0).collect(),
        }),
        InstKind::BuildArray { elements } => Some(SsaOp::BuildArray {
            elements: elements.iter().map(|v| v.0).collect(),
        }),
        InstKind::BuildMap { pairs } => Some(SsaOp::BuildMap {
            pairs: pairs.iter().map(|(k, v)| (k.0, v.0)).collect(),
        }),
        InstKind::BuildObject { pairs } | InstKind::BuildRecord { pairs } => {
            Some(SsaOp::BuildObject {
                keys: pairs.iter().map(|(k, _)| k.as_ref().into()).collect(),
                values: pairs.iter().map(|(_, v)| v.0).collect(),
                is_record: matches!(kind, InstKind::BuildRecord { .. }),
            })
        }
        InstKind::BuildTuple { elements } => Some(SsaOp::BuildTuple {
            elements: elements.iter().map(|v| v.0).collect(),
        }),
        InstKind::BuildArraySpread { elements } => Some(SsaOp::BuildArraySpread {
            elements: elements
                .iter()
                .map(|(v, s)| varn_types::ssa::SsaSpread {
                    value: v.0,
                    spread: *s,
                })
                .collect(),
        }),
        InstKind::BuildObjectSpread { parts } => Some(SsaOp::BuildObjectSpread {
            parts: parts
                .iter()
                .map(|(k, v)| varn_types::ssa::SsaObjectSpreadPart {
                    key: k.as_ref().map(|s| s.as_ref().into()),
                    value: v.0,
                })
                .collect(),
            cs_base: u16::from(site.ic_slot?),
        }),
        InstKind::GetProperty { object, name } => Some(SsaOp::GetProperty {
            object: object.0,
            name: name.as_ref().into(),
            cs: u16::from(site.ic_slot?),
        }),
        InstKind::SetProperty {
            object,
            name,
            value,
        } => Some(SsaOp::SetProperty {
            object: object.0,
            value: value.0,
            name: name.as_ref().into(),
            cs: u16::from(site.ic_slot?),
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
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. }
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
        | InstKind::ObjectRest { .. }
        | InstKind::ToString { .. }
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
        | InstKind::CallSpread { .. } => project_indexed(kind, value_tys)
            .or_else(|| project_object(kind))
            .or_else(|| project_class(kind)),
    }
}

fn project_indexed(kind: &InstKind, value_tys: &[HirType]) -> Option<SsaOp> {
    match kind {
        InstKind::ArrayGetIndex { object, index } if is_int(value_tys, *index) => {
            Some(SsaOp::ArrayGetIndex {
                object: object.0,
                index: index.0,
            })
        }
        InstKind::ArraySetIndex {
            object,
            index,
            value,
        } if is_int(value_tys, *index) => Some(SsaOp::ArraySetIndex {
            object: object.0,
            index: index.0,
            value: value.0,
        }),
        InstKind::GetIndex { object, index }
        | InstKind::ArrayGetIndex { object, index }
        | InstKind::MapGetIndex { object, index } => Some(SsaOp::GetIndex {
            object: object.0,
            index: index.0,
        }),
        InstKind::SetIndex {
            object,
            index,
            value,
        }
        | InstKind::ArraySetIndex {
            object,
            index,
            value,
        }
        | InstKind::MapSetIndex {
            object,
            index,
            value,
        } => Some(SsaOp::SetIndex {
            object: object.0,
            index: index.0,
            value: value.0,
        }),
        InstKind::ArrayPush { array, value } => Some(SsaOp::ArrayPush {
            array: array.0,
            value: value.0,
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
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
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
        | InstKind::BuildObjectSpread { .. } => None,
    }
}

fn project_object(kind: &InstKind) -> Option<SsaOp> {
    match kind {
        InstKind::This => Some(SsaOp::This),
        InstKind::ObjectMerge { target, source } => Some(SsaOp::ObjectMerge {
            target: target.0,
            source: source.0,
        }),
        InstKind::ObjectRest { object, skip_keys } => Some(SsaOp::ObjectRest {
            object: object.0,
            skip_keys: skip_keys.iter().map(|k| k.as_ref().into()).collect(),
        }),
        InstKind::GetPropertyMaybe { object, name } => Some(SsaOp::GetPropertyMaybe {
            object: object.0,
            name: name.as_ref().into(),
        }),
        InstKind::ArrayLength { operand } => Some(SsaOp::ArrayLength { operand: operand.0 }),
        InstKind::BytesLength { operand } => Some(SsaOp::BytesLength { operand: operand.0 }),
        InstKind::StrLength { operand } => Some(SsaOp::StrLength { operand: operand.0 }),
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
        | InstKind::MethodCall { .. }
        | InstKind::IsNull { .. }
        | InstKind::Cast { .. }
        | InstKind::Convert { .. }
        | InstKind::BuildArray { .. }
        | InstKind::BuildTuple { .. }
        | InstKind::BuildObject { .. }
        | InstKind::BuildRecord { .. }
        | InstKind::BuildMap { .. }
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
        | InstKind::ModuleSlot { .. }
        | InstKind::GetEnumTag { .. }
        | InstKind::IsArray { .. }
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

fn project_class(kind: &InstKind) -> Option<SsaOp> {
    match kind {
        InstKind::GetFixedField {
            object,
            slot,
            offset,
            tag,
        } => Some(SsaOp::GetFixedField {
            object: object.0,
            slot: *slot,
            offset: *offset,
            access: *tag,
        }),
        InstKind::SetFixedField {
            object,
            value,
            slot,
            offset,
            tag,
        } => Some(SsaOp::SetFixedField {
            object: object.0,
            value: value.0,
            slot: *slot,
            offset: *offset,
            kind: *tag,
        }),
        InstKind::MakeClass { name, super_class } => Some(SsaOp::MakeClass {
            name: name.as_ref().into(),
            super_class: super_class.map(|v| v.0),
        }),
        InstKind::DeclareLayout { class, layout } => Some(SsaOp::DeclareLayout {
            class: class.0,
            layout: layout.clone(),
        }),
        InstKind::DefineStatic { class, name, value } => Some(SsaOp::DefineMethod {
            class: class.0,
            name: name.as_ref().into(),
            member: value.0,
            kind: 1,
        }),
        InstKind::DefineMethod {
            class,
            name,
            method,
            is_static,
        } => Some(SsaOp::DefineMethod {
            class: class.0,
            name: name.as_ref().into(),
            member: method.0,
            kind: if *is_static { 1 } else { 0 },
        }),
        InstKind::DefineAccessor {
            class,
            name,
            accessor,
            is_getter,
            is_static,
        } => Some(SsaOp::DefineMethod {
            class: class.0,
            name: name.as_ref().into(),
            member: accessor.0,
            kind: 2 + if *is_static { 2 } else { 0 } + if *is_getter { 0 } else { 1 },
        }),
        InstKind::GetSuper { name } => Some(SsaOp::GetSuper {
            name: name.as_ref().into(),
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
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
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
        | InstKind::SuperCall { .. }
        | InstKind::SuperMethodCall { .. }
        | InstKind::ExtensionCall { .. }
        | InstKind::CallSpread { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => None,
    }
}

fn is_int(value_tys: &[HirType], v: Value) -> bool {
    value_tys.get(v.0 as usize).is_some_and(|t| {
        crate::ssa::emit::slot_kind_of(*t) == varn_types::register_meta::SlotKind::Int
    })
}
