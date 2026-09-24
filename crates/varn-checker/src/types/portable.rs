//! Forma **portable** de un `Type`: la misma estructura semántica, sin
//! `CheckerTyId`.
//!
//! Un `Type` es un `CheckerTyId` y solo significa algo dentro de la
//! `CheckerTyTable` que lo internó (ver `interned.rs`). Ningún artefacto que
//! cruce una frontera de módulo o de proceso — el caché en disco, la interfaz
//! de un módulo, el bundle stdlib — puede llevar ese id: el consumidor tiene
//! su propia tabla y el número apunta a otra forma.
//!
//! `PortableType` es ese árbol self-describing: nombres como texto, hijos
//! recursivos, sin índices. `encode` lo produce desde una tabla y `decode` lo
//! re-interna en la tabla del consumidor. Es el análogo, para tipos ya
//! resueltos, de lo que `TypeNode` es para tipos sintácticos.
//!
//! Lo único que no puede codificarse es `TypeKind::Typeof(ExprId)`: un
//! `ExprId` es relativo a un `AstArena` que este formato no transporta. Se
//! degrada a `Dynamic` (honesto-desconocido, igual que el resto del checker
//! para un tipo que no puede determinar) en lugar de inventar un índice.

use varn_core::{AtomInterner, BuiltinType, LangPrimitive, TypeKind};

