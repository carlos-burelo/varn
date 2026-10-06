use super::*;

impl Type {
    pub fn is_bytes(&self, table: &CheckerTyTable) -> bool {
        matches!(
            table.get(self.0),
            TypeKind::Builtin(varn_core::BuiltinType::Bytes)
        )
    }

    pub fn is_nullable(&self, table: &CheckerTyTable) -> bool {
        match table.get(self.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Null) => true,
            TypeKind::Union(list) => table
                .get_list(list)
                .iter()
                .any(|m| Type::resolved(*m).is_nullable(table)),
            _ => false,
        }
    }

    pub fn make_nullable(ty: Type, table: &mut CheckerTyTable) -> Type {
        if ty.is_nullable(table) {
            ty
        } else {
            Type::union(vec![ty, Type::Null], table)
        }
    }

    pub fn non_nullified(&self, table: &mut CheckerTyTable) -> Type {
        match table.get(self.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Null) => Type::Never,
            TypeKind::Union(list) => {
                let members: Vec<Type> = table
                    .get_list(list)
                    .iter()
                    .map(|id| Type::resolved(*id))
                    .collect();
                let new_members: Vec<Type> = members
                    .into_iter()
                    .filter(|m| !m.is_nullable(table))
                    .collect();

                if new_members.is_empty() {
                    Type::Never
                } else if new_members.len() == 1 {
                    new_members.into_iter().next().unwrap()
                } else {
                    let ids: Vec<CheckerTyId> = new_members.iter().map(|m| m.0).collect();
                    let new_list = table.intern_list(&ids);
                    Type::resolved(table.intern(TypeKind::Union(new_list)))
                }
            }
            _ => *self,
        }
    }

    pub fn minus_named(&self, name: varn_core::Atom, table: &mut CheckerTyTable) -> Type {
        match table.get(self.0) {
            TypeKind::Named(n, _) if n == name => Type::Never,
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(list).to_vec();
                let mut kept: Vec<CheckerTyId> = Vec::with_capacity(members.len());
                for id in members {
                    let is_named_match =
                        matches!(table.get(id), TypeKind::Named(n, _) if n == name);
                    if !is_named_match {
                        kept.push(id);
                    }
                }
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    Type::resolved(kept[0])
                } else {
                    let new_list = table.intern_list(&kept);
                    Type::resolved(table.intern(TypeKind::Union(new_list)))
                }
            }
            _ => *self,
        }
    }

    pub fn minus(&self, other: &Type, table: &mut CheckerTyTable) -> Type {
        if self == other {
            return Type::Never;
        }
        match table.get(self.0) {
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(list).to_vec();
                let dynamic_array = {
                    let arr = Type::array(Type::Dynamic, table);
                    arr.0
                };
                let mut kept: Vec<CheckerTyId> = Vec::with_capacity(members.len());
                for id in members {
                    if id == other.0 {
                        continue;
                    }
                    if let (TypeKind::Array(_), TypeKind::Array(_)) =
                        (table.get(id), table.get(other.0))
                    {
                        if other.0 == dynamic_array || id == other.0 {
                            continue;
                        }
                    }
                    kept.push(id);
                }
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    Type::resolved(kept[0])
                } else {
                    let new_list = table.intern_list(&kept);
                    Type::resolved(table.intern(TypeKind::Union(new_list)))
                }
            }
            _ => *self,
        }
    }

    pub fn map_generics(
        &self,
        mapping: &FxHashMap<varn_core::Atom, Type>,
        table: &mut CheckerTyTable,
    ) -> Type {
        match table.get(self.0) {
            TypeKind::Named(n, _) => {
                if let Some(t) = mapping.get(&n) {
                    return *t;
                }
                *self
            }
            TypeKind::Generic(n, args, origin) => {
                let arg_ids = table.get_list(args).to_vec();
                let new_args: Vec<CheckerTyId> = arg_ids
                    .into_iter()
                    .map(|a| Type::resolved(a).map_generics(mapping, table).0)
                    .collect();
                let new_list = table.intern_list(&new_args);
                Type(table.intern(TypeKind::Generic(n, new_list, origin)), self.1)
            }
            TypeKind::Array(inner) => {
                let mapped = Type::resolved(inner).map_generics(mapping, table);
                Type::array(mapped, table)
            }
            TypeKind::Union(list) => {
                let ids = table.get_list(list).to_vec();
                let new_members: Vec<Type> = ids
                    .into_iter()
                    .map(|id| Type::resolved(id).map_generics(mapping, table))
                    .collect();
                Type::union(new_members, table)
            }
            TypeKind::Fn(fid) => {
                let ft = table.get_function(fid).clone();
                let new_params: Vec<FunctionParam> = ft
                    .params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: Type::resolved(p.ty).map_generics(mapping, table).0,
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect();
                let new_ret = Type::resolved(ft.return_type)
                    .map_generics(mapping, table)
                    .0;
                Type::fn_(
                    FunctionType {
                        params: new_params,
                        return_type: new_ret,
                        is_arrow: ft.is_arrow,
                        type_params: ft.type_params.clone(),
                    },
                    table,
                )
            }
            TypeKind::Object(mid) => {
                let members = table.get_object_members(mid).to_vec();
                let new_members: Vec<ObjectTypeMember> = members
                    .into_iter()
                    .map(|m| m.map_generics(mapping, table))
                    .collect();
                Type::object(new_members, table)
            }
            _ => *self,
        }
    }

    pub fn with_origin(self, origin: varn_core::Atom, table: &mut CheckerTyTable) -> Self {
        match table.get(self.0) {
            TypeKind::Named(n, _) => Type(table.intern(TypeKind::Named(n, Some(origin))), self.1),
            TypeKind::Generic(n, args, _) => Type(
                table.intern(TypeKind::Generic(n, args, Some(origin))),
                self.1,
            ),
            _ => self,
        }
    }
}
