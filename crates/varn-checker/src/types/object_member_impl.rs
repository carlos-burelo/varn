use super::*;

impl ObjectTypeMember {
    pub fn map_generics(
        &self,
        mapping: &FxHashMap<varn_core::Atom, Type>,
        table: &mut CheckerTyTable,
    ) -> Self {
        match self {
            ObjectTypeMember::Property {
                name,
                ty,
                optional,
                readonly,
            } => ObjectTypeMember::Property {
                name: name.clone(),
                ty: Type(*ty, false).map_generics(mapping, table).0,
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
                        ty: Type(p.ty, false).map_generics(mapping, table).0,
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect(),
                return_type: Type(*return_type, false).map_generics(mapping, table).0,
                optional: *optional,
                is_arrow: *is_arrow,
            },
            ObjectTypeMember::Index {
                param_name,
                key_ty,
                value_ty,
            } => ObjectTypeMember::Index {
                param_name: param_name.clone(),
                key_ty: Type(*key_ty, false).map_generics(mapping, table).0,
                value_ty: Type(*value_ty, false).map_generics(mapping, table).0,
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
                        ty: Type(p.ty, false).map_generics(mapping, table).0,
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect(),
                return_type: Type(*return_type, false).map_generics(mapping, table).0,
                is_arrow: *is_arrow,
            },
        }
    }
}
