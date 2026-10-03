use super::resolve::NameResolver;
use super::union::lower_union;
use crate::types::{CheckerTyTable, Type};
use varn_core::{AtomInterner, BuiltinType, LangPrimitive, TypeKind};
use varn_tir::{BackendTy, DynReason, TyTable};

pub(super) const NEVER_TY: varn_tir::TyId = varn_tir::TyId(0);

pub fn lower_type(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    lower_kind(&table.get(ty.0), table, interner, tt, names)
}

fn opaque() -> BackendTy {
    BackendTy::Dynamic(DynReason::NotYetSupported)
}

fn resolve_named(name: &str, names: &dyn NameResolver) -> BackendTy {
    names
        .class_id(name)
        .map(BackendTy::Class)
        .or_else(|| names.enum_id(name).map(BackendTy::Enum))
        .unwrap_or_else(opaque)
}

fn resolve_type_ref(
    name: varn_core::Atom,
    origin: Option<varn_core::Atom>,
    interner: &AtomInterner,
    names: &dyn NameResolver,
) -> BackendTy {
    let name = interner.resolve(name);
    match origin
        .and_then(|o| interner.try_resolve(o))
        .filter(|o| !names.is_local_origin(o))
    {
        Some(origin) => names
            .foreign_enum_id(name, origin)
            .map(BackendTy::Enum)
            .unwrap_or_else(opaque),
        None => resolve_named(name, names),
    }
}

fn lower_kind(
    kind: &crate::types::InternedTypeKind,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    match *kind {
        TypeKind::Generic(name, args, _)
            if interner.resolve(name) == BuiltinType::Map.name()
                && table.get_list(args).len() == 2 =>
        {
            let arg_ids = table.get_list(args).to_vec();
            let k = lower_type(&Type::resolved(arg_ids[0]), table, interner, tt, names);
            let v = lower_type(&Type::resolved(arg_ids[1]), table, interner, tt, names);
            BackendTy::Map(tt.intern(k), tt.intern(v))
        }
        TypeKind::Generic(name, args, _)
            if interner.resolve(name) == BuiltinType::Set.name()
                && table.get_list(args).len() == 1 =>
        {
            let el = lower_type(
                &Type::resolved(table.get_list(args)[0]),
                table,
                interner,
                tt,
                names,
            );
            BackendTy::Set(tt.intern(el))
        }
        TypeKind::Builtin(varn_core::BuiltinType::Map) => {
            let d = tt.intern(opaque());
            BackendTy::Map(d, d)
        }
        TypeKind::Builtin(varn_core::BuiltinType::Set) => {
            let d = tt.intern(opaque());
            BackendTy::Set(d)
        }

        TypeKind::Primitive(p) => lower_primitive(p),
        TypeKind::Literal(l) => lower_primitive(l.base()),
        TypeKind::Builtin(b) => lower_builtin(b),

        TypeKind::Array(el) => {
            let inner = lower_type(&Type::resolved(el), table, interner, tt, names);
            BackendTy::Array(tt.intern(inner))
        }

        TypeKind::Tuple(els) => {
            let lowered: Vec<BackendTy> = table
                .get_list(els)
                .iter()
                .map(|e| lower_type(&Type::resolved(*e), table, interner, tt, names))
                .collect();
            BackendTy::Tuple(tt.intern_list(&lowered))
        }

        TypeKind::Named(name, origin) => resolve_type_ref(name, origin, interner, names),

        TypeKind::Generic(name, _, origin) => resolve_type_ref(name, origin, interner, names),

        TypeKind::EnumVariant { enum_name, .. } => names
            .enum_id(interner.resolve(enum_name))
            .map(BackendTy::Enum)
            .unwrap_or_else(opaque),

        TypeKind::Union(members) => lower_union(members, table, interner, tt, names),

        TypeKind::Object(members) => {
            let members = table.get_object_members(members);
            if members.len() == 1 {
                if let crate::types::ObjectTypeMember::Index {
                    key_ty, value_ty, ..
                } = &members[0]
                {
                    let k = lower_type(&Type::resolved(*key_ty), table, interner, tt, names);
                    let v = lower_type(&Type::resolved(*value_ty), table, interner, tt, names);
                    return BackendTy::Map(tt.intern(k), tt.intern(v));
                }
            }
            BackendTy::Dynamic(DynReason::IndexSignature)
        }

        TypeKind::Fn(_)
        | TypeKind::Intersection(_)
        | TypeKind::This
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::TypePredicate { .. } => opaque(),
    }
}

fn lower_primitive(p: LangPrimitive) -> BackendTy {
    match p {
        LangPrimitive::Int => BackendTy::Int,
        LangPrimitive::Float => BackendTy::Float,
        LangPrimitive::Bool => BackendTy::Bool,
        LangPrimitive::Char => BackendTy::Char,
        LangPrimitive::Str => BackendTy::Str,
        LangPrimitive::Decimal => BackendTy::Decimal,
        LangPrimitive::BigInt => BackendTy::BigInt,
        LangPrimitive::Void => BackendTy::Void,
        LangPrimitive::Never => BackendTy::Never,
        LangPrimitive::Null => BackendTy::Nullable(NEVER_TY),
        LangPrimitive::Dynamic => BackendTy::Dynamic(DynReason::Declared),
    }
}

fn lower_builtin(b: BuiltinType) -> BackendTy {
    match b {
        BuiltinType::Bytes => BackendTy::Bytes,
        BuiltinType::Array
        | BuiltinType::Map
        | BuiltinType::Set
        | BuiltinType::Range
        | BuiltinType::Task
        | BuiltinType::TaskHandle
        | BuiltinType::Generator => opaque(),
    }
}

pub fn prime(tt: &mut TyTable) {
    let id = tt.intern(BackendTy::Never);
    debug_assert_eq!(id, NEVER_TY);
}
