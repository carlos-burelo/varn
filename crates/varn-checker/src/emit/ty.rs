//! `varn_checker::types::Type` → `varn_tir::BackendTy`.
//!
//! The narrowing that `CgTy → HirType → SlotKind` did silently, done once and
//! explicitly. A type the TIR does not model precisely (a bare type
//! parameter, an imported type, a host-shaped intrinsic like `Regex` or
//! `Task`, an arbitrary function type) lowers to `Dynamic(Unannotated)` — the
//! value carries no more type information here, which is the honest state.

use crate::types::Type;
use varn_core::{TypeKind, TypeTag};
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

/// Lower one type. `tt` interns the structured payloads; `names` maps a named
/// type to its class/enum handle.
pub fn lower_type(ty: &Type, tt: &mut TyTable, names: &dyn NameResolver) -> BackendTy {
    lower_kind(ty.kind(), tt, names)
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
    kind: &crate::types::SemanticTypeKind,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    match kind {
        // `Map` / `Set` are structured intrinsics: their element types live in
        // a `Generic` node, or are absent for a bare annotation. Either way the
        // container itself is a known reference type, not `Dynamic` — this is
        // what lets `m.get(k)` reach `CallNativeOp` and `.size` a typed read.
        // `Map<V>` (key defaults to `str`) or `Map<K, V>`.
        TypeKind::Generic(name, args, _)
            if name.as_ref() == TypeTag::Map.name() && (args.len() == 1 || args.len() == 2) =>
        {
            let (k, v) = if args.len() == 2 {
                (lower_type(&args[0], tt, names), lower_type(&args[1], tt, names))
            } else {
                (BackendTy::Str, lower_type(&args[0], tt, names))
            };
            BackendTy::Map(tt.intern(k), tt.intern(v))
        }
        TypeKind::Generic(name, args, _)
            if name.as_ref() == TypeTag::Set.name() && args.len() == 1 =>
        {
            let el = lower_type(&args[0], tt, names);
            BackendTy::Set(tt.intern(el))
        }
        TypeKind::Intrinsic(TypeTag::Map) => {
            let d = tt.intern(BackendTy::Dynamic(DynReason::Unannotated));
            BackendTy::Map(d, d)
        }
        TypeKind::Intrinsic(TypeTag::Set) => {
            let d = tt.intern(BackendTy::Dynamic(DynReason::Unannotated));
            BackendTy::Set(d)
        }

        TypeKind::Intrinsic(tag) => lower_tag(*tag),

        TypeKind::Array(el) => {
            let inner = lower_type(el, tt, names);
            BackendTy::Array(tt.intern(inner))
        }

        TypeKind::Tuple(els) => {
            let lowered: Vec<BackendTy> =
                els.iter().map(|e| lower_type(e, tt, names)).collect();
            BackendTy::Tuple(tt.intern_list(&lowered))
        }

        TypeKind::Named(name, _) => resolve_named(name, names),

        // A generic reference — `Box<T>`, `Result<int, str>` — is the named
        // class / enum with its type arguments erased at the backend level.
        // Only a generic that names neither (a bare type parameter `T`, an
        // alias) stays opaque.
        TypeKind::Generic(name, _, _)
            if names.class_id(name).is_some() || names.enum_id(name).is_some() =>
        {
            resolve_named(name, names)
        }
        TypeKind::Generic(name, args, _) if args.is_empty() => resolve_named(name, names),

        TypeKind::EnumVariant { enum_name, .. } => {
            names.enum_id(enum_name).map(BackendTy::Enum).unwrap_or_else(opaque)
        }

        // `T | null` keeps its payload as Nullable; anything else is a
        // non-discriminated union and stays honestly dynamic.
        TypeKind::Union(members) => lower_union(members, tt, names),

        // An object type with named members reads like an index signature to
        // the backend: no slots, all by-name.
        TypeKind::Object(_) => BackendTy::Dynamic(DynReason::IndexSignature),

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

fn lower_tag(tag: TypeTag) -> BackendTy {
    match tag {
        TypeTag::Int => BackendTy::Int,
        TypeTag::Float => BackendTy::Float,
        TypeTag::Bool => BackendTy::Bool,
        TypeTag::Char => BackendTy::Char,
        TypeTag::Str => BackendTy::Str,
        TypeTag::Decimal => BackendTy::Decimal,
        TypeTag::BigInt => BackendTy::BigInt,
        TypeTag::Void => BackendTy::Void,
        TypeTag::Never => BackendTy::Never,
        // "only null": a Nullable payload of Never, so the null pattern with
        // no non-null inhabitant.
        TypeTag::Null => BackendTy::Nullable(NEVER_TY),
        TypeTag::Dynamic => BackendTy::Dynamic(DynReason::Unannotated),
        // Structured intrinsics with no resolved element type, plus the
        // host-shaped ones (Task, Regex, DateTime, …): opaque dynamic.
        _ => BackendTy::Dynamic(DynReason::Unannotated),
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

fn lower_union(members: &[Type], tt: &mut TyTable, names: &dyn NameResolver) -> BackendTy {
    let is_null = |t: &Type| matches!(t.kind(), TypeKind::Intrinsic(TypeTag::Null));
    let non_null: Vec<&Type> = members.iter().filter(|t| !is_null(t)).collect();

    if non_null.len() == members.len() {
        // No null in the union — non-discriminated.
        return BackendTy::Dynamic(DynReason::Union);
    }
    match non_null.as_slice() {
        [] => BackendTy::Nullable(NEVER_TY),
        [only] => {
            let inner = lower_type(only, tt, names);
            BackendTy::Nullable(tt.intern(inner))
        }
        // `A | B | null` — the payload itself is a non-discriminated union.
        _ => BackendTy::Dynamic(DynReason::Union),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_core::TypeKind;

    fn t(kind: crate::types::SemanticTypeKind) -> Type {
        Type(kind, false)
    }
    fn prim(tag: TypeTag) -> Type {
        t(TypeKind::Intrinsic(tag))
    }

    fn table() -> TyTable {
        let mut tt = TyTable::default();
        prime(&mut tt);
        tt
    }

    #[test]
    fn scalars_map_directly() {
        let mut tt = table();
        assert_eq!(
            lower_type(&prim(TypeTag::Int), &mut tt, &NoNames),
            BackendTy::Int
        );
        assert_eq!(
            lower_type(&prim(TypeTag::Char), &mut tt, &NoNames),
            BackendTy::Char
        );
        assert_eq!(
            lower_type(&prim(TypeTag::Str), &mut tt, &NoNames),
            BackendTy::Str
        );
        assert_eq!(
            lower_type(&prim(TypeTag::Decimal), &mut tt, &NoNames),
            BackendTy::Decimal
        );
    }

    #[test]
    fn dynamic_carries_unannotated_not_a_default() {
        let mut tt = table();
        assert_eq!(
            lower_type(&prim(TypeTag::Dynamic), &mut tt, &NoNames),
            BackendTy::Dynamic(DynReason::Unannotated)
        );
    }

    #[test]
    fn array_of_int_is_array_of_int() {
        let mut tt = table();
        let ty = t(TypeKind::Array(Box::new(prim(TypeTag::Int))));
        let BackendTy::Array(id) = lower_type(&ty, &mut tt, &NoNames) else {
            panic!("expected Array");
        };
        assert_eq!(tt.get(id), BackendTy::Int);
    }

    #[test]
    fn int_or_null_is_nullable_int_not_dynamic() {
        let mut tt = table();
        let ty = t(TypeKind::Union(vec![
            prim(TypeTag::Int),
            prim(TypeTag::Null),
        ]));
        let BackendTy::Nullable(id) = lower_type(&ty, &mut tt, &NoNames) else {
            panic!("expected Nullable");
        };
        assert_eq!(tt.get(id), BackendTy::Int);
    }

    #[test]
    fn a_real_union_stays_dynamic_union() {
        let mut tt = table();
        let ty = t(TypeKind::Union(vec![
            prim(TypeTag::Int),
            prim(TypeTag::Str),
        ]));
        assert_eq!(
            lower_type(&ty, &mut tt, &NoNames),
            BackendTy::Dynamic(DynReason::Union)
        );
    }

    #[test]
    fn an_unresolved_named_type_is_dynamic_unannotated() {
        // A type parameter or an imported type: genuinely unknown here, not a
        // TIR representation gap.
        let mut tt = table();
        let ty = t(TypeKind::Named(std::rc::Rc::from("Point"), None));
        assert_eq!(
            lower_type(&ty, &mut tt, &NoNames),
            BackendTy::Dynamic(DynReason::Unannotated)
        );
    }
}
