use super::Checker;
use crate::checker::recorder::Recorder;
use std::sync::Arc;
use varn_core::ast::{ArrowBody, ExprId};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_arrow(
        &mut self,
        rec: &mut Recorder,
        params: &[varn_core::ast::Param],
        return_type: &Option<varn_core::ast::TypeNode>,
        body: &ArrowBody,
        is_async: bool,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let params = params.to_vec();
        let return_type = return_type.clone();
        let body = body.clone();
        let saved_expected = self.expected_return_type.take();

        let resolved_ret = return_type
            .as_ref()
            .map(|rt| self.resolve_type_node_cached(rt, bind))
            .or_else(|| self.expected_return_from_fn_type());
        self.expected_return_type = if is_async {
            resolved_ret.map(|t| varn_sem::types::awaited(&t, &self.ty_table))
        } else {
            resolved_ret
        };

        let saved_scope = self.current_scope;
        if let Some(fn_scope) = self.next_child_scope(bind) {
            self.current_scope = fn_scope;
            self.record_scope_span(rec, range.start.offset, range.end.offset, fn_scope);
        }

        let mut injected_type_params: Vec<Arc<str>> = vec![];
        if let Some(expected_fn) = self.expected_fn_type() {
            for ep in &expected_fn.params {
                if let varn_core::TypeKind::Named(n, origin) = self.ty_table.get(ep.ty)
                    && !varn_core::is_lang_type(n, origin)
                {
                    let n_str = bind.interner.try_resolve(n).unwrap_or_default();
                    injected_type_params.push(Arc::from(n_str));
                }
            }
            if let varn_core::TypeKind::Named(n, origin) =
                self.ty_table.get(expected_fn.return_type)
                && !varn_core::is_lang_type(n, origin)
            {
                let n_str = bind.interner.try_resolve(n).unwrap_or_default();
                injected_type_params.push(Arc::from(n_str));
            }
            for tp in &injected_type_params {
                self.active_type_params.insert(tp.clone());
            }
            self.apply_contextual_arrow_params(rec, &params, &expected_fn, bind);
        }

        self.check_param_defaults(rec, &params, bind);
        let block = match &body {
            ArrowBody::Block(stmt) if return_type.is_some() => Some(*stmt),
            ArrowBody::Block(_) | ArrowBody::Expr(_) => None,
        };
        self.in_function_body(is_async, |c| c.check_arrow_body(rec, body, range, bind));
        if let Some(block) = block {
            self.require_return(rec, block, bind);
        }

        self.current_scope = saved_scope;
        self.expected_return_type = saved_expected;
        for tp in &injected_type_params {
            self.active_type_params.remove(tp.as_ref());
        }
        self.mark_infer_env_dirty();
    }

    pub(super) fn check_function_expr(
        &mut self,
        rec: &mut Recorder,
        params: &[varn_core::ast::Param],
        return_type: &Option<varn_core::ast::TypeNode>,
        body: varn_core::ast::StmtId,
        is_async: bool,
        is_generator: bool,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let return_type = return_type.clone();
        let saved_expected = self.expected_return_type.take();
        self.expected_return_type = return_type.as_ref().map(|rt| {
            let ty = self.resolve_type_node_cached(rt, bind);
            if is_async {
                varn_sem::types::awaited(&ty, &self.ty_table)
            } else {
                ty
            }
        });

        let saved_scope = self.current_scope;
        if let Some(fn_scope) = self.next_child_scope(bind) {
            self.current_scope = fn_scope;
            self.record_scope_span(rec, range.start.offset, range.end.offset, fn_scope);
        }

        self.check_param_defaults(rec, params, bind);
        let outer_super = std::mem::replace(&mut self.super_reachable, false);
        self.in_function_body(is_async, |c| c.check_stmt(rec, body, bind));
        self.super_reachable = outer_super;
        if return_type.is_some() && !is_generator {
            self.require_return(rec, body, bind);
        }

        self.current_scope = saved_scope;
        self.expected_return_type = saved_expected;
    }

    fn is_enum_type_of(&self, t: &Type, bind: &BindResult) -> bool {
        let varn_core::TypeKind::Named(n, origin) = self.ty_table.get(t.0) else {
            return false;
        };
        let name = self.resolve_bind_atom(bind, n);
        let origin_modules: Vec<String> = origin
            .map(|o| self.resolve_bind_atom(bind, o).to_string())
            .into_iter()
            .collect();
        crate::checker_expressions::members::is_enum_type(
            self.resolver,
            bind,
            &name,
            &origin_modules,
        )
    }

    fn cast_related(
        &mut self,
        rec: &mut Recorder,
        source: &Type,
        target: &Type,
        expression: Option<ExprId>,
        bind: &BindResult,
    ) -> bool {
        let source_base = source.apparent(&self.ty_table);
        let target_base = target.apparent(&self.ty_table);
        if source_base.is_dynamic()
            || (!self.open_parameters(*source, bind).is_empty()
                && self.open_parameters(*target, bind).is_empty())
            || target_base.is_dynamic()
            || (source_base.is_numeric() && target_base.is_numeric())
            || (source_base.is_int() && self.is_enum_type_of(&target_base, bind))
            || (target_base.is_int() && self.is_enum_type_of(&source_base, bind))
            || self.value_fits(rec, target, source, expression, Some(bind))
            || self.value_fits(rec, source, target, None, Some(bind))
        {
            return true;
        }
        let members_of = |checker: &Self, t: &Type| -> Option<Vec<Type>> {
            match checker.ty_table.get(t.0) {
                varn_core::TypeKind::Union(list) => Some(
                    checker
                        .ty_table
                        .get_list(list)
                        .iter()
                        .map(|m| Type::resolved(*m))
                        .collect(),
                ),
                varn_core::TypeKind::Primitive(_)
                | varn_core::TypeKind::Literal(_)
                | varn_core::TypeKind::This
                | varn_core::TypeKind::Array(_)
                | varn_core::TypeKind::Intersection(_)
                | varn_core::TypeKind::Tuple(_)
                | varn_core::TypeKind::Named(..)
                | varn_core::TypeKind::Generic(..)
                | varn_core::TypeKind::TemplateLiteral(_)
                | varn_core::TypeKind::Fn(_)
                | varn_core::TypeKind::Object(_)
                | varn_core::TypeKind::Typeof(_)
                | varn_core::TypeKind::KeyOf(_)
                | varn_core::TypeKind::IndexedAccess { .. }
                | varn_core::TypeKind::Mapped { .. }
                | varn_core::TypeKind::Conditional { .. }
                | varn_core::TypeKind::Infer(_)
                | varn_core::TypeKind::EnumVariant { .. }
                | varn_core::TypeKind::TypePredicate { .. } => None,
            }
        };
        if let Some(members) = members_of(self, &source_base) {
            return members
                .iter()
                .any(|m| self.cast_related(rec, m, &target_base, None, bind));
        }
        if let Some(members) = members_of(self, &target_base) {
            return members
                .iter()
                .any(|m| self.cast_related(rec, &source_base, m, None, bind));
        }
        if let (varn_core::TypeKind::Array(a), varn_core::TypeKind::Array(b)) = (
            self.ty_table.get(source_base.0),
            self.ty_table.get(target_base.0),
        ) {
            return self.cast_related(rec, &Type::resolved(a), &Type::resolved(b), None, bind);
        }
        false
    }

    pub(super) fn check_as_cast(
        &mut self,
        rec: &mut Recorder,
        at: ExprId,
        expression: ExprId,
        type_ann: &varn_core::ast::TypeNode,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let target = self.resolve_type_node_cached(type_ann, bind);
        let source = self.infer_type(rec, expression, bind);
        if !self.open_parameters(target, bind).is_empty() {
            if source.0 == target.0
                || source == Type::Never
                || self.value_fits(rec, &target, &source, Some(expression), Some(bind))
                || rec.decoder_returns.contains(&at.index())
            {
                return;
            }
            self.emit(Diagnostic::error(ErrorCode::InvalidAsCast,
                "cannot check a cast to an unresolved type parameter; preserve its type or decode at a concrete call site")
                .with_range(range));
            return;
        }
        if self.cast_related(rec, &source, &target, Some(expression), bind) {
            return;
        }
        let source_s = source.display(&self.ty_table, &bind.interner);
        let target_s = target.display(&self.ty_table, &bind.interner);
        self.emit(
            Diagnostic::error(
                ErrorCode::InvalidAsCast,
                format!("cannot convert '{source_s}' to '{target_s}' with 'as': the types are unrelated"),
            )
            .with_range(range),
        );
    }

    pub(super) fn check_satisfies(
        &mut self,
        rec: &mut Recorder,
        expression: ExprId,
        type_ann: &varn_core::ast::TypeNode,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(rec, expression, bind);
        self.check_type_names(type_ann, bind);
        let declared_ty = self.resolve_type_node_cached(type_ann, bind);
        let inferred_ty = self.infer_type(rec, expression, bind);
        if !self.types_compatible_cached(&declared_ty, &inferred_ty, Some(bind)) {
            let declared_s = declared_ty.display(&self.ty_table, &bind.interner);
            let inferred_s = inferred_ty.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidSatisfies,
                    format!("expression does not satisfy '{declared_s}': got '{inferred_s}'"),
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_await(
        &mut self,
        rec: &mut Recorder,
        argument: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_some() {
            self.forbid_pure("suspend on 'await' (pure functions are synchronous)", range);
        }
        if !self.in_async {
            self.emit(
                Diagnostic::error(
                    ErrorCode::AwaitOutsideAsync,
                    "'await' is only valid inside an async function or at the top level",
                )
                .with_range(range),
            );
        }
        self.check_expr(rec, argument, bind);
        let arg_ty = self.infer_type(rec, argument, bind);
        if !arg_ty.is_dynamic() && !varn_sem::types::is_awaitable(&arg_ty, &self.ty_table) {
            let arg_ty_s = arg_ty.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::warning(
                    ErrorCode::TypeMismatch,
                    format!("'await' applied to non-Future type '{arg_ty_s}' has no effect"),
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_yield(
        &mut self,
        rec: &mut Recorder,
        argument: Option<ExprId>,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_some() {
            self.forbid_pure("suspend on 'yield' (pure functions are synchronous)", range);
        }
        let declared = self.declared_yield;
        let ty = if let Some(arg) = argument {
            self.with_expected(declared, |c| c.check_expr(rec, arg, bind));
            let given = self.infer_type(rec, arg, bind);
            if let Some(declared) = declared
                && !self.value_fits(rec, &declared, &given, Some(arg), Some(bind))
            {
                let declared_s = declared.display(&self.ty_table, &bind.interner);
                let given_s = given.display(&self.ty_table, &bind.interner);
                self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::TypeMismatch,
                            format!(
                                "type mismatch: the generator yields '{declared_s}', but this yields '{given_s}'"
                            ),
                        )
                        .with_range(self.ast_arena.expr(arg).range),
                    );
            }
            given
        } else {
            Type::Void
        };
        if let Some(yields) = &mut self.yielded_types {
            yields.push(ty);
        }
    }

    fn check_arrow_body(
        &mut self,
        rec: &mut Recorder,
        body: ArrowBody,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        match body {
            ArrowBody::Block(stmt) => self.check_stmt(rec, stmt, bind),
            ArrowBody::Expr(e) => {
                let expected_ret = self.expected_return_type;
                self.with_expected(expected_ret, |c| c.check_expr(rec, e, bind));
                let actual = self.infer_type(rec, e, bind);
                if let Some(expected) = self.expected_return_type {
                    let expected_kind = self.ty_table.get(expected.0);
                    let is_tp = matches!(expected_kind, varn_core::TypeKind::Named(n, _) if self.active_type_params.contains(bind.interner.try_resolve(n).unwrap_or_default()));
                    let is_void = matches!(
                        expected_kind,
                        varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Void)
                    );
                    if !is_tp
                        && !is_void
                        && !self.types_compatible_cached(&expected, &actual, Some(bind))
                    {
                        let expected_s = expected.display(&self.ty_table, &bind.interner);
                        let actual_s = actual.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: arrow function is declared to return '{expected_s}', but returns '{actual_s}'"
                            ))
                            .with_range(range),
                        );
                    }
                }
            }
        }
    }
}
