use super::resolve::NameResolver;
use super::union::lower_union;
use varn_core::{AtomInterner, BuiltinType, LangPrimitive, TypeKind};
use varn_sem::types::{CheckerTyTable, Type};
use varn_tir::{BackendTy, DynReason, TyTable};

pub(super) const NEVER_TY: varn_tir::TyId = varn_tir::TyId(0);

pub const NULL_TY: BackendTy = BackendTy::Nullable(NEVER_TY);

pub fn lower_type(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    if let Some((index, shape)) = names.nominal_shape(*ty) {
        return lower_nominal_shape(index, shape, table, interner, tt, names);
    }
    match ty.builtin_of(table) {
        Some(b) if b != BuiltinType::Array => lower_builtin(b, ty, table, interner, tt, names),
        Some(_) | None => lower_kind(&table.get(ty.0), table, interner, tt, names),
    }
}

fn bare() -> BackendTy {
    BackendTy::Dynamic(DynReason::Declared)
}

fn interface() -> BackendTy {
    BackendTy::Dynamic(DynReason::Declared)
}

fn lower_nominal_shape(
    index: usize,
    shape: &varn_sem::output::ResolvedShape,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    let shape_kind: fn(varn_tir::ShapeId) -> DynReason = match shape.kind {
        varn_sem::output::ShapeKind::Interface => DynReason::Interface,
        varn_sem::output::ShapeKind::Object => DynReason::Shape,
        varn_sem::output::ShapeKind::Record => DynReason::Record,
    };
    if let Some(id) = tt.named_shape(index) {
        return BackendTy::Dynamic(shape_kind(id));
    }
    let id = tt.reserve_named_shape(index, shape.name.clone());
    let fields = shape
        .fields
        .iter()
        .map(|p| varn_tir::ShapeField {
            name: p.name.clone(),
            ty: lower_type(&p.ty, table, interner, tt, names),
            optional: p.is_optional,
            readonly: p.is_readonly,
        })
        .collect();
    tt.fill_named_shape(id, fields);
    BackendTy::Dynamic(shape_kind(id))
}

fn resolve_named(name: &str, names: &dyn NameResolver) -> Option<BackendTy> {
    names
        .class_id(name)
        .map(BackendTy::Class)
        .or_else(|| names.enum_id(name).map(BackendTy::Enum))
}

fn resolve_type_ref(
    name: varn_core::Atom,
    origin: Option<varn_core::Atom>,
    interner: &AtomInterner,
    names: &dyn NameResolver,
) -> Option<BackendTy> {
    let name = interner.resolve(name);
    match origin
        .and_then(|o| interner.try_resolve(o))
        .filter(|o| !names.is_local_origin(o))
    {
        Some(origin) => names
            .foreign_class_id(name, origin)
            .map(BackendTy::Class)
            .or_else(|| names.foreign_enum_id(name, origin).map(BackendTy::Enum)),
        None => resolve_named(name, names),
    }
}

fn lower_alias(
    name: &str,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    if names.is_type_param(name) {
        return BackendTy::Dynamic(DynReason::Generic);
    }
    let Some(target) = names.alias(name) else {
        return interface();
    };
    lower_through_alias(name, target, table, interner, tt, names)
}

fn lower_through_alias(
    name: &str,
    target: Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    if !tt.enter_alias(name) {
        return BackendTy::Dynamic(DynReason::Union);
    }
    let lowered = lower_type(&target, table, interner, tt, names);
    tt.leave_alias();
    lowered
}

fn lower_kind(
    kind: &varn_sem::types::InternedTypeKind,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    match *kind {
        TypeKind::Primitive(p) => lower_primitive(p),
        TypeKind::Literal(l) => BackendTy::Literal(match l {
            varn_core::TypeLiteral::Int(n) => varn_core::TypeLiteral::Int(n),
            varn_core::TypeLiteral::Bool(b) => varn_core::TypeLiteral::Bool(b),
            varn_core::TypeLiteral::Char(c) => varn_core::TypeLiteral::Char(c),
            varn_core::TypeLiteral::Str(s) => {
                varn_core::TypeLiteral::Str(tt.intern_literal_str(interner.resolve(s)))
            }
        }),

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

        TypeKind::Named(name, origin) => {
            let local = origin
                .and_then(|o| interner.try_resolve(o))
                .is_none_or(|o| names.is_local_origin(o));
            match resolve_type_ref(name, origin, interner, names) {
                None if local => lower_alias(interner.resolve(name), table, interner, tt, names),
                None if names.is_type_param(interner.resolve(name)) => {
                    BackendTy::Dynamic(DynReason::Generic)
                }
                None => interface(),
                Some(resolved) => resolved,
            }
        }

        TypeKind::Generic(name, _, origin) => {
            resolve_type_ref(name, origin, interner, names).unwrap_or_else(interface)
        }

        TypeKind::EnumVariant { enum_name, .. } => names
            .enum_id(interner.resolve(enum_name))
            .map(BackendTy::Enum)
            .unwrap_or_else(interface),

        TypeKind::Union(members) => lower_union(members, table, interner, tt, names),

        TypeKind::Object(members) => {
            let members = table.get_object_members(members);
            if let [varn_sem::types::ObjectTypeMember::Index {
                key_ty, value_ty, ..
            }] = members
            {
                let _ = key_ty;
                let v = lower_type(&Type::resolved(*value_ty), table, interner, tt, names);
                return BackendTy::Dict(tt.intern(v));
            }
            lower_shape(members, table, interner, tt, names, DynReason::Shape)
        }
        TypeKind::Record(members) => lower_shape(
            table.get_object_members(members),
            table,
            interner,
            tt,
            names,
            DynReason::Record,
        ),

        TypeKind::Fn(_) => BackendTy::Dynamic(DynReason::Callable),

        TypeKind::Intersection(_) => interface(),

        TypeKind::TemplateLiteral(_) | TypeKind::KeyOf(_) => BackendTy::Str,

        TypeKind::TypePredicate { .. } => BackendTy::Bool,

        TypeKind::This
        | TypeKind::Typeof(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_) => BackendTy::Dynamic(DynReason::Generic),
    }
}

