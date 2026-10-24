use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::{ExprKind, Pattern};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn reject_void_value(&mut self, ty: &Type, range: varn_core::SourceRange) {
        if *ty == Type::Void {
            self.emit(
                Diagnostic::error(
                    ErrorCode::VoidValueUsed,
                    "a 'void' call produces no value; use `()` (Unit) for an empty result",
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_variable(
        &mut self,
        rec: &mut Recorder,
        v: &varn_core::ast::VariableDecl,
        decl_range: &varn_core::SourceRange,
        bind: &BindResult,
    ) {
        for d in &v.declarators {
            let ann = d
                .type_ann
                .as_ref()
                .filter(|node| self.check_type_names(node, bind));
            let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

            if ann.is_none() {
                self.declare_annotated_function(rec, d, bind);
            }

            if let Some(init_expr) = d.init {
                self.with_expected(ann_ty_opt, |c| c.check_expr(rec, init_expr, bind));

                if matches!(
                    &self.ast_arena.expr(init_expr).kind,
                    ExprKind::Function { .. } | ExprKind::Arrow { .. }
                ) && let Some(effects) = self.closure_def_effects(init_expr, bind)
                {
                    let mut bound = Vec::new();
                    Self::pattern_bound_atoms(&d.id, &mut bound);
                    for name in bound {
                        if let Some(id) = bind
                            .scopes
                            .get(self.current_scope)
                            .resolve(name, &bind.scopes)
                        {
                            let entry = self.closure_effects.entry(id).or_default();
                            entry.vars.extend(effects.vars.iter().copied());
                            entry.vars.sort();
                            entry.vars.dedup();
                            entry.members.extend(effects.members.iter().cloned());
                            entry.members.sort();
                            entry.members.dedup();
                        }
                    }
                }

                if let Some(ann_ty) = &ann_ty_opt {
                    let init_ty = self.infer_type(rec, init_expr, bind);
                    let is_empty_array = init_ty.is_dynamic()
                        && matches!(&self.ast_arena.expr(init_expr).kind, ExprKind::Array { elements } if elements.is_empty());
                    let is_compatible =
                        self.value_fits(rec, ann_ty, &init_ty, Some(init_expr), Some(bind));
                    if !is_empty_array && !is_compatible {
                        let ann_ty_s = ann_ty.display(&self.ty_table, &bind.interner);
                        let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: declared as '{ann_ty_s}' but initialised with '{init_ty_s}'"
                            ))
                            .with_range(*decl_range),
                        );
                    }
                    self.check_pattern(rec, &d.id, ann_ty, bind);
                } else {
                    let init_ty = self.infer_type(rec, init_expr, bind);
                    self.reject_void_value(&init_ty, *decl_range);
                    self.check_pattern(rec, &d.id, &init_ty, bind);
                }
            }
        }
    }

    fn declare_annotated_function(
        &mut self,
        rec: &mut Recorder,
        d: &varn_core::ast::VarDeclarator,
        bind: &BindResult,
    ) {
        let (Pattern::Identifier { name, .. }, Some(init)) = (&d.id, d.init) else {
            return;
        };
        let kind = &self.ast_arena.expr(init).kind;
        let (params, return_type, is_arrow) = if let ExprKind::Arrow {
            params,
            return_type: Some(ret),
            ..
        } = kind
        {
            (params, ret, true)
        } else if let ExprKind::Function {
            params,
            return_type: Some(ret),
            is_generator: false,
            ..
        } = kind
        {
            (params, ret, false)
        } else {
            return;
        };
        if params.iter().any(|p| p.type_ann.is_none()) {
            return;
        }
        let Some(id) = bind
            .scopes
            .get(self.current_scope)
            .resolve(*name, &bind.scopes)
        else {
            return;
        };
        let view = varn_sem::bind::BindView::new(bind, self.resolver).at(self.current_scope);
        let ty = varn_sem::infer::build_fn_type(
            params,
            &Some(return_type.clone()),
            is_arrow,
            Some(&view),
            std::sync::Arc::make_mut(&mut self.ty_table),
            Type::Dynamic,
        );
        rec.symbol_types.insert(id, ty);
    }
}
