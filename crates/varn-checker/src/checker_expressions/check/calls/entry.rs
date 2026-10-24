use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use crate::checker_generics::{build_call_mapping, check_call_type_arg_arity};
use crate::generic_substitution::map_generics_cached;
use varn_core::ast::{Arg, ExprId, ExprKind, TypeNode};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::{FunctionParam, Type};

impl<'r> Checker<'r> {
    pub(in super::super) fn check_call_expr(
        &mut self,
        rec: &mut Recorder,
        callee: ExprId,
        args: &[Arg],
        type_args: &[TypeNode],
        range: &SourceRange,
        call_id: varn_core::ast::AstId,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        if let ExprKind::Identifier { name } = &arena.expr(callee).kind {
            let name = bind.interner.resolve(*name);
            if varn_core::LangPrimitive::from_str(name).is_some() {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::NotCallable,
                        format!(
                            "'{name}' is a type, not a function: convert with 'value as {name}'"
                        ),
                    )
                    .with_range(*range),
                );
                for arg in args {
                    let (Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. }) = arg;
                    self.check_expr(rec, *e, bind);
                }
                return;
            }
        }
        if matches!(arena.expr(callee).kind, ExprKind::Super) {
            self.check_super_call(rec, args, range, call_id, bind);
        }
        self.callee_position = true;
        self.check_expr(rec, callee, bind);
        self.callee_position = false;
        self.record_extension_call(rec, callee, range, bind);

        let callee_ty_raw = self.infer_type(rec, callee, bind);
        let callee_ty =
            callee_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let callee_kind = self.ty_table.get(callee_ty.0);

        let callee_sid: Option<usize> = match &arena.expr(callee).kind {
            ExprKind::Identifier { name } => bind
                .scopes
                .get(self.current_scope)
                .resolve(*name, &bind.scopes),
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => {
                if let ExprKind::Identifier { name: prop } = &arena.expr(*property).kind {
                    let prop_name = bind.interner.resolve(*prop);
                    let obj_ty = self.infer_type(rec, *object, bind);
                    self.find_member_info(&obj_ty, prop_name, bind)
                        .and_then(|(_, sid)| {
                            sid.filter(|s| {
                                *s < bind.arena.len()
                                    && bind.interner.get(prop_name) == Some(bind.arena.get(*s).name)
                            })
                        })
                } else {
                    None
                }
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => None,
        };
        self.check_pure_callee(callee_sid, *range, bind);
        self.check_capability_callee(callee_sid, *range, bind);

        let holds_value = self.callee_is_value_binding(callee, bind);
        if holds_value && !self.is_callable(callee_kind, bind) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotCallable,
                    format!(
                        "type '{}' is not callable",
                        callee_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(*range),
            );
        }

        let effective_callee_ty = if let TypeKind::Fn(fid) = callee_kind {
            let ft = self.ty_table.get_function(fid).clone();
            check_call_type_arg_arity(callee, type_args, &ft, self, range, bind);
            let mapping = build_call_mapping(callee, type_args, args, &ft, self, rec, bind);
            if type_args.is_empty() {
                self.check_inferred_constraints(callee, &mapping, range, bind);
            }
            map_generics_cached(self, &callee_ty, &mapping)
        } else {
            callee_ty
        };

        if let ExprKind::Member { property, .. } = &arena.expr(callee).kind {
            let property = *property;
            self.record_type(
                rec,
                arena.expr(property).range.start.offset,
                effective_callee_ty,
            );
        } else {
            self.record_type(
                rec,
                arena.expr(callee).range.start.offset,
                effective_callee_ty,
            );
        }

        let effective_kind = self.ty_table.get(effective_callee_ty.0);
        let params_for_context: Vec<FunctionParam> = if let TypeKind::Fn(fid) = effective_kind {
            self.ty_table.get_function(fid).params.clone()
        } else {
            vec![]
        };
        self.check_call_args_with_context(rec, args, &params_for_context, bind);

        if let TypeKind::Fn(fid) = effective_kind {
            let params = self.ty_table.get_function(fid).params.clone();
            self.validate_call_arguments(rec, args, &params, range, call_id, bind);
        }

        if rec.enabled
            && let TypeKind::Fn(fid) = effective_kind
        {
            let ft = self.ty_table.get_function(fid).clone();
            let callee_name = match &arena.expr(callee).kind {
                ExprKind::Identifier { name } => {
                    Some(std::sync::Arc::from(bind.interner.resolve(*name)))
                }
                ExprKind::Member { property, .. } => {
                    if let ExprKind::Identifier { name } = &arena.expr(*property).kind {
                        Some(std::sync::Arc::from(bind.interner.resolve(*name)))
                    } else {
                        None
                    }
                }
                ExprKind::IntLiteral { .. }
                | ExprKind::FloatLiteral { .. }
                | ExprKind::BigIntLiteral { .. }
                | ExprKind::DecimalLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::CharLiteral { .. }
                | ExprKind::BoolLiteral { .. }
                | ExprKind::NullLiteral
                | ExprKind::RegexLiteral { .. }
                | ExprKind::Template { .. }
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Missing
                | ExprKind::This
                | ExprKind::ImportMeta(_)
                | ExprKind::Super
                | ExprKind::Array { .. }
                | ExprKind::Object { .. }
                | ExprKind::Tuple { .. }
                | ExprKind::Record { .. }
                | ExprKind::Unary { .. }
                | ExprKind::Update { .. }
                | ExprKind::Binary { .. }
                | ExprKind::Logical { .. }
                | ExprKind::Assign { .. }
                | ExprKind::Conditional { .. }
                | ExprKind::Call { .. }
                | ExprKind::New { .. }
                | ExprKind::Function { .. }
                | ExprKind::Arrow { .. }
                | ExprKind::Sequence { .. }
                | ExprKind::Paren { .. }
                | ExprKind::Await { .. }
                | ExprKind::Spawn { .. }
                | ExprKind::Yield { .. }
                | ExprKind::Spread { .. }
                | ExprKind::Pipeline { .. }
                | ExprKind::Range { .. }
                | ExprKind::NonNull { .. }
                | ExprKind::Try { .. }
                | ExprKind::As { .. }
                | ExprKind::Satisfies { .. }
                | ExprKind::ClassExpr { .. }
                | ExprKind::Match { .. }
                | ExprKind::Is { .. }
                | ExprKind::With { .. }
                | ExprKind::MetaAccess { .. } => None,
            };
            let params = ft
                .params
                .iter()
                .map(|p| varn_sem::semantic_info::CallParamInfo {
                    name: p.name.clone(),
                    ty: Type::resolved(p.ty),
                    optional: p.optional,
                    is_rest: p.is_rest,
                })
                .collect();

            let mut arg_to_param_map = Vec::with_capacity(args.len());
            for (i, arg) in args.iter().enumerate() {
                match arg {
                    Arg::Named { label, .. } => {
                        if let Some(pos) = ft
                            .params
                            .iter()
                            .position(|p| p.name.as_deref() == Some(label.as_str()))
                        {
                            arg_to_param_map.push(pos);
                        } else {
                            arg_to_param_map.push(i);
                        }
                    }
                    Arg::Positional(_) | Arg::Spread(_) => {
                        arg_to_param_map.push(i);
                    }
                }
            }

            let call_res = varn_sem::semantic_info::CallResolution {
                callee_name,
                params,
                return_ty: Type::resolved(ft.return_type),
                arg_to_param_map,
            };
            rec.call_resolutions
                .insert(range.start.offset, call_res.clone());
            rec.call_resolutions
                .insert(arena.expr(callee).range.start.offset, call_res);
        }

        self.check_type_arg_constraints(callee, type_args, range, bind);
        self.forget_called_closure(rec, callee, callee_sid, args, bind);
        self.forget_all_members();
    }
}