fn is_plain_shape(members: &[varn_sem::types::ObjectTypeMember]) -> bool {
    use varn_sem::types::ObjectTypeMember as Member;
    members.iter().all(|m| match m {
        Member::Property { .. } | Member::Method { .. } => true,
        Member::Index { .. } | Member::Callable { .. } => false,
    })
}

fn shape_fields(
    members: &[varn_sem::types::ObjectTypeMember],
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> Vec<varn_tir::ShapeField> {
    use varn_sem::types::ObjectTypeMember as Member;
    let mut fields = Vec::with_capacity(members.len());
    for member in members {
        match member {
            Member::Property {
                name,
                ty,
                optional,
                readonly,
            } => fields.push(varn_tir::ShapeField {
                name: name.clone(),
                ty: lower_type(&Type::resolved(*ty), table, interner, tt, names),
                optional: *optional,
                readonly: *readonly,
            }),
            Member::Method { name, optional, .. } => fields.push(varn_tir::ShapeField {
                name: name.clone(),
                ty: BackendTy::Dynamic(DynReason::Callable),
                optional: *optional,
                readonly: true,
            }),
            Member::Index { .. } | Member::Callable { .. } => {}
        }
    }
    fields
}

fn lower_shape(
    members: &[varn_sem::types::ObjectTypeMember],
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
    shape_kind: fn(varn_tir::ShapeId) -> DynReason,
) -> BackendTy {
    if !is_plain_shape(members) {
        return BackendTy::Dynamic(DynReason::IndexSignature);
    }
    let fields = shape_fields(members, table, interner, tt, names);
    BackendTy::Dynamic(shape_kind(tt.intern_shape(fields)))
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
        LangPrimitive::Null => NULL_TY,
        LangPrimitive::Dynamic => BackendTy::Dynamic(DynReason::Declared),
    }
}

fn lower_builtin(
    b: BuiltinType,
    ty: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    let args: Vec<Type> = match table.get(ty.0) {
        TypeKind::Generic(_, list, _) => table
            .get_list(list)
            .iter()
            .map(|&id| Type::resolved(id))
            .collect(),
        TypeKind::Array(el) => vec![Type::resolved(el)],
        TypeKind::Named(..)
        | TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Record(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => vec![],
    };
    let lower = |t: &Type, tt: &mut TyTable| {
        let lowered = lower_type(t, table, interner, tt, names);
        tt.intern(lowered)
    };
    match (b, args.as_slice()) {
        (BuiltinType::Map, [k, v]) => BackendTy::Map(lower(k, tt), lower(v, tt)),
        (BuiltinType::Map, _) => {
            let d = tt.intern(bare());
            BackendTy::Map(d, d)
        }
        (BuiltinType::Set, [el]) => BackendTy::Set(lower(el, tt)),
        (BuiltinType::Set, _) => BackendTy::Set(tt.intern(bare())),
        (BuiltinType::Array, [el]) => BackendTy::Array(lower(el, tt)),
        (BuiltinType::Task, [result]) => {
            BackendTy::Dynamic(DynReason::Awaitable(lower(result, tt)))
        }
        (BuiltinType::Generator | BuiltinType::AsyncGenerator, [yielded]) => {
            BackendTy::Dynamic(DynReason::Yields(lower(yielded, tt)))
        }
        (BuiltinType::TaskHandle, [result]) => {
            BackendTy::Dynamic(DynReason::TaskHandle(lower(result, tt)))
        }
        (BuiltinType::Range, [bound]) => BackendTy::Dynamic(DynReason::Range(lower(bound, tt))),
        (BuiltinType::Bytes, _) => BackendTy::Bytes,
        (
            BuiltinType::Array
            | BuiltinType::Range
            | BuiltinType::Task
            | BuiltinType::TaskHandle
            | BuiltinType::Generator
            | BuiltinType::AsyncGenerator,
            _,
        ) => bare(),
    }
}

pub fn prime(tt: &mut TyTable) {
    let id = tt.intern(BackendTy::Never);
    debug_assert_eq!(id, NEVER_TY);
}
