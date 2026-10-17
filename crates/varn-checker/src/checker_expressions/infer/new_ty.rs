use super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::ExprId;
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn infer_new_type(
        &mut self,
        rec: &mut Recorder,
        callee: ExprId,
        type_args: &[varn_core::ast::TypeNode],
        ctor_args: &[varn_core::ast::Arg],
        bind: &BindResult,
    ) -> Type {
        let callee_ty = self.infer_type(rec, callee, bind);
        if callee_ty.is_dynamic() {
            return Type::Dynamic;
        }
        let callee_kind = self.ty_table.get(callee_ty.0);
        match callee_kind {
            TypeKind::Named(name, origin) => {
                let name_str = bind.interner.resolve(name).to_string();
                let origin_str = origin.map(|o| std::sync::Arc::from(bind.interner.resolve(o)));
                if !type_args.is_empty() {
                    let args: Vec<Type> = type_args
                        .iter()
                        .map(|a| {
                            self.check_type_names(a, bind);
                            self.resolve_type_node_cached(a, bind)
                        })
                        .collect();
                    Type::generic_with_origin(
                        name_str,
                        args,
                        origin_str,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                } else if let Some(contextual) = self.expected_instantiation(name) {
                    contextual
                } else if let Some(args) = self.instantiation_from_args(
                    rec,
                    callee,
                    &name_str,
                    origin_str.as_ref(),
                    ctor_args,
                    bind,
                ) {
                    Type::generic_with_origin(
                        name_str,
                        args,
                        origin_str,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                } else {
                    Type::named_with_origin(
                        name_str,
                        origin_str,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            }
            TypeKind::Generic(name, args, origin) => {
                let name_str = bind.interner.resolve(name).to_string();
                let origin_str = origin.map(|o| std::sync::Arc::from(bind.interner.resolve(o)));
                let arg_ids = self.ty_table.get_list(args).to_vec();
                let args: Vec<Type> = arg_ids.into_iter().map(Type::resolved).collect();
                Type::generic_with_origin(
                    name_str,
                    args,
                    origin_str,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
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
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => {
                if let varn_core::ast::ExprKind::Identifier { name } =
                    &self.ast_arena.expr(callee).kind
                {
                    let name_str = bind.interner.resolve(*name).to_string();
                    let view =
                        varn_sem::bind::BindView::new(bind, self.resolver).at(self.current_scope);
                    let origin = varn_binder::nominal_origin(
                        &name_str,
                        Some(&view),
                        std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let origin_str = origin.map(|o| self.resolve_bind_atom(bind, o));
                    if !type_args.is_empty() {
                        let args: Vec<Type> = type_args
                            .iter()
                            .map(|a| {
                                self.check_type_names(a, bind);
                                self.resolve_type_node_cached(a, bind)
                            })
                            .collect();
                        Type::generic_atom(
                            *name,
                            args,
                            origin,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        )
                    } else if let Some(contextual) = self.expected_instantiation(*name) {
                        contextual
                    } else if let Some(args) = self.instantiation_from_args(
                        rec,
                        callee,
                        &name_str,
                        origin_str.as_ref(),
                        ctor_args,
                        bind,
                    ) {
                        Type::generic_atom(
                            *name,
                            args,
                            origin,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        )
                    } else {
                        Type::named_with_origin_atom(
                            *name,
                            origin,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        )
                    }
                } else {
                    Type::Dynamic
                }
            }
        }
    }

    fn instantiation_from_args(
        &mut self,
        rec: &mut Recorder,
        callee: ExprId,
        class: &str,
        origin: Option<&std::sync::Arc<str>>,
        ctor_args: &[varn_core::ast::Arg],
        bind: &BindResult,
    ) -> Option<Vec<Type>> {
        use crate::checker_expressions::members::member_util::class_type_params;
        use varn_sem::bind::TypeContext;
        let type_params = class_type_params(self.resolver, class, origin, bind);
        if type_params.is_empty() {
            return None;
        }
        let ctor = varn_sem::bind::BindView::new(bind, self.resolver)
            .get_class_members(class, origin.map(|o| o.as_ref()))
            .and_then(|members| {
                members
                    .iter()
                    .find(|m| m.kind == varn_sem::types::ClassMemberKind::Constructor)
                    .map(|m| m.ty)
            });
        let params = match ctor.map(|ty| self.ty_table.get(ty.0)) {
            Some(TypeKind::Fn(fid)) => self.ty_table.get_function(fid).params.clone(),
            Some(_) | None => Vec::new(),
        };
        let mapping = crate::checker_generics::infer_mapping_from_args(
            &type_params,
            &params,
            ctor_args,
            self,
            rec,
            bind,
        );
        let defaults = self.declared_type_defaults(callee, bind);
        type_params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                mapping
                    .get(p)
                    .copied()
                    .or_else(|| defaults.get(i).copied().flatten())
            })
            .collect()
    }

    fn declared_type_defaults(&self, callee: ExprId, bind: &BindResult) -> Vec<Option<Type>> {
        let varn_core::ast::ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind
        else {
            return Vec::new();
        };
        bind.scopes
            .get(bind.global_scope)
            .resolve(*name, &bind.scopes)
            .map(|sid| bind.arena.get(sid).type_param_defaults.clone())
            .unwrap_or_default()
    }

    fn expected_instantiation(&mut self, name: varn_core::Atom) -> Option<Type> {
        let expected = self
            .expected_type?
            .non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        match self.ty_table.get(expected.0) {
            TypeKind::Generic(expected_name, _, _) if expected_name == name => Some(expected),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        }
    }
}
