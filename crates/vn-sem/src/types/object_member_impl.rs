use super::*;

impl ObjectTypeMember {
    pub fn map_generics(
        &self,
        mapping: &FxHashMap<vn_core::Atom, Type>,
        table: &mut CheckerTyTable,
    ) -> Self {
        self.map_types(table, &mut |ty, table| ty.map_generics(mapping, table))
    }

    pub fn map_types(
        &self,
        table: &mut CheckerTyTable,
        f: &mut dyn FnMut(Type, &mut CheckerTyTable) -> Type,
    ) -> Self {
        let mut params_of = |params: &[FunctionParam], table: &mut CheckerTyTable| {
            params
                .iter()
                .map(|p| FunctionParam {
                    name: p.name.clone(),
                    ty: f(Type::resolved(p.ty), table).0,
                    optional: p.optional,
                    is_rest: p.is_rest,
                })
                .collect::<Vec<_>>()
        };
        match self {
            ObjectTypeMember::Property {
                name,
                ty,
                optional,
                readonly,
            } => ObjectTypeMember::Property {
                name: name.clone(),
                ty: f(Type::resolved(*ty), table).0,
                optional: *optional,
                readonly: *readonly,
            },
            ObjectTypeMember::Method {
                name,
                params,
                return_type,
                optional,
                is_arrow,
                type_params,
            } => {
                let params = params_of(params, table);
                ObjectTypeMember::Method {
                    name: name.clone(),
                    params,
                    return_type: f(Type::resolved(*return_type), table).0,
                    optional: *optional,
                    is_arrow: *is_arrow,
                    type_params: type_params.clone(),
                }
            }
            ObjectTypeMember::Callable {
                params,
                return_type,
                is_arrow,
            } => {
                let params = params_of(params, table);
                ObjectTypeMember::Callable {
                    params,
                    return_type: f(Type::resolved(*return_type), table).0,
                    is_arrow: *is_arrow,
                }
            }
        }
    }
}
