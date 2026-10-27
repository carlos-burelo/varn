use super::Checker;
use rustc_hash::FxHashMap;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::output::{ResolvedShape, ShapeKind};
use varn_sem::types::{ClassMemberInfo, FunctionType, ObjectTypeMember, TypeContext};
use varn_sem::types::{ClassMemberKind, Type};

impl Checker<'_> {
    pub(super) fn collect_nominal_shapes(
        &mut self,
        bind: &BindResult,
        roots: &[Type],
    ) -> Vec<ResolvedShape> {
        let mut roots = roots.to_vec();
        roots.sort_by_key(|t| t.0);
        roots.dedup_by_key(|t| t.0);
        let mut pending: VecDeque<_> = roots.into_iter().map(|t| (t, 0)).collect();
        let mut seen = BTreeSet::new();
        let mut shapes = BTreeMap::new();
        while let Some((ty, depth)) = pending.pop_front() {
            if !seen.insert(ty.0) {
                continue;
            }
            if let Some(target) = self.nominal_alias_target(bind, ty) {
                pending.push_back((target, depth + 1));
            }
            pending.extend(
                ty.components(&self.ty_table)
                    .into_iter()
                    .map(|t| (t, depth)),
            );
            let Some(shape) = self.resolve_nominal_shape(bind, ty, &mut BTreeSet::new()) else {
                continue;
            };
            if depth >= 64 {
                self.emit(Diagnostic::error(
                    ErrorCode::InfiniteTypeExpansion,
                    format!("type '{}' expands without a finite shape", shape.name),
                ));
                break;
            }
            pending.extend(shape.fields.iter().map(|f| (f.ty, depth + 1)));
            shapes.insert(ty.0, shape);
        }
        shapes.into_values().collect()
    }

    fn resolve_nominal_shape(
        &mut self,
        bind: &BindResult,
        ty: Type,
        parents: &mut BTreeSet<varn_sem::types::CheckerTyId>,
    ) -> Option<ResolvedShape> {
        if let Some(target) = self.nominal_alias_target(bind, ty) {
            if parents.len() >= 64 || !parents.insert(ty.0) {
                return None;
            }
            let mut shape = match self.ty_table.get(target.0) {
                TypeKind::Object(id) | TypeKind::Record(id) => {
                    let kind = if matches!(self.ty_table.get(target.0), TypeKind::Record(_)) {
                        ShapeKind::Record
                    } else {
                        ShapeKind::Object
                    };
                    let mut fields = Vec::new();
                    for member in self.ty_table.get_object_members(id).to_vec() {
                        let field = match member {
                            ObjectTypeMember::Property {
                                name,
                                ty,
                                optional,
                                readonly,
                            } => ClassMemberInfo {
                                name,
                                ty: Type::resolved(ty),
                                is_optional: optional,
                                is_readonly: readonly,
                                kind: ClassMemberKind::Property,
                                ..ClassMemberInfo::default()
                            },
                            ObjectTypeMember::Method {
                                name,
                                params,
                                return_type,
                                optional,
                                is_arrow,
                                type_params,
                            } => {
                                let ty = Type::fn_(
                                    FunctionType {
                                        params,
                                        return_type,
                                        is_arrow,
                                        type_params,
                                    },
                                    Arc::make_mut(&mut self.ty_table),
                                );
                                ClassMemberInfo {
                                    name,
                                    ty,
                                    is_optional: optional,
                                    kind: ClassMemberKind::Method,
                                    ..ClassMemberInfo::default()
                                }
                            }
                            ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => {
                                return None
                            }
                        };
                        fields.push(field);
                    }
                    Some(ResolvedShape {
                        ty,
                        kind,
                        fields,
                        name: Arc::from(
                            ty.display(&self.ty_table, self.ty_table.names())
                                .to_string(),
                        ),
                    })
                }
                TypeKind::Named(..) | TypeKind::Generic(..) => {
                    self.resolve_nominal_shape(bind, target, parents)
                }
                TypeKind::Primitive(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Fn(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => None,
            };
            parents.remove(&ty.0);
            if let Some(shape) = &mut shape {
                shape.ty = ty;
                shape.name = Arc::from(
                    ty.display(&self.ty_table, self.ty_table.names())
                        .to_string(),
                );
            }
            return shape;
        }
        let (name, origin, args) = match self.ty_table.get(ty.0) {
            TypeKind::Named(name, origin) => (name, origin, Vec::new()),
            TypeKind::Generic(name, list, origin) => {
                (name, origin, self.ty_table.get_list(list).to_vec())
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
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
            | TypeKind::TypePredicate { .. } => return None,
        };
        if varn_core::is_lang_type(name, origin) {
            return None;
        }
        if origin.is_none()
            && bind
                .arena
                .all()
                .iter()
                .any(|s| s.name == name && s.kind == varn_sem::symbol::SymbolKind::TypeParameter)
        {
            return None;
        }
        let name = self.resolve_bind_atom(bind, name);
        let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
        let foreign = match origin
            .as_deref()
            .filter(|o| *o != bind.source_file.as_ref())
        {
            Some(origin) => Some(
                self.resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))?,
            ),
            None => None,
        };
        let owner = foreign.as_deref().unwrap_or(bind);
        let mut fields = match owner.get_interface_members_local(&name) {
            Some(fields) => fields.clone(),
            None if owner.classify_decl(&name) == Some(varn_sem::bind::DeclKind::Interface) => {
                Vec::new()
            }
            None => return None,
        };
        if parents.len() >= 64 || !parents.insert(ty.0) {
            self.emit(Diagnostic::error(
                ErrorCode::CircularTypeReference,
                format!("interface '{name}' has circular inheritance"),
            ));
            return None;
        }
        let atom = owner.interner.get(&name)?;
        let sid = owner
            .scopes
            .get(owner.global_scope)
            .resolve(atom, &owner.scopes);
        let params = sid
            .map(|sid| owner.arena.get(sid).type_params.clone())
            .unwrap_or_default();
        let mapping: FxHashMap<_, _> = params
            .into_iter()
            .zip(args.into_iter().map(Type::resolved))
            .collect();
        for field in &mut fields {
            if foreign.is_some() {
                field.ty = self.reintern_foreign_ty(owner, field.ty);
            }
            field.ty = field
                .ty
                .map_generics(&mapping, Arc::make_mut(&mut self.ty_table));
        }
        fields.retain(|f| {
            !f.is_static
                && matches!(
                    f.kind,
                    ClassMemberKind::Property
                        | ClassMemberKind::Method
                        | ClassMemberKind::Getter
                        | ClassMemberKind::Setter
                )
        });
        let mut inherited = Vec::new();
        for parent in owner.get_class_interfaces(&name) {
            let mut args = Vec::new();
            for arg in &parent.args {
                let arg = if foreign.is_some() {
                    self.reintern_foreign_ty(owner, *arg)
                } else {
                    *arg
                };
                args.push(arg.map_generics(&mapping, Arc::make_mut(&mut self.ty_table)));
            }
            let origin = parent
                .origin
                .clone()
                .or_else(|| Some(Arc::from(owner.source_file.as_ref())));
            let table = Arc::make_mut(&mut self.ty_table);
            let parent_ty = if args.is_empty() {
                Type::named_with_origin(parent.name.clone(), origin, table)
            } else {
                Type::generic_with_origin(parent.name.clone(), args, origin, table)
            };
            if let Some(shape) = self.resolve_nominal_shape(bind, parent_ty, parents) {
                for field in shape.fields {
                    if !fields
                        .iter()
                        .chain(inherited.iter())
                        .any(|f: &varn_sem::types::ClassMemberInfo| f.name == field.name)
                    {
                        inherited.push(field);
                    }
                }
            }
        }
        fields.extend(inherited);
        parents.remove(&ty.0);
        Some(ResolvedShape {
            ty,
            kind: ShapeKind::Interface,
            name: Arc::from(
                ty.display(&self.ty_table, self.ty_table.names())
                    .to_string(),
            ),
            fields,
        })
    }
    fn nominal_alias_target(&mut self, bind: &BindResult, ty: Type) -> Option<Type> {
        let (name, origin, args) = match self.ty_table.get(ty.0) {
            TypeKind::Named(name, origin) => (name, origin, Vec::new()),
            TypeKind::Generic(name, list, origin) => {
                (name, origin, self.ty_table.get_list(list).to_vec())
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
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
            | TypeKind::TypePredicate { .. } => return None,
        };
        if origin.is_none()
            && bind
                .arena
                .all()
                .iter()
                .any(|s| s.name == name && s.kind == varn_sem::symbol::SymbolKind::TypeParameter)
        {
            return None;
        }
        let name = self.resolve_bind_atom(bind, name);
        let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
        let view = varn_sem::bind::BindView::new(bind, self.resolver);
        let mut target = view.resolve_type_alias(&name, origin.as_deref())?;
        let params = view
            .type_param_names(&name, origin.as_deref())
            .unwrap_or_default();
        if let Some(origin) = origin
            .as_deref()
            .filter(|o| *o != bind.source_file.as_ref())
        {
            let owner = self
                .resolver
                .module_bind(origin)
                .or_else(|| self.resolver.stdlib_bind(origin))?;
            target = self.reintern_foreign_ty(&owner, target);
        }
        let mapping = params
            .into_iter()
            .map(|p| varn_core::Atom::of(&p))
            .zip(args.into_iter().map(Type::resolved))
            .collect();
        Some(target.map_generics(&mapping, Arc::make_mut(&mut self.ty_table)))
    }
}