impl Checker<'_> {
    fn callee_is_value_binding(&self, callee: ExprId, bind: &BindResult) -> bool {
        use varn_sem::symbol::SymbolKind;
        let ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind else {
            return false;
        };
        let scope = bind.scopes.get(self.current_scope);
        scope.resolve(*name, &bind.scopes).is_some_and(|id| {
            matches!(
                bind.arena.get(id).kind,
                SymbolKind::Var | SymbolKind::Let | SymbolKind::Const | SymbolKind::Parameter
            )
        })
    }

    fn is_callable(&mut self, kind: varn_sem::types::InternedTypeKind, bind: &BindResult) -> bool {
        use varn_sem::types::TypeContext;
        let (name, origin) = match kind {
            TypeKind::Fn(_) | TypeKind::Primitive(varn_core::LangPrimitive::Dynamic) => {
                return true;
            }
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => (name, origin),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => return false,
        };
        let name = self.resolve_bind_atom(bind, name);
        let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
        let view = varn_sem::bind::BindView::new(bind, self.resolver);
        match view.get_class_members(&name, origin.as_deref()) {
            Some(members) => members
                .iter()
                .any(|m| m.name.as_ref() == varn_core::MemberKey::Callable.as_str()),
            None => {
                let origin_modules: Vec<String> =
                    origin.iter().map(|o| o.to_string()).collect();
                let interface = bind
                    .get_interface_members_local(&name)
                    .cloned()
                    .or_else(|| {
                        self.resolver
                            .find_bind_for_type(&name, &origin_modules)
                            .and_then(|b| b.get_interface_members_local(&name).cloned())
                    });
                interface.is_none_or(|members| {
                    members
                        .iter()
                        .any(|m| m.name.as_ref() == varn_core::MemberKey::Callable.as_str())
                })
            }
        }
    }
}
