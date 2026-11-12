use super::Checker;
use crate::checker::recorder::Recorder;
use vn_core::ast::destructure::is_destructuring_assign;
use vn_core::ast::ExprId;
use vn_core::{Diagnostic, ErrorCode};
use vn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_assign(
        &mut self,
        rec: &mut Recorder,
        op: vn_core::ast::AssignOp,
        target: ExprId,
        value: ExprId,
        range: vn_core::SourceRange,
        bind: &BindResult,
    ) {
        use vn_core::ast::AssignOp;
        let arena = self.ast_arena;
        if is_destructuring_assign(arena, op, target) {
            self.check_destructuring_assign(rec, target, value, bind);
            return;
        }
        let prev = self.is_assignment_target;
        self.is_assignment_target = true;
        self.check_expr(rec, target, bind);
        self.is_assignment_target = prev;

        let reads_operand = match op {
            AssignOp::Assign
            | AssignOp::AndAssign
            | AssignOp::OrAssign
            | AssignOp::NullishAssign => false,
            AssignOp::AddAssign
            | AssignOp::SubAssign
            | AssignOp::MulAssign
            | AssignOp::DivAssign
            | AssignOp::ModAssign
            | AssignOp::PowAssign
            | AssignOp::BitAndAssign
            | AssignOp::BitOrAssign
            | AssignOp::BitXorAssign
            | AssignOp::ShlAssign
            | AssignOp::ShrAssign
            | AssignOp::UShrAssign => true,
        };
        if matches!(op, AssignOp::AndAssign | AssignOp::OrAssign) {
            self.require_condition(rec, target, bind);
        }
        if reads_operand {
            let read_ty = self.infer_type(rec, target, bind);
            if !read_ty.is_unresolved() && read_ty.is_nullable(&self.ty_table) {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::InvalidTypeOperator,
                        format!(
                            "invalid compound assignment: the target is '{}' and may be null",
                            read_ty.display(&self.ty_table, &bind.interner)
                        ),
                    )
                    .with_range(arena.expr(target).range),
                );
            }
        }

        let target_ty = self.assign_target_type(rec, target, bind);

        let target_expected = if target_ty.is_unresolved() {
            None
        } else {
            Some(target_ty)
        };
        let mark = self.diagnostics_mark();
        self.with_expected(target_expected, |c| c.check_expr(rec, value, bind));
        let value_rejected = self.errors_since(mark);

        self.check_extension_assignment(rec, target, bind);

        let valid_target = matches!(
            &arena.expr(target).kind,
            vn_core::ast::ExprKind::Identifier { .. } | vn_core::ast::ExprKind::Member { .. }
        );
        if !valid_target {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "invalid left-hand side in assignment",
                )
                .with_range(arena.expr(target).range),
            );
        }

        let reassignable =
            if let vn_core::ast::ExprKind::Identifier { name } = &arena.expr(target).kind {
                self.check_reassignable(*name, range, bind)
            } else {
                true
            };

        let value_ty = self.infer_type(rec, value, bind);
        let is_empty_array_val = value_ty.is_unresolved()
            && matches!(&arena.expr(value).kind, vn_core::ast::ExprKind::Array { elements } if elements.is_empty());
        let accepted = if reads_operand {
            self.types_compatible_cached(&target_ty, &value_ty, Some(bind))
        } else {
            self.value_fits(rec, &target_ty, &value_ty, Some(value), Some(bind))
        };
        if !is_empty_array_val && !accepted && valid_target && reassignable && !value_rejected {
            let value_ty_s = value_ty.display(&self.ty_table, &bind.interner);
            let target_ty_s = target_ty.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::error(
                    ErrorCode::TypeMismatch,
                    format!("type mismatch: cannot assign '{value_ty_s}' to '{target_ty_s}'"),
                )
                .with_range(range),
            );
        }

        self.check_pure_assign_target(target, range, bind);
    }

    pub(super) fn assign_target_type(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        bind: &BindResult,
    ) -> vn_sem::types::Type {
        let arena = self.ast_arena;
        if let vn_core::ast::ExprKind::Identifier { name } = &arena.expr(target).kind {
            let name = *name;
            let scope = bind.scopes.get(self.current_scope);
            scope
                .resolve(name, &bind.scopes)
                .and_then(|id| {
                    rec.symbol_types
                        .get(&id)
                        .cloned()
                        .or_else(|| bind.arena.get(id).ty)
                })
                .unwrap_or_else(|| self.infer_type(rec, target, bind))
        } else if let vn_core::ast::ExprKind::Member {
            object,
            property,
            computed: true,
            ..
        } = &arena.expr(target).kind
        {
            self.computed_member_slot(rec, *object, *property, bind).0
        } else if matches!(
            &arena.expr(target).kind,
            vn_core::ast::ExprKind::Member { .. }
        ) {
            self.member_path_declared(rec, target, bind)
                .map(|(_, declared)| declared)
                .unwrap_or_else(|| self.infer_type(rec, target, bind))
        } else {
            self.infer_type(rec, target, bind)
        }
    }

    pub(super) fn check_reassignable(
        &mut self,
        name: vn_core::Atom,
        range: vn_core::SourceRange,
        bind: &BindResult,
    ) -> bool {
        let scope = bind.scopes.get(self.current_scope);
        if let Some(id) = scope.resolve(name, &bind.scopes) {
            let sym = bind.arena.get(id);
            let what = match sym.kind {
                vn_sem::symbol::SymbolKind::Const => Some("constant"),
                vn_sem::symbol::SymbolKind::Class => Some("class"),
                vn_sem::symbol::SymbolKind::Enum => Some("enum"),
                vn_sem::symbol::SymbolKind::Var
                | vn_sem::symbol::SymbolKind::Let
                | vn_sem::symbol::SymbolKind::Function
                | vn_sem::symbol::SymbolKind::Interface
                | vn_sem::symbol::SymbolKind::TypeAlias
                | vn_sem::symbol::SymbolKind::Parameter
                | vn_sem::symbol::SymbolKind::Property
                | vn_sem::symbol::SymbolKind::Method
                | vn_sem::symbol::SymbolKind::TypeParameter
                | vn_sem::symbol::SymbolKind::Namespace
                | vn_sem::symbol::SymbolKind::Extension
                | vn_sem::symbol::SymbolKind::EnumMember => None,
            };
            if let Some(what) = what {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::NotAssignable,
                        format!(
                            "cannot reassign to {what} '{}'",
                            bind.interner.resolve(name)
                        ),
                    )
                    .with_range(range),
                );
                return false;
            }
        }
        true
    }

    pub(super) fn check_pure_assign_target(
        &mut self,
        target: ExprId,
        range: vn_core::SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        if self.pure_scope.is_some() {
            match &arena.expr(target).kind {
                vn_core::ast::ExprKind::Identifier { name } => {
                    self.pure_assign_target_ok(*name, range, bind)
                }
                vn_core::ast::ExprKind::Member { .. } => self.forbid_pure(
                    "mutate reachable state (only parameters and function locals can be assigned)",
                    range,
                ),
                vn_core::ast::ExprKind::IntLiteral { .. }
                | vn_core::ast::ExprKind::FloatLiteral { .. }
                | vn_core::ast::ExprKind::BigIntLiteral { .. }
                | vn_core::ast::ExprKind::DecimalLiteral { .. }
                | vn_core::ast::ExprKind::StrLiteral { .. }
                | vn_core::ast::ExprKind::CharLiteral { .. }
                | vn_core::ast::ExprKind::BoolLiteral { .. }
                | vn_core::ast::ExprKind::NullLiteral
                | vn_core::ast::ExprKind::Template { .. }
                | vn_core::ast::ExprKind::TaggedTemplate { .. }
                | vn_core::ast::ExprKind::Missing
                | vn_core::ast::ExprKind::This
                | vn_core::ast::ExprKind::ImportMeta(_)
                | vn_core::ast::ExprKind::Super
                | vn_core::ast::ExprKind::Array { .. }
                | vn_core::ast::ExprKind::Object { .. }
                | vn_core::ast::ExprKind::Tuple { .. }
                | vn_core::ast::ExprKind::Unary { .. }
                | vn_core::ast::ExprKind::Update { .. }
                | vn_core::ast::ExprKind::Binary { .. }
                | vn_core::ast::ExprKind::Logical { .. }
                | vn_core::ast::ExprKind::Assign { .. }
                | vn_core::ast::ExprKind::Conditional { .. }
                | vn_core::ast::ExprKind::Call { .. }
                | vn_core::ast::ExprKind::New { .. }
                | vn_core::ast::ExprKind::Function { .. }
                | vn_core::ast::ExprKind::Arrow { .. }
                | vn_core::ast::ExprKind::Sequence { .. }
                | vn_core::ast::ExprKind::Paren { .. }
                | vn_core::ast::ExprKind::Await { .. }
                | vn_core::ast::ExprKind::Spawn { .. }
                | vn_core::ast::ExprKind::Yield { .. }
                | vn_core::ast::ExprKind::Spread { .. }
                | vn_core::ast::ExprKind::Pipeline { .. }
                | vn_core::ast::ExprKind::Range { .. }
                | vn_core::ast::ExprKind::NonNull { .. }
                | vn_core::ast::ExprKind::Try { .. }
                | vn_core::ast::ExprKind::As { .. }
                | vn_core::ast::ExprKind::Satisfies { .. }
                | vn_core::ast::ExprKind::ClassExpr { .. }
                | vn_core::ast::ExprKind::Match { .. }
                | vn_core::ast::ExprKind::Is { .. }
                | vn_core::ast::ExprKind::With { .. }
                | vn_core::ast::ExprKind::MetaAccess { .. } => {}
            }
        }
    }

    pub(super) fn check_update(&mut self, rec: &mut Recorder, operand: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        self.check_expr(rec, operand, bind);
        self.require_number(rec, operand, "'++' and '--'", bind);
        self.plan_value_update(rec, operand, bind);
        if self.pure_scope.is_some() {
            let range = arena.expr(operand).range;
            match &arena.expr(operand).kind {
                vn_core::ast::ExprKind::Identifier { name } => {
                    self.pure_assign_target_ok(*name, range, bind)
                }
                vn_core::ast::ExprKind::Member { .. } => self.forbid_pure(
                    "mutate reachable state (only parameters and function locals can be assigned)",
                    range,
                ),
                vn_core::ast::ExprKind::IntLiteral { .. }
                | vn_core::ast::ExprKind::FloatLiteral { .. }
                | vn_core::ast::ExprKind::BigIntLiteral { .. }
                | vn_core::ast::ExprKind::DecimalLiteral { .. }
                | vn_core::ast::ExprKind::StrLiteral { .. }
                | vn_core::ast::ExprKind::CharLiteral { .. }
                | vn_core::ast::ExprKind::BoolLiteral { .. }
                | vn_core::ast::ExprKind::NullLiteral
                | vn_core::ast::ExprKind::Template { .. }
                | vn_core::ast::ExprKind::TaggedTemplate { .. }
                | vn_core::ast::ExprKind::Missing
                | vn_core::ast::ExprKind::This
                | vn_core::ast::ExprKind::ImportMeta(_)
                | vn_core::ast::ExprKind::Super
                | vn_core::ast::ExprKind::Array { .. }
                | vn_core::ast::ExprKind::Object { .. }
                | vn_core::ast::ExprKind::Tuple { .. }
                | vn_core::ast::ExprKind::Unary { .. }
                | vn_core::ast::ExprKind::Update { .. }
                | vn_core::ast::ExprKind::Binary { .. }
                | vn_core::ast::ExprKind::Logical { .. }
                | vn_core::ast::ExprKind::Assign { .. }
                | vn_core::ast::ExprKind::Conditional { .. }
                | vn_core::ast::ExprKind::Call { .. }
                | vn_core::ast::ExprKind::New { .. }
                | vn_core::ast::ExprKind::Function { .. }
                | vn_core::ast::ExprKind::Arrow { .. }
                | vn_core::ast::ExprKind::Sequence { .. }
                | vn_core::ast::ExprKind::Paren { .. }
                | vn_core::ast::ExprKind::Await { .. }
                | vn_core::ast::ExprKind::Spawn { .. }
                | vn_core::ast::ExprKind::Yield { .. }
                | vn_core::ast::ExprKind::Spread { .. }
                | vn_core::ast::ExprKind::Pipeline { .. }
                | vn_core::ast::ExprKind::Range { .. }
                | vn_core::ast::ExprKind::NonNull { .. }
                | vn_core::ast::ExprKind::Try { .. }
                | vn_core::ast::ExprKind::As { .. }
                | vn_core::ast::ExprKind::Satisfies { .. }
                | vn_core::ast::ExprKind::ClassExpr { .. }
                | vn_core::ast::ExprKind::Match { .. }
                | vn_core::ast::ExprKind::Is { .. }
                | vn_core::ast::ExprKind::With { .. }
                | vn_core::ast::ExprKind::MetaAccess { .. } => {}
            }
        }
        if !matches!(
            &arena.expr(operand).kind,
            vn_core::ast::ExprKind::Identifier { .. } | vn_core::ast::ExprKind::Member { .. }
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "invalid left-hand side in update expression",
                )
                .with_range(arena.expr(operand).range),
            );
        }
    }
}
