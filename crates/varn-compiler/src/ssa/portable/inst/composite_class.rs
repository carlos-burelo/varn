use crate::ssa::ir::InstKind;
use varn_types::ssa::SsaOp;

pub(super) fn project_class(kind: &InstKind) -> Option<SsaOp> {
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
        | InstKind::BytesGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
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
