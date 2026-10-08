use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::{ExprId, ForInit, Pattern, StmtId, VarDeclarator};
use varn_core::{Diagnostic, ErrorCode, SourceRange, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_if_stmt(
        &mut self,
        rec: &mut Recorder,
        test: ExprId,
        consequent: StmtId,
        alternate: Option<StmtId>,
        bind: &BindResult,
    ) {
        self.check_expr(rec, test, bind);
        if self.can_extract_narrowings(test) {
            let narrow_true = self.extract_narrowings(rec, test, bind, true);
            self.with_narrowings(&narrow_true, |checker| {
                checker.check_stmt(rec, consequent, bind)
            });

            if let Some(alt) = alternate {
                let narrow_false = self.extract_narrowings(rec, test, bind, false);
                self.with_narrowings(&narrow_false, |checker| checker.check_stmt(rec, alt, bind));
            }
        } else {
            self.check_stmt(rec, consequent, bind);
            if let Some(alt) = alternate {
                self.check_stmt(rec, alt, bind);
            }
        }
    }

    pub(super) fn check_while_stmt(
        &mut self,
        rec: &mut Recorder,
        test: ExprId,
        body: StmtId,
        bind: &BindResult,
    ) {
        self.check_expr(rec, test, bind);
        self.loop_depth += 1;
        if self.can_extract_narrowings(test) {
            let narrow_true = self.extract_narrowings(rec, test, bind, true);
            self.with_narrowings(&narrow_true, |checker| checker.check_stmt(rec, body, bind));
        } else {
            self.check_stmt(rec, body, bind);
        }
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_stmt(
        &mut self,
        rec: &mut Recorder,
        init: Option<Box<ForInit>>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        self.loop_depth += 1;
        self.with_next_child_scope_span(
            rec,
            bind,
            range.start.offset,
            range.end.offset,
            |checker, rec| {
                if let Some(i) = &init {
                    match i.as_ref() {
                        ForInit::Var { declarators, .. } => {
                            checker.check_for_var_init(rec, declarators, bind)
                        }
                        ForInit::Expr(e) => checker.check_expr(rec, *e, bind),
                    }
                }
                if let Some(t) = test {
                    checker.check_expr(rec, t, bind);
                }
                if let Some(u) = update {
                    checker.check_expr(rec, u, bind);
                }
                checker.check_stmt(rec, body, bind);
            },
        );
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_of_stmt(
        &mut self,
        rec: &mut Recorder,
        left: Pattern,
        right: ExprId,
        body: StmtId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(rec, right, bind);
        let right_ty = self.infer_type(rec, right, bind);
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
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => Type::Dynamic,
        };
        self.loop_depth += 1;
        self.with_next_child_scope_span(
            rec,
            bind,
            range.start.offset,
            range.end.offset,
            |checker, rec| {
                checker.check_pattern(rec, &left, &elem_ty, bind);
                checker.check_stmt(rec, body, bind);
            },
        );
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_in_stmt(
        &mut self,
        rec: &mut Recorder,
        left: Pattern,
        right: ExprId,
        body: StmtId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(rec, right, bind);
        self.loop_depth += 1;
        self.with_next_child_scope_span(
            rec,
            bind,
            range.start.offset,
            range.end.offset,
            |checker, rec| {
                checker.check_pattern(rec, &left, &Type::Str, bind);
                checker.check_stmt(rec, body, bind);
            },
        );
        self.loop_depth -= 1;
    }

    pub(super) fn check_for_var_init(
        &mut self,
        rec: &mut Recorder,
        declarators: &[VarDeclarator],
        bind: &BindResult,
    ) {
        for declarator in declarators {
            let ann = declarator.type_ann.as_ref();
            let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

            if let Some(init_expr) = declarator.init {
                self.with_expected(ann_ty_opt, |c| c.check_expr(rec, init_expr, bind));

                if let Some(ann_ty) = &ann_ty_opt {
                    let init_ty = self.infer_type(rec, init_expr, bind);
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
                    self.check_pattern(rec, &declarator.id, ann_ty, bind);
                } else {
                    let init_ty = self.infer_type(rec, init_expr, bind);
                    self.reject_void_value(&init_ty, declarator.range);
                    self.check_pattern(rec, &declarator.id, &init_ty, bind);
                }
            }
        }
    }
}