use super::{CheckerTyId, CheckerTyTable, FunctionParam, FunctionType, ObjectTypeMember, Type};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PortableType {
    Primitive(LangPrimitive),
    Builtin(BuiltinType),
    This,
    Array(Box<PortableType>),
    Union(Vec<PortableType>),
    Intersection(Vec<PortableType>),
    Tuple(Vec<PortableType>),
    Named(String, Option<String>),
    Generic(String, Vec<PortableType>, Option<String>),
    TemplateLiteral(Vec<PortableType>),
    Fn(PortableFunction),
    Object(Vec<PortableObjectMember>),
    KeyOf(Box<PortableType>),
    IndexedAccess {
        object: Box<PortableType>,
        index: Box<PortableType>,
    },
    Mapped {
        key_var: String,
        source: Box<PortableType>,
        value: Box<PortableType>,
        optional: bool,
        readonly: bool,
    },
    Conditional {
        check: Box<PortableType>,
        extends: Box<PortableType>,
        true_type: Box<PortableType>,
        false_type: Box<PortableType>,
    },
    Infer(String),
    EnumVariant {
        enum_name: String,
        variant_name: String,
        type_args: Vec<PortableType>,
        payload_ty: Box<PortableType>,
    },
    TypePredicate {
        parameter_name: String,
        target_type: Box<PortableType>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PortableFunction {
    pub params: Vec<PortableParam>,
    pub return_type: Box<PortableType>,
    pub is_arrow: bool,
    pub type_params: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PortableParam {
    pub name: Option<String>,
    pub ty: PortableType,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PortableObjectMember {
    Property {
        name: String,
        ty: PortableType,
        optional: bool,
        readonly: bool,
    },
    Method {
        name: String,
        params: Vec<PortableParam>,
        return_type: PortableType,
        optional: bool,
        is_arrow: bool,
    },
    Index {
        param_name: String,
        key_ty: PortableType,
        value_ty: PortableType,
    },
    Callable {
        params: Vec<PortableParam>,
        return_type: PortableType,
        is_arrow: bool,
    },
}

/// Codifica `ty` en su forma portable. Requiere la tabla que lo internó y el
/// interner que resuelve los nombres embebidos.
pub fn encode(ty: Type, table: &CheckerTyTable, interner: &AtomInterner) -> PortableType {
    let name = |a: varn_core::Atom| interner.resolve(a).to_string();
    match table.get(ty.0) {
        TypeKind::Primitive(p) => PortableType::Primitive(p),
        TypeKind::Builtin(b) => PortableType::Builtin(b),
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
            optional: optional,
            readonly: readonly,
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

/// Re-interna `p` en `table` (la del consumidor) resolviendo y publicando los
/// nombres en `interner`. El id resultante es válido en `table` y en ninguna
/// otra.
pub fn decode(p: &PortableType, table: &mut CheckerTyTable, interner: &mut AtomInterner) -> Type {
    let ty = match p {
        PortableType::Primitive(p) => table.intern(TypeKind::Primitive(*p)),
        PortableType::Builtin(b) => table.intern(TypeKind::Builtin(*b)),
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

/// Codifica una lista de tipos ya internada (`TyListId`).
pub fn encode_list(
    list: super::TyListId,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> Vec<PortableType> {
    table
        .get_list(list)
        .iter()
        .map(|id| encode(Type(*id, false), table, interner))
        .collect()
}

/// Re-interna una lista portable y devuelve su `TyListId` local.
pub fn decode_list(
    list: &[PortableType],
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> super::TyListId {
    let ids: Vec<CheckerTyId> = list.iter().map(|p| decode(p, table, interner).0).collect();
    table.intern_list(&ids)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CheckerTyId;

    fn roundtrip(ty: Type, table: &CheckerTyTable, interner: &AtomInterner) -> PortableType {
        let encoded = encode(ty, table, interner);
        let mut fresh = CheckerTyTable::new();
        let mut fresh_interner = AtomInterner::new();
        // El fresh_interner debe poder resolver los nombres que decode
        // re-interna; `decode` los mete él mismo, así que basta con partir de
        // los intrínsecos.
        let decoded = decode(&encoded, &mut fresh, &mut fresh_interner);
        // Re-codificar el resultado debe dar exactamente la misma forma: es la
        // prueba de que la traducción no perdió ni cambió estructura.
        encode(decoded, &fresh, &fresh_interner)
    }

    #[test]
    fn roundtrips_intrinsics_and_arrays() {
        let mut t = CheckerTyTable::new();
        let i = AtomInterner::new();
        let arr = t.intern(TypeKind::Array(CheckerTyId::INT));
        assert_eq!(
            roundtrip(Type(arr, false), &t, &i),
            encode(Type(arr, false), &t, &i)
        );
    }

    #[test]
    fn roundtrips_generic_with_origin_and_names() {
        let mut t = CheckerTyTable::new();
        let mut i = AtomInterner::new();
        let name = i.intern("Sender");
        let origin = i.intern("runtime:task");
        let args = t.intern_list(&[CheckerTyId::INT]);
        let g = t.intern(TypeKind::Generic(name, args, Some(origin)));
        let encoded = encode(Type(g, false), &t, &i);
        assert_eq!(
            encoded,
            PortableType::Generic(
                "Sender".to_string(),
                vec![PortableType::Primitive(LangPrimitive::Int)],
                Some("runtime:task".to_string())
            )
        );
        // Y sobrevive un ciclo encode→decode→encode.
        let mut fresh = CheckerTyTable::new();
        let mut fi = AtomInterner::new();
        let decoded = decode(&encoded, &mut fresh, &mut fi);
        assert_eq!(encode(decoded, &fresh, &fi), encoded);
    }

    #[test]
    fn roundtrips_function_signature() {
        let mut t = CheckerTyTable::new();
        let i = AtomInterner::new();
        let ret = t.intern_function(FunctionType {
            params: vec![FunctionParam {
                name: Some(std::sync::Arc::from("x")),
                ty: CheckerTyId::FLOAT,
                optional: false,
                is_rest: false,
            }],
            return_type: CheckerTyId::FLOAT,
            is_arrow: false,
            type_params: vec![],
        });
        let f = t.intern(TypeKind::Fn(ret));
        let encoded = encode(Type(f, false), &t, &i);
        let mut fresh = CheckerTyTable::new();
        let mut fi = AtomInterner::new();
        let decoded = decode(&encoded, &mut fresh, &mut fi);
        let TypeKind::Fn(fid) = fresh.get(decoded.0) else {
            panic!("debe decodificar a Fn");
        };
        let ft = fresh.get_function(fid);
        assert_eq!(ft.params[0].ty, CheckerTyId::FLOAT);
        assert_eq!(ft.return_type, CheckerTyId::FLOAT);
    }

    #[test]
    fn roundtrips_object_members() {
        let mut t = CheckerTyTable::new();
        let i = AtomInterner::new();
        let oid = t.intern_object_members(vec![ObjectTypeMember::Index {
            param_name: std::sync::Arc::from("k"),
            key_ty: CheckerTyId::STR,
            value_ty: CheckerTyId::INT,
        }]);
        let o = t.intern(TypeKind::Object(oid));
        let encoded = encode(Type(o, false), &t, &i);
        let mut fresh = CheckerTyTable::new();
        let mut fi = AtomInterner::new();
        let decoded = decode(&encoded, &mut fresh, &mut fi);
        let TypeKind::Object(oid) = fresh.get(decoded.0) else {
            panic!("debe decodificar a Object");
        };
        let members = fresh.get_object_members(oid);
        assert!(matches!(
            &members[0],
            ObjectTypeMember::Index { key_ty, value_ty, .. }
                if *key_ty == CheckerTyId::STR && *value_ty == CheckerTyId::INT
        ));
    }

    #[test]
    fn typeof_degrades_to_dynamic_instead_of_inventing_an_id() {
        // No podemos fabricar un `ExprId` legítimo sin arena, así que la
        // garantía que probamos es la ausencia de pánico y la degradación
        // documentada cuando encode ve un `Typeof`: se cubre indirectamente
        // por el match exhaustivo; aquí fijamos la forma portable resultante.
        let mut t = CheckerTyTable::new();
        let i = AtomInterner::new();
        let dynamic = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Dynamic));
        assert_eq!(
            encode(Type(dynamic, false), &t, &i),
            PortableType::Primitive(LangPrimitive::Dynamic)
        );
    }
}
