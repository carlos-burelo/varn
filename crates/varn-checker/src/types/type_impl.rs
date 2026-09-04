use super::*;
use std::rc::Rc;

#[allow(non_upper_case_globals)]
impl Type {
    pub const Int: Type = Type(TypeKind::Intrinsic(TypeTag::Int), false);
    pub const Float: Type = Type(TypeKind::Intrinsic(TypeTag::Float), false);
    pub const Decimal: Type = Type(TypeKind::Intrinsic(TypeTag::Decimal), false);
    pub const BigInt: Type = Type(TypeKind::Intrinsic(TypeTag::BigInt), false);
    pub const Str: Type = Type(TypeKind::Intrinsic(TypeTag::Str), false);
    pub const Char: Type = Type(TypeKind::Intrinsic(TypeTag::Char), false);
    pub const Bool: Type = Type(TypeKind::Intrinsic(TypeTag::Bool), false);
    pub const Symbol: Type = Type(TypeKind::Intrinsic(TypeTag::Symbol), false);
    pub const Void: Type = Type(TypeKind::Intrinsic(TypeTag::Void), false);
    pub const Null: Type = Type(TypeKind::Intrinsic(TypeTag::Null), false);
    pub const Never: Type = Type(TypeKind::Intrinsic(TypeTag::Never), false);
    pub const Dynamic: Type = Type(TypeKind::Intrinsic(TypeTag::Dynamic), false);
    pub const This: Type = Type(TypeKind::This, false);
    pub fn intrinsic(tag: TypeTag) -> Self {
        Type(TypeKind::Intrinsic(tag), false)
    }

    pub fn get_array_element_type(&self) -> Type {
        match &self.0 {
            TypeKind::Array(inner) => (**inner).clone(),
            _ => Type::Dynamic,
        }
    }

    pub fn fn_(f: FunctionType) -> Self {
        Type(TypeKind::Fn(f), false)
    }

    pub fn named(name: impl Into<Rc<str>>) -> Self {
        Type(TypeKind::Named(name.into(), None), false)
    }

    pub fn named_with_origin(name: impl Into<Rc<str>>, origin: Option<impl Into<Rc<str>>>) -> Self {
        Type(
            TypeKind::Named(name.into(), origin.map(|o| o.into())),
            false,
        )
    }

    pub fn array(inner: Type) -> Self {
        Type(TypeKind::Array(Box::new(inner)), false)
    }

    pub fn generic(name: impl Into<Rc<str>>, args: Vec<Type>) -> Self {
        Type(TypeKind::Generic(name.into(), args, None), false)
    }

    pub fn generic_with_origin(
        name: impl Into<Rc<str>>,
        args: Vec<Type>,
        origin: Option<impl Into<Rc<str>>>,
    ) -> Self {
        Type(
            TypeKind::Generic(name.into(), args, origin.map(|o| o.into())),
            false,
        )
    }

    pub fn object(members: Vec<ObjectTypeMember>) -> Self {
        Type(TypeKind::Object(members), false)
    }

    pub fn union(members: Vec<Type>) -> Self {
        if members.len() == 1 {
            if !matches!(members[0].0, TypeKind::Union(_)) {
                return members.into_iter().next().unwrap();
            }
        } else if members.len() == 2 {
            if !matches!(members[0].0, TypeKind::Union(_))
                && !matches!(members[1].0, TypeKind::Union(_))
            {
                if members[0] == members[1] {
                    return members.into_iter().next().unwrap();
                } else {
                    return Type(TypeKind::Union(members), false);
                }
            }
        } else if members.is_empty() {
            return Type(TypeKind::Union(members), false);
        }

        let mut seen = rustc_hash::FxHashSet::default();
        let mut flat: Vec<Type> = Vec::with_capacity(members.len());
        for m in members {
            match m.0 {
                TypeKind::Union(inner) => {
                    for t in inner {
                        if seen.insert(t.clone()) {
                            flat.push(t);
                        }
                    }
                }
                _ => {
                    if seen.insert(m.clone()) {
                        flat.push(m);
                    }
                }
            }
        }
        if flat.len() == 1 {
            flat.remove(0)
        } else {
            Type(TypeKind::Union(flat), false)
        }
    }

