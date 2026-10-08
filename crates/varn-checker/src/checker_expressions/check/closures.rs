use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use std::sync::Arc;
use varn_core::ast::{ArrowBody, ExprId};
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn check_arrow(
        &mut self,
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
            resolved_ret.map(|t| crate::types::awaited(&t, &self.ty_table))
        } else {
            resolved_ret
        };

        let saved_scope = self.current_scope;
        if let Some(fn_scope) = self.next_child_scope(bind) {
            self.current_scope = fn_scope;
            self.record_scope_span(range.start.offset, range.end.offset, fn_scope);
        }

        let mut injected_type_params: Vec<Arc<str>> = vec![];
        if let Some(expected_fn) = self.expected_fn_type() {
            for ep in &expected_fn.params {
                if let varn_core::TypeKind::Named(n, _) = self.ty_table.get(ep.ty) {
                    let n_str = bind.interner.try_resolve(n).unwrap_or_default();
                    if !varn_core::is_lang_type_name(n_str) {
                        injected_type_params.push(Arc::from(n_str));
                    }
                }
            }
            if let varn_core::TypeKind::Named(n, _) = self.ty_table.get(expected_fn.return_type) {
                let n_str = bind.interner.try_resolve(n).unwrap_or_default();
                if !varn_core::is_lang_type_name(n_str) {
                    injected_type_params.push(Arc::from(n_str));
                }
            }
            for tp in &injected_type_params {
                self.active_type_params.insert(tp.clone());
            }
            self.apply_contextual_arrow_params(&params, &expected_fn, bind);
        }

        self.in_function_body(is_async, |c| c.check_arrow_body(body, range, bind));

        self.current_scope = saved_scope;
        self.expected_return_type = saved_expected;
        for tp in &injected_type_params {
            self.active_type_params.remove(tp.as_ref());
        }
    }

    pub(super) fn check_function_expr(
        &mut self,
        return_type: &Option<varn_core::ast::TypeNode>,
        body: varn_core::ast::StmtId,
        is_async: bool,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let return_type = return_type.clone();
        let saved_expected = self.expected_return_type.take();
        self.expected_return_type = return_type.as_ref().map(|rt| {
            let ty = self.resolve_type_node_cached(rt, bind);
            if is_async {
                crate::types::awaited(&ty, &self.ty_table)
            } else {
                ty
            }
        });

        let saved_scope = self.current_scope;
        if let Some(fn_scope) = self.next_child_scope(bind) {
            self.current_scope = fn_scope;
            self.record_scope_span(range.start.offset, range.end.offset, fn_scope);
        }

        self.in_function_body(is_async, |c| c.check_stmt(body, bind));

        self.current_scope = saved_scope;
        self.expected_return_type = saved_expected;
    }

    pub(super) fn check_satisfies(
        &mut self,
        expression: ExprId,
        type_ann: &varn_core::ast::TypeNode,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(expression, bind);
        let declared_ty = self.resolve_type_node_cached(type_ann, bind);
        let inferred_ty = self.infer_type(expression, bind);
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
        self.check_expr(argument, bind);
        let arg_ty = self.infer_type(argument, bind);
        if !arg_ty.is_dynamic() && !crate::types::is_awaitable(&arg_ty, &self.ty_table) {
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
        argument: Option<ExprId>,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_some() {
            self.forbid_pure("suspend on 'yield' (pure functions are synchronous)", range);
        }
        let ty = if let Some(arg) = argument {
            self.check_expr(arg, bind);
            self.infer_type(arg, bind)
        } else {
            Type::Void
        };
        if let Some(yields) = &mut self.yielded_types {
            yields.push(ty);
        }
    }

    fn check_arrow_body(
        &mut self,
        body: ArrowBody,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        match body {
            ArrowBody::Block(stmt) => self.check_stmt(stmt, bind),
            ArrowBody::Expr(e) => {
                let expected_ret = self.expected_return_type;
                self.with_expected(expected_ret, |c| c.check_expr(e, bind));
                let actual = self.infer_type(e, bind);
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
