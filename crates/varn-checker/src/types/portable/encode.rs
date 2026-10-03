use varn_core::{AtomInterner, LangPrimitive, TypeKind, TypeLiteral};

use super::super::{CheckerTyTable, FunctionParam, FunctionType, ObjectTypeMember, TyListId, Type};
use super::shape::{PortableFunction, PortableObjectMember, PortableParam, PortableType};

/// Codifica `ty` en su forma portable. Requiere la tabla que lo internó y el
/// interner que resuelve los nombres embebidos.
pub fn encode(ty: Type, table: &CheckerTyTable, interner: &AtomInterner) -> PortableType {
    let name = |a: varn_core::Atom| interner.resolve(a).to_string();
    match table.get(ty.0) {
        TypeKind::Primitive(p) => PortableType::Primitive(p),
        TypeKind::Builtin(b) => PortableType::Builtin(b),
        TypeKind::Literal(l) => PortableType::Literal(match l {
            TypeLiteral::Int(v) => TypeLiteral::Int(v),
            TypeLiteral::Str(a) => TypeLiteral::Str(name(a)),
            TypeLiteral::Bool(v) => TypeLiteral::Bool(v),
            TypeLiteral::Char(v) => TypeLiteral::Char(v),
        }),
        TypeKind::This => PortableType::This,
        TypeKind::Array(inner) => {
            PortableType::Array(Box::new(encode(Type(inner, false), table, interner)))
        }
        TypeKind::Union(list) => PortableType::Union(encode_list(list, table, interner)),
        TypeKind::Intersection(list) => {
            PortableType::Intersection(encode_list(list, table, interner))
        }
        TypeKind::Tuple(list) => PortableType::Tuple(encode_list(list, table, interner)),
        TypeKind::Named(n, o) => PortableType::Named(name(n), o.map(name)),
        TypeKind::Generic(n, list, o) => {
            PortableType::Generic(name(n), encode_list(list, table, interner), o.map(name))
        }
        TypeKind::TemplateLiteral(list) => {
            PortableType::TemplateLiteral(encode_list(list, table, interner))
        }
        TypeKind::Fn(fid) => {
            PortableType::Fn(encode_function(table.get_function(fid), table, interner))
        }
        TypeKind::Object(oid) => PortableType::Object(
            table
                .get_object_members(oid)
                .iter()
                .map(|m| encode_object_member(m, table, interner))
                .collect(),
        ),
        // `ExprId` es arena-relativo: no hay forma portable de escribirlo.
        // Honesto-desconocido en vez de un número que apunte a otra cosa.
        TypeKind::Typeof(_) => PortableType::Primitive(LangPrimitive::Dynamic),
        TypeKind::KeyOf(inner) => {
            PortableType::KeyOf(Box::new(encode(Type(inner, false), table, interner)))
        }
        TypeKind::IndexedAccess { object, index } => PortableType::IndexedAccess {
            object: Box::new(encode(Type(object, false), table, interner)),
            index: Box::new(encode(Type(index, false), table, interner)),
        },
        TypeKind::Mapped {
            key_var,
            source,
            value,
            optional,
            readonly,
        } => PortableType::Mapped {
            key_var: name(key_var),
            source: Box::new(encode(Type(source, false), table, interner)),
            value: Box::new(encode(Type(value, false), table, interner)),
            optional,
            readonly,
        },
        TypeKind::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => PortableType::Conditional {
            check: Box::new(encode(Type(check, false), table, interner)),
            extends: Box::new(encode(Type(extends, false), table, interner)),
            true_type: Box::new(encode(Type(true_type, false), table, interner)),
            false_type: Box::new(encode(Type(false_type, false), table, interner)),
        },
        TypeKind::Infer(n) => PortableType::Infer(name(n)),
        TypeKind::EnumVariant {
            enum_name,
            variant_name,
            type_args,
            payload_ty,
        } => PortableType::EnumVariant {
            enum_name: name(enum_name),
            variant_name: name(variant_name),
            type_args: encode_list(type_args, table, interner),
            payload_ty: Box::new(encode(Type(payload_ty, false), table, interner)),
        },
        TypeKind::TypePredicate {
            parameter_name,
            target_type,
        } => PortableType::TypePredicate {
            parameter_name: name(parameter_name),
            target_type: Box::new(encode(Type(target_type, false), table, interner)),
        },
    }
}

/// Codifica una lista de tipos ya internada (`TyListId`).
pub fn encode_list(
    list: TyListId,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> Vec<PortableType> {
    table
        .get_list(list)
        .iter()
        .map(|id| encode(Type(*id, false), table, interner))
        .collect()
}

fn encode_function(
    f: &FunctionType,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> PortableFunction {
    PortableFunction {
        params: f
            .params
            .iter()
            .map(|p| encode_param(p, table, interner))
            .collect(),
        return_type: Box::new(encode(Type(f.return_type, false), table, interner)),
        is_arrow: f.is_arrow,
        type_params: f.type_params.iter().map(|s| s.to_string()).collect(),
    }
}

fn encode_param(
    p: &FunctionParam,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> PortableParam {
    PortableParam {
        name: p.name.as_ref().map(|s| s.to_string()),
        ty: encode(Type(p.ty, false), table, interner),
        optional: p.optional,
        is_rest: p.is_rest,
    }
}

fn encode_object_member(
    m: &ObjectTypeMember,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> PortableObjectMember {
    match m {
        ObjectTypeMember::Property {
            name,
            ty,
            optional,
            readonly,
        } => PortableObjectMember::Property {
            name: name.to_string(),
            ty: encode(Type(*ty, false), table, interner),
            optional: *optional,
            readonly: *readonly,
        },
        ObjectTypeMember::Method {
            name,
            params,
            return_type,
            optional,
            is_arrow,
        } => PortableObjectMember::Method {
            name: name.to_string(),
            params: params
                .iter()
                .map(|p| encode_param(p, table, interner))
                .collect(),
            return_type: encode(Type(*return_type, false), table, interner),
            optional: *optional,
            is_arrow: *is_arrow,
        },
        ObjectTypeMember::Index {
            param_name,
            key_ty,
            value_ty,
        } => PortableObjectMember::Index {
            param_name: param_name.to_string(),
            key_ty: encode(Type(*key_ty, false), table, interner),
            value_ty: encode(Type(*value_ty, false), table, interner),
        },
        ObjectTypeMember::Callable {
            params,
            return_type,
            is_arrow,
        } => PortableObjectMember::Callable {
            params: params
                .iter()
                .map(|p| encode_param(p, table, interner))
                .collect(),
            return_type: encode(Type(*return_type, false), table, interner),
            is_arrow: *is_arrow,
        },
    }
}