    pub fn is_dynamic(&self) -> bool {
        matches!(&self.0, TypeKind::Intrinsic(TypeTag::Dynamic))
    }

    pub fn stdlib_key(&self) -> Option<&'static str> {
        use varn_core::IntrinsicType as I;
        match &self.0 {
            TypeKind::Intrinsic(tag) => match tag {
                TypeTag::Int => Some(I::Int.as_str()),
                TypeTag::Float => Some(I::Float.as_str()),
                TypeTag::Decimal => Some(I::Decimal.as_str()),
                TypeTag::BigInt => Some(I::BigInt.as_str()),
                TypeTag::Str => Some(I::Str.as_str()),
                TypeTag::Char => Some(I::Char.as_str()),
                TypeTag::Bool => Some(I::Bool.as_str()),
                TypeTag::Symbol => Some(I::Symbol.as_str()),
                _ => None,
            },
            TypeKind::Array(_) => Some(I::Array.as_str()),
            _ => None,
        }
    }

    pub fn descriptor_key(&self) -> Option<&str> {
        if let Some(k) = self.stdlib_key() {
            return Some(k);
        }
        match &self.0 {
            TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(n.as_ref()),
            _ => None,
        }
    }

    pub fn is_int(&self) -> bool {
        matches!(self.0, TypeKind::Intrinsic(TypeTag::Int))
    }
    pub fn is_float(&self) -> bool {
        matches!(self.0, TypeKind::Intrinsic(TypeTag::Float))
    }
    pub fn is_str(&self) -> bool {
        matches!(self.0, TypeKind::Intrinsic(TypeTag::Str))
    }
    pub fn is_bool(&self) -> bool {
        matches!(self.0, TypeKind::Intrinsic(TypeTag::Bool))
    }
    pub fn is_void(&self) -> bool {
        matches!(self.0, TypeKind::Intrinsic(TypeTag::Void))
    }

    pub fn to_type_tag(&self) -> TypeTag {
        match &self.0 {
            TypeKind::Intrinsic(tag) => *tag,
            TypeKind::Array(_) => TypeTag::Array,
            TypeKind::Object(_) => TypeTag::Object,
            TypeKind::Named(..) => TypeTag::Class,
            TypeKind::Generic(..) => TypeTag::Class,
            TypeKind::Fn(_) => TypeTag::Function,
            TypeKind::Tuple(_) => TypeTag::Tuple,
            _ => TypeTag::Dynamic,
        }
    }

    pub fn is_nullable(&self) -> bool {
        match &self.0 {
            TypeKind::Intrinsic(TypeTag::Null) => true,
            TypeKind::Union(members) => members.iter().any(|m| m.is_nullable()),
            _ => false,
        }
    }

    pub fn make_nullable(ty: Type) -> Type {
        if ty.is_nullable() {
            ty
        } else {
            Type::union(vec![ty, Type::Null])
        }
    }

    pub fn non_nullified(&self) -> Type {
        match &self.0 {
            TypeKind::Intrinsic(TypeTag::Null) => Type::Never,
            TypeKind::Union(members) => {
                let mut new_members: Vec<Type> = members
                    .iter()
                    .filter(|m| !m.is_nullable())
                    .cloned()
                    .collect();

                if new_members.is_empty() {
                    Type::Never
                } else if new_members.len() == 1 {
                    new_members.pop().unwrap()
                } else {
                    Type(TypeKind::Union(new_members), false)
                }
            }
            _ => self.clone(),
        }
    }

    pub fn minus_named(&self, name: &str) -> Type {
        match &self.0 {
            TypeKind::Named(n, _) if n.as_ref() == name => Type::Never,
            TypeKind::Union(members) => {
                let mut kept: Vec<Type> = members
                    .iter()
                    .filter(|m| !matches!(&m.0, TypeKind::Named(n, _) if n.as_ref() == name))
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    kept.pop().unwrap()
                } else {
                    Type(TypeKind::Union(kept), false)
                }
            }
            _ => self.clone(),
        }
    }

    pub fn minus(&self, other: &Type) -> Type {
        if self == other {
            return Type::Never;
        }
        match &self.0 {
            TypeKind::Union(members) => {
                let mut kept: Vec<Type> = members.iter().filter(|m| *m != other).cloned().collect();
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    kept.pop().unwrap()
                } else {
                    Type(TypeKind::Union(kept), false)
                }
            }
            _ => self.clone(),
        }
    }

    pub fn map_generics(&self, mapping: &FxHashMap<Rc<str>, Type>) -> Type {
        match &self.0 {
            TypeKind::Named(n, _) => {
                if let Some(t) = mapping.get(n) {
                    return t.clone();
                }
                self.clone()
            }
            TypeKind::Generic(n, args, origin) => {
                let new_args: Vec<Type> = args.iter().map(|a| a.map_generics(mapping)).collect();
                Type(
                    TypeKind::Generic(n.clone(), new_args, origin.clone()),
                    self.1,
                )
            }
            TypeKind::Array(inner) => Type::array(inner.map_generics(mapping)),
            TypeKind::Union(members) => {
                let new_members: Vec<Type> =
                    members.iter().map(|m| m.map_generics(mapping)).collect();
                Type::union(new_members)
            }
            TypeKind::Fn(ft) => {
                let new_params: Vec<FunctionParam> = ft
                    .params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: p.ty.map_generics(mapping),
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect();
                let new_ret = ft.return_type.map_generics(mapping);
                Type::fn_(FunctionType {
                    params: new_params,
                    return_type: Box::new(new_ret),
                    is_arrow: ft.is_arrow,
                    type_params: ft.type_params.clone(),
                })
            }
            TypeKind::Object(members) => {
                let new_members: Vec<ObjectTypeMember> = members
                    .iter()
                    .map(|m| match m {
                        ObjectTypeMember::Property {
                            name,
                            ty,
                            optional,
                            readonly,
                        } => ObjectTypeMember::Property {
                            name: name.clone(),
                            ty: ty.map_generics(mapping),
                            optional: *optional,
                            readonly: *readonly,
                        },
                        ObjectTypeMember::Method {
                            name,
                            params,
                            return_type,
                            optional,
                            is_arrow,
                        } => {
                            let new_params: Vec<FunctionParam> = params
                                .iter()
                                .map(|p| FunctionParam {
                                    name: p.name.clone(),
                                    ty: p.ty.map_generics(mapping),
                                    optional: p.optional,
                                    is_rest: p.is_rest,
                                })
                                .collect();
                            ObjectTypeMember::Method {
                                name: name.clone(),
                                params: new_params,
                                return_type: Box::new(return_type.map_generics(mapping)),
                                optional: *optional,
                                is_arrow: *is_arrow,
                            }
                        }
                        ObjectTypeMember::Index {
                            param_name,
                            key_ty,
                            value_ty,
                        } => ObjectTypeMember::Index {
                            param_name: param_name.clone(),
                            key_ty: Box::new(key_ty.map_generics(mapping)),
                            value_ty: Box::new(value_ty.map_generics(mapping)),
                        },
                        ObjectTypeMember::Callable {
                            params,
                            return_type,
                            is_arrow,
                        } => {
                            let new_params: Vec<FunctionParam> = params
                                .iter()
                                .map(|p| FunctionParam {
                                    name: p.name.clone(),
                                    ty: p.ty.map_generics(mapping),
                                    optional: p.optional,
                                    is_rest: p.is_rest,
                                })
                                .collect();
                            ObjectTypeMember::Callable {
                                params: new_params,
                                return_type: Box::new(return_type.map_generics(mapping)),
                                is_arrow: *is_arrow,
                            }
                        }
                    })
                    .collect();
                Type::object(new_members)
            }
            _ => self.clone(),
        }
    }

    pub fn with_origin(mut self, origin: Rc<str>) -> Self {
        match &mut self.0 {
            TypeKind::Named(_, ref mut o) => *o = Some(origin),
            TypeKind::Generic(_, _, ref mut o) => *o = Some(origin),
            _ => {}
        }
        self
    }
}
