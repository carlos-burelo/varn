use super::*;
use std::rc::Rc;

impl ObjectTypeMember {
    pub fn map_generics(&self, mapping: &FxHashMap<Rc<str>, Type>) -> Self {
        match self {
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
            } => ObjectTypeMember::Method {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: p.ty.map_generics(mapping),
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect(),
                return_type: Box::new(return_type.map_generics(mapping)),
                optional: *optional,
                is_arrow: *is_arrow,
            },
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
            } => ObjectTypeMember::Callable {
                params: params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: p.ty.map_generics(mapping),
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect(),
                return_type: Box::new(return_type.map_generics(mapping)),
                is_arrow: *is_arrow,
            },
        }
    }
}
