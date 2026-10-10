use super::resolve::NameResolver;
use super::union::lower_union;
use varn_core::{AtomInterner, BuiltinType, LangPrimitive, TypeKind};
use varn_sem::types::{CheckerTyTable, Type};
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
            .foreign_class_id(name, origin)
            .map(BackendTy::Class)
            .or_else(|| names.foreign_enum_id(name, origin).map(BackendTy::Enum))
            .unwrap_or_else(opaque),
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
    if let Some(id) = tt.named_shape(name) {
        return BackendTy::Dynamic(DynReason::Shape(id));
    }
    let members = match names.alias(name).map(|target| table.get(target.0)) {
        Some(TypeKind::Object(members)) => table.get_object_members(members),
        Some(_) | None => return opaque(),
    };
    if members.is_empty() || !is_plain_shape(members) {
        return opaque();
    }
    let id = tt.reserve_named_shape(std::sync::Arc::from(name));
    let fields = shape_fields(members, table, interner, tt, names);
    tt.fill_named_shape(id, fields);
    BackendTy::Dynamic(DynReason::Shape(id))
}

fn lower_kind(
    kind: &varn_sem::types::InternedTypeKind,
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
        TypeKind::Generic(name, args, _)
            if interner.resolve(name) == BuiltinType::Task.name()
                && table.get_list(args).len() == 1 =>
        {
            let result = lower_type(
                &Type::resolved(table.get_list(args)[0]),
                table,
                interner,
                tt,
                names,
            );
            BackendTy::Dynamic(DynReason::Awaitable(tt.intern(result)))
        }
        TypeKind::Generic(name, args, _)
            if matches!(
                interner.resolve(name),
                "Generator" | "AsyncGenerator"
            ) && table.get_list(args).len() == 1 =>
        {
            let yielded = lower_type(
                &Type::resolved(table.get_list(args)[0]),
                table,
                interner,
                tt,
                names,
            );
            BackendTy::Dynamic(DynReason::Yields(tt.intern(yielded)))
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

        TypeKind::Named(name, origin) => {
            let local = origin
                .and_then(|o| interner.try_resolve(o))
                .is_none_or(|o| names.is_local_origin(o));
            match resolve_type_ref(name, origin, interner, names) {
                BackendTy::Dynamic(_) if local => {
                    lower_alias(interner.resolve(name), table, interner, tt, names)
                }
                resolved => resolved,
            }
        }

        TypeKind::Generic(name, _, origin) => resolve_type_ref(name, origin, interner, names),

        TypeKind::EnumVariant { enum_name, .. } => names
            .enum_id(interner.resolve(enum_name))
            .map(BackendTy::Enum)
            .unwrap_or_else(opaque),

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
            lower_shape(members, table, interner, tt, names)
        }

        TypeKind::Fn(_) => BackendTy::Dynamic(DynReason::Callable),

        TypeKind::Intersection(_)
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
                name, ty, optional, ..
            } => fields.push(varn_tir::ShapeField {
                name: name.clone(),
                ty: lower_type(&Type::resolved(*ty), table, interner, tt, names),
                optional: *optional,
            }),
            Member::Method { name, optional, .. } => fields.push(varn_tir::ShapeField {
                name: name.clone(),
                ty: BackendTy::Dynamic(DynReason::Callable),
                optional: *optional,
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
) -> BackendTy {
    if members.is_empty() || !is_plain_shape(members) || !tt.enter_shape() {
        return BackendTy::Dynamic(DynReason::IndexSignature);
    }
    let fields = shape_fields(members, table, interner, tt, names);
    BackendTy::Dynamic(DynReason::Shape(tt.leave_shape(fields)))
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
