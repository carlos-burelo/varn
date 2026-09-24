//! `varn_checker::types::Type` → `varn_tir::BackendTy`.
//!
//! The narrowing that `CgTy → HirType → SlotKind` did silently, done once and
//! explicitly. A type the TIR does not model precisely (a bare type
//! parameter, an imported type, a host-shaped intrinsic like `Regex` or
//! `Task`, an arbitrary function type) lowers to `Dynamic(Unannotated)` — the
//! value carries no more type information here, which is the honest state.

use crate::types::{CheckerTyTable, Type};
use varn_core::{AtomInterner, BuiltinType, LangPrimitive, TypeKind};
use varn_tir::{BackendTy, ClassId, DynReason, EnumId, TyTable};

/// Resolves a type name to the module-table handle the checker assigned it.
/// [`NoNames`] answers `None`, so every named type falls to
/// `Dynamic(Unannotated)`.
pub trait NameResolver {
    fn class_id(&self, name: &str) -> Option<ClassId>;
    fn enum_id(&self, name: &str) -> Option<EnumId>;
}

pub struct NoNames;

impl NameResolver for NoNames {
    fn class_id(&self, _: &str) -> Option<ClassId> {
        None
    }
    fn enum_id(&self, _: &str) -> Option<EnumId> {
        None
    }
}

/// Lower one type. `table`/`interner` are the checker's hash-consed type
/// table and the interner that resolves its `Atom` names — the source of
/// truth `ty` is an id into, now that `Type` no longer carries a materialized
/// tree of its own. `tt` interns the structured payloads; `names` maps a
/// named type to its class/enum handle.
pub fn lower_type(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    lower_kind(&table.get(ty.0), table, interner, tt, names)
}

/// A type the TIR does not model precisely.
fn opaque() -> BackendTy {
    BackendTy::Dynamic(DynReason::Unannotated)
}

fn resolve_named(name: &str, names: &dyn NameResolver) -> BackendTy {
    names
        .class_id(name)
        .map(BackendTy::Class)
        .or_else(|| names.enum_id(name).map(BackendTy::Enum))
        // An unresolved name is a type parameter (erased) or an imported type
        // — either way the type is genuinely unknown here, not a TIR gap.
        .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated))
}

