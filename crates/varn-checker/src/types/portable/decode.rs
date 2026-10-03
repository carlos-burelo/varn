use varn_core::{AtomInterner, TypeKind, TypeLiteral};

use super::super::{
    CheckerTyId, CheckerTyTable, FunctionParam, FunctionType, ObjectTypeMember, TyListId, Type,
};
use super::shape::{PortableFunction, PortableObjectMember, PortableParam, PortableType};

/// Re-interna `p` en `table` (la del consumidor) resolviendo y publicando los
/// nombres en `interner`. El id resultante es válido en `table` y en ninguna
/// otra.
pub fn decode(p: &PortableType, table: &mut CheckerTyTable, interner: &mut AtomInterner) -> Type {
    let ty = match p {
        PortableType::Primitive(p) => table.intern(TypeKind::Primitive(*p)),
        PortableType::Builtin(b) => table.intern(TypeKind::Builtin(*b)),
        PortableType::Literal(l) => table.intern(TypeKind::Literal(match l {
            TypeLiteral::Int(v) => TypeLiteral::Int(*v),
            TypeLiteral::Str(s) => TypeLiteral::Str(interner.intern(s)),
            TypeLiteral::Bool(v) => TypeLiteral::Bool(*v),
            TypeLiteral::Char(v) => TypeLiteral::Char(*v),
        })),
        PortableType::This => table.intern(TypeKind::This),
        PortableType::Array(inner) => {
            let inner = decode(inner, table, interner).0;
            table.intern(TypeKind::Array(inner))
        }
        PortableType::Union(members) => {
            let list = decode_list(members, table, interner);
            table.intern(TypeKind::Union(list))
        }
        PortableType::Intersection(members) => {
            let list = decode_list(members, table, interner);
            table.intern(TypeKind::Intersection(list))
        }
        PortableType::Tuple(members) => {
            let list = decode_list(members, table, interner);
            table.intern(TypeKind::Tuple(list))
        }
        PortableType::Named(n, o) => {
            let n = interner.intern(n);
            let o = o.as_ref().map(|s| interner.intern(s));
            table.intern(TypeKind::Named(n, o))
        }
        PortableType::Generic(n, args, o) => {
            let n = interner.intern(n);
            let list = decode_list(args, table, interner);
            let o = o.as_ref().map(|s| interner.intern(s));
            table.intern(TypeKind::Generic(n, list, o))
        }
        PortableType::TemplateLiteral(parts) => {
            let list = decode_list(parts, table, interner);
            table.intern(TypeKind::TemplateLiteral(list))
        }
        PortableType::Fn(f) => {
            let f = decode_function(f, table, interner);
            let fid = table.intern_function(f);
            table.intern(TypeKind::Fn(fid))
        }
        PortableType::Object(members) => {
            let members: Vec<ObjectTypeMember> = members
                .iter()
                .map(|m| decode_object_member(m, table, interner))
                .collect();
            let oid = table.intern_object_members(members);
            table.intern(TypeKind::Object(oid))
        }
        PortableType::KeyOf(inner) => {
            let inner = decode(inner, table, interner).0;
            table.intern(TypeKind::KeyOf(inner))
        }
        PortableType::IndexedAccess { object, index } => {
            let object = decode(object, table, interner).0;
            let index = decode(index, table, interner).0;
            table.intern(TypeKind::IndexedAccess { object, index })
        }
        PortableType::Mapped {
            key_var,
            source,
            value,
            optional,
            readonly,
        } => {
            let key_var = interner.intern(key_var);
            let source = decode(source, table, interner).0;
            let value = decode(value, table, interner).0;
            table.intern(TypeKind::Mapped {
                key_var,
                source,
                value,
                optional: *optional,
                readonly: *readonly,
            })
        }
        PortableType::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check = decode(check, table, interner).0;
            let extends = decode(extends, table, interner).0;
            let true_type = decode(true_type, table, interner).0;
            let false_type = decode(false_type, table, interner).0;
            table.intern(TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            })
        }
        PortableType::Infer(n) => {
            let n = interner.intern(n);
            table.intern(TypeKind::Infer(n))
        }
        PortableType::EnumVariant {
            enum_name,
            variant_name,
            type_args,
            payload_ty,
        } => {
            let enum_name = interner.intern(enum_name);
            let variant_name = interner.intern(variant_name);
            let type_args = decode_list(type_args, table, interner);
            let payload_ty = decode(payload_ty, table, interner).0;
            table.intern(TypeKind::EnumVariant {
                enum_name,
                variant_name,
                type_args,
                payload_ty,
            })
        }
        PortableType::TypePredicate {
            parameter_name,
            target_type,
        } => {
            let parameter_name = interner.intern(parameter_name);
            let target_type = decode(target_type, table, interner).0;
            table.intern(TypeKind::TypePredicate {
                parameter_name,
                target_type,
            })
        }
    };
    Type(ty, false)
}

/// Re-interna una lista portable y devuelve su `TyListId` local.
pub fn decode_list(
    list: &[PortableType],
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> TyListId {
    let ids: Vec<CheckerTyId> = list.iter().map(|p| decode(p, table, interner).0).collect();
    table.intern_list(&ids)
}

fn decode_function(
    f: &PortableFunction,
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> FunctionType {
    FunctionType {
        params: f
            .params
            .iter()
            .map(|p| decode_param(p, table, interner))
            .collect(),
        return_type: decode(&f.return_type, table, interner).0,
        is_arrow: f.is_arrow,
        type_params: f
            .type_params
            .iter()
            .map(|s| std::sync::Arc::from(s.as_str()))
            .collect(),
    }
}

fn decode_param(
    p: &PortableParam,
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> FunctionParam {
    FunctionParam {
        name: p.name.as_ref().map(|s| std::sync::Arc::from(s.as_str())),
        ty: decode(&p.ty, table, interner).0,
        optional: p.optional,
        is_rest: p.is_rest,
    }
}

fn decode_object_member(
    m: &PortableObjectMember,
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> ObjectTypeMember {
    match m {
        PortableObjectMember::Property {
            name,
            ty,
            optional,
            readonly,
        } => ObjectTypeMember::Property {
            name: std::sync::Arc::from(name.as_str()),
            ty: decode(ty, table, interner).0,
            optional: *optional,
            readonly: *readonly,
        },
        PortableObjectMember::Method {
            name,
            params,
            return_type,
            optional,
            is_arrow,
        } => ObjectTypeMember::Method {
            name: std::sync::Arc::from(name.as_str()),
            params: params
                .iter()
                .map(|p| decode_param(p, table, interner))
                .collect(),
            return_type: decode(return_type, table, interner).0,
            optional: *optional,
            is_arrow: *is_arrow,
        },
        PortableObjectMember::Index {
            param_name,
            key_ty,
            value_ty,
        } => ObjectTypeMember::Index {
            param_name: std::sync::Arc::from(param_name.as_str()),
            key_ty: decode(key_ty, table, interner).0,
            value_ty: decode(value_ty, table, interner).0,
        },
        PortableObjectMember::Callable {
            params,
            return_type,
            is_arrow,
        } => ObjectTypeMember::Callable {
            params: params
                .iter()
                .map(|p| decode_param(p, table, interner))
                .collect(),
            return_type: decode(return_type, table, interner).0,
            is_arrow: *is_arrow,
        },
    }
}
