use super::super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use varn_core::ast::{ExprId, ForInit, Pattern, StmtId, VarDeclarator};
use varn_core::{Diagnostic, ErrorCode, SourceRange, TypeKind};

impl<'r> Checker<'r> {
    pub(super) fn check_if_stmt(
        &mut self,
        test: ExprId,
        consequent: StmtId,
        alternate: Option<StmtId>,
        bind: &BindResult,
    ) {
        self.check_expr(test, bind);
        if self.can_extract_narrowings(test) {
            let narrow_true = self.extract_narrowings(test, bind, true);
            self.with_narrowings(&narrow_true, |checker| checker.check_stmt(consequent, bind));

            if let Some(alt) = alternate {
                let narrow_false = self.extract_narrowings(test, bind, false);
                self.with_narrowings(&narrow_false, |checker| checker.check_stmt(alt, bind));
            }
        } else {
            self.check_stmt(consequent, bind);
            if let Some(alt) = alternate {
                self.check_stmt(alt, bind);
            }
        }
    }

    pub(super) fn check_while_stmt(&mut self, test: ExprId, body: StmtId, bind: &BindResult) {
        self.check_expr(test, bind);
        self.loop_depth += 1;
        if self.can_extract_narrowings(test) {
            let narrow_true = self.extract_narrowings(test, bind, true);
            self.with_narrowings(&narrow_true, |checker| checker.check_stmt(body, bind));
        } else {
            self.check_stmt(body, bind);
        }
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_stmt(
        &mut self,
        init: Option<Box<ForInit>>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        self.loop_depth += 1;
        self.with_next_child_scope_span(bind, range.start.offset, range.end.offset, |checker| {
            if let Some(i) = &init {
                match i.as_ref() {
                    ForInit::Var { declarators, .. } => {
                        checker.check_for_var_init(declarators, bind)
                    }
                    ForInit::Expr(e) => checker.check_expr(*e, bind),
                }
            }
            if let Some(t) = test {
                checker.check_expr(t, bind);
            }
            if let Some(u) = update {
                checker.check_expr(u, bind);
            }
            checker.check_stmt(body, bind);
        });
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_of_stmt(
        &mut self,
        left: Pattern,
        right: ExprId,
        body: StmtId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(right, bind);
        let right_ty = self.infer_type(right, bind);
        let right_kind = self.ty_table.get(right_ty.0);
        let elem_ty = match right_kind {
            TypeKind::Array(inner) => Type::resolved(inner),
            TypeKind::Primitive(varn_core::LangPrimitive::Str) | TypeKind::TemplateLiteral(_) => {
                Type::Char
            }
            TypeKind::Named(name, _)
                if bind.interner.get(varn_core::LangPrimitive::Str.name()) == Some(name) =>
            {
                Type::Char
            }
            TypeKind::Generic(name, args, _)
                if bind.interner.get(varn_core::BuiltinType::Map.name()) == Some(name)
                    && self.ty_table.get_list(args).len() == 2 =>
            {
                let arg_ids = self.ty_table.get_list(args).to_vec();
                let list = std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&arg_ids);
                Type::resolved(
                    std::sync::Arc::make_mut(&mut self.ty_table).intern(TypeKind::Tuple(list)),
                )
            }
            TypeKind::Generic(_name, args, _) if self.ty_table.get_list(args).len() == 1 => {
                Type::resolved(self.ty_table.get_list(args)[0])
            }
            TypeKind::Builtin(varn_core::BuiltinType::Range) => Type::Int,
            _ => Type::Dynamic,
        };
        self.loop_depth += 1;
        self.with_next_child_scope_span(bind, range.start.offset, range.end.offset, |checker| {
            checker.check_pattern(&left, &elem_ty, bind);
            checker.check_stmt(body, bind);
        });
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_in_stmt(
        &mut self,
        left: Pattern,
        right: ExprId,
        body: StmtId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(right, bind);
        self.loop_depth += 1;
        self.with_next_child_scope_span(bind, range.start.offset, range.end.offset, |checker| {
            checker.check_pattern(&left, &Type::Str, bind);
            checker.check_stmt(body, bind);
        });
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_var_init(&mut self, declarators: &[VarDeclarator], bind: &BindResult) {
        for declarator in declarators {
            let ann = declarator.type_ann.as_ref().or(match &declarator.id {
                varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                _ => None,
            });
            let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

            if let Some(init_expr) = declarator.init {
                self.with_expected(ann_ty_opt, |c| c.check_expr(init_expr, bind));

                if let Some(ann_ty) = &ann_ty_opt {
                    let init_ty = self.infer_type(init_expr, bind);
                    let is_empty_array = init_ty.is_dynamic()
                        && matches!(&self.ast_arena.expr(init_expr).kind, varn_core::ast::ExprKind::Array { elements } if elements.is_empty());
                    if !is_empty_array
                        && !self.value_assignable_to(ann_ty, &init_ty, Some(init_expr), Some(bind))
                    {
                        let ann_ty_s = ann_ty.display(&self.ty_table, &bind.interner);
                        let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: declared as '{ann_ty_s}' but initialised with '{init_ty_s}'"
                            ))
                            .with_range(declarator.range),
                        );
                    }
                    self.check_pattern(&declarator.id, ann_ty, bind);
                } else {
                    let init_ty = self.infer_type(init_expr, bind);
                    self.reject_void_value(&init_ty, declarator.range);
                    self.check_pattern(&declarator.id, &init_ty, bind);
                }
            }
        }
    }
}