fn lower_kind(
    kind: &crate::types::InternedTypeKind,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    match *kind {
        // `Map` / `Set` are structured intrinsics: their element types live in
        // a `Generic` node, or are absent for a bare annotation. Either way the
        // container itself is a known reference type, not `Dynamic` — this is
        // what lets `m.get(k)` reach `CallNativeOp` and `.size` a typed read.
        // `Map<V>` (key defaults to `str`) or `Map<K, V>`.
        TypeKind::Generic(name, args, _)
            if interner.resolve(name) == BuiltinType::Map.name()
                && (table.get_list(args).len() == 1 || table.get_list(args).len() == 2) =>
        {
            let arg_ids = table.get_list(args).to_vec();
            let (k, v) = if arg_ids.len() == 2 {
                (
                    lower_type(&Type(arg_ids[0], false), table, interner, tt, names),
                    lower_type(&Type(arg_ids[1], false), table, interner, tt, names),
                )
            } else {
                (
                    BackendTy::Str,
                    lower_type(&Type(arg_ids[0], false), table, interner, tt, names),
                )
            };
            BackendTy::Map(tt.intern(k), tt.intern(v))
        }
        TypeKind::Generic(name, args, _)
            if interner.resolve(name) == BuiltinType::Set.name() && table.get_list(args).len() == 1 =>
        {
            let el = lower_type(&Type(table.get_list(args)[0], false), table, interner, tt, names);
            BackendTy::Set(tt.intern(el))
        }
        TypeKind::Builtin(varn_core::BuiltinType::Map) => {
            let d = tt.intern(BackendTy::Dynamic(DynReason::Unannotated));
            BackendTy::Map(d, d)
        }
        TypeKind::Builtin(varn_core::BuiltinType::Set) => {
            let d = tt.intern(BackendTy::Dynamic(DynReason::Unannotated));
            BackendTy::Set(d)
        }

        TypeKind::Primitive(p) => lower_primitive(p),
        TypeKind::Literal(l) => lower_primitive(l.base()),
        TypeKind::Builtin(b) => lower_builtin(b),

        TypeKind::Array(el) => {
            let inner = lower_type(&Type(el, false), table, interner, tt, names);
            BackendTy::Array(tt.intern(inner))
        }

        TypeKind::Tuple(els) => {
            let lowered: Vec<BackendTy> = table
                .get_list(els)
                .iter()
                .map(|e| lower_type(&Type(*e, false), table, interner, tt, names))
                .collect();
            BackendTy::Tuple(tt.intern_list(&lowered))
        }

        TypeKind::Named(name, _) => resolve_named(interner.resolve(name), names),

        // A generic reference — `Box<T>`, `Result<int, str>` — is the named
        // class / enum with its type arguments erased at the backend level.
        // Only a generic that names neither (a bare type parameter `T`, an
        // alias) stays opaque.
        TypeKind::Generic(name, _, _)
            if names.class_id(interner.resolve(name)).is_some()
                || names.enum_id(interner.resolve(name)).is_some() =>
        {
            resolve_named(interner.resolve(name), names)
        }
        TypeKind::Generic(name, args, _) if table.get_list(args).is_empty() => {
            resolve_named(interner.resolve(name), names)
        }

        TypeKind::EnumVariant { enum_name, .. } => names
            .enum_id(interner.resolve(enum_name))
            .map(BackendTy::Enum)
            .unwrap_or_else(opaque),

        // `T | null` keeps its payload as Nullable; anything else is a
        // non-discriminated union and stays honestly dynamic.
        TypeKind::Union(members) => lower_union(members, table, interner, tt, names),

        // An object type with a single index signature is lowered to Map<K, V>.
        // An object type with named members reads like an index signature to
        // the backend: no slots, all by-name.
        TypeKind::Object(members) => {
            let members = table.get_object_members(members);
            if members.len() == 1 {
                if let crate::types::ObjectTypeMember::Index { key_ty, value_ty, .. } = &members[0]
                {
                    let k = lower_type(&Type(*key_ty, false), table, interner, tt, names);
                    let v = lower_type(&Type(*value_ty, false), table, interner, tt, names);
                    return BackendTy::Map(tt.intern(k), tt.intern(v));
                }
            }
            BackendTy::Dynamic(DynReason::IndexSignature)
        }

        // Types with no precise TIR representation: opaque dynamic.
        TypeKind::Fn(_)
        | TypeKind::Generic(..) // with type arguments
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
        // "only null": a Nullable payload of Never, so the null pattern with
        // no non-null inhabitant.
        LangPrimitive::Null => BackendTy::Nullable(NEVER_TY),
        LangPrimitive::Dynamic => BackendTy::Dynamic(DynReason::Unannotated),
    }
}

/// A builtin named without type arguments has no element type to lower.
fn lower_builtin(b: BuiltinType) -> BackendTy {
    match b {
        BuiltinType::Bytes => BackendTy::Bytes,
        BuiltinType::Array
        | BuiltinType::Map
        | BuiltinType::Set
        | BuiltinType::Range
        | BuiltinType::Task
        | BuiltinType::TaskHandle
        | BuiltinType::Generator => BackendTy::Dynamic(DynReason::Unannotated),
    }
}

/// `TyId(0)` is reserved for `Never` by [`prime`]; callers must run it once
/// per table before lowering.
const NEVER_TY: varn_tir::TyId = varn_tir::TyId(0);

/// Intern the handles `lower_tag` hands out as constants. Run once per module
/// table, before any `lower_type` call.
pub fn prime(tt: &mut TyTable) {
    let id = tt.intern(BackendTy::Never);
    debug_assert_eq!(id, NEVER_TY);
}

fn lower_union(
    members: crate::types::TyListId,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    let member_ids = table.get_list(members);
    let is_null = |id: &crate::types::CheckerTyId| {
        matches!(table.get(*id), TypeKind::Primitive(varn_core::LangPrimitive::Null))
    };
    let non_null: Vec<&crate::types::CheckerTyId> =
        member_ids.iter().filter(|id| !is_null(id)).collect();

    if non_null.len() == member_ids.len() {
        // No null in the union — non-discriminated.
        return BackendTy::Dynamic(DynReason::Union);
    }
    match non_null.as_slice() {
        [] => BackendTy::Nullable(NEVER_TY),
        [only] => {
            let inner = lower_type(&Type(**only, false), table, interner, tt, names);
            BackendTy::Nullable(tt.intern(inner))
        }
        // `A | B | null` — the payload itself is a non-discriminated union.
        _ => BackendTy::Dynamic(DynReason::Union),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CheckerTyId;
    use varn_core::TypeKind;

    fn table() -> (TyTable, CheckerTyTable, AtomInterner) {
        let mut tt = TyTable::default();
        prime(&mut tt);
        (tt, CheckerTyTable::default(), AtomInterner::new())
    }

    fn intern(ct: &mut CheckerTyTable, kind: crate::types::InternedTypeKind) -> Type {
        Type(ct.intern(kind), false)
    }

    #[test]
    fn scalars_map_directly() {
        let (mut tt, mut ct, interner) = table();
        let int_ty = intern(&mut ct, TypeKind::Primitive(varn_core::LangPrimitive::Int));
        assert_eq!(
            lower_type(&int_ty, &ct, &interner, &mut tt, &NoNames),
            BackendTy::Int
        );
        let str_ty = intern(&mut ct, TypeKind::Primitive(varn_core::LangPrimitive::Str));
        assert_eq!(
            lower_type(&str_ty, &ct, &interner, &mut tt, &NoNames),
            BackendTy::Str
        );
        let decimal_ty = intern(&mut ct, TypeKind::Primitive(varn_core::LangPrimitive::Decimal));
        assert_eq!(
            lower_type(&decimal_ty, &ct, &interner, &mut tt, &NoNames),
            BackendTy::Decimal
        );
    }

    #[test]
    fn array_of_int_is_array_of_int() {
        let (mut tt, mut ct, interner) = table();
        let int_id: CheckerTyId = ct.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let ty = intern(&mut ct, TypeKind::Array(int_id));
        let BackendTy::Array(id) = lower_type(&ty, &ct, &interner, &mut tt, &NoNames) else {
            panic!("expected Array");
        };
        assert_eq!(tt.get(id), BackendTy::Int);
    }

    #[test]
    fn int_or_null_is_nullable_int_not_dynamic() {
        let (mut tt, mut ct, interner) = table();
        let int_id = ct.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let null_id = ct.intern(TypeKind::Primitive(varn_core::LangPrimitive::Null));
        let list = ct.intern_list(&[int_id, null_id]);
        let ty = intern(&mut ct, TypeKind::Union(list));
        let BackendTy::Nullable(id) = lower_type(&ty, &ct, &interner, &mut tt, &NoNames) else {
            panic!("expected Nullable");
        };
        assert_eq!(tt.get(id), BackendTy::Int);
    }

    #[test]
    fn a_real_union_stays_dynamic_union() {
        let (mut tt, mut ct, interner) = table();
        let int_id = ct.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let str_id = ct.intern(TypeKind::Primitive(varn_core::LangPrimitive::Str));
        let list = ct.intern_list(&[int_id, str_id]);
        let ty = intern(&mut ct, TypeKind::Union(list));
        assert_eq!(
            lower_type(&ty, &ct, &interner, &mut tt, &NoNames),
            BackendTy::Dynamic(DynReason::Union)
        );
    }

    #[test]
    fn an_unresolved_named_type_is_dynamic_unannotated() {
        // A type parameter or an imported type: genuinely unknown here, not a
        // TIR representation gap.
        let (mut tt, mut ct, mut interner) = table();
        let name = interner.intern("Point");
        let ty = intern(&mut ct, TypeKind::Named(name, None));
        assert_eq!(
            lower_type(&ty, &ct, &interner, &mut tt, &NoNames),
            BackendTy::Dynamic(DynReason::Unannotated)
        );
    }
}
