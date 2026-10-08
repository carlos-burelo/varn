use super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::ExprId;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_assign(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        value: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let prev = self.is_assignment_target;
        self.is_assignment_target = true;
        self.check_expr(rec, target, bind);
        self.is_assignment_target = prev;

        let target_ty =
            if let varn_core::ast::ExprKind::Identifier { name } = &arena.expr(target).kind {
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
            } else {
                self.infer_type(rec, target, bind)
            };

        let target_expected = if target_ty.is_dynamic() {
            None
        } else {
            Some(target_ty)
        };
        self.with_expected(target_expected, |c| c.check_expr(rec, value, bind));

        self.check_extension_assignment(rec, target, bind);

        if !matches!(
            &arena.expr(target).kind,
            varn_core::ast::ExprKind::Identifier { .. } | varn_core::ast::ExprKind::Member { .. }
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "invalid left-hand side in assignment",
                )
                .with_range(arena.expr(target).range),
            );
        }

        if let varn_core::ast::ExprKind::Identifier { name } = &arena.expr(target).kind {
            let name = *name;
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(name, &bind.scopes) {
                let sym = bind.arena.get(id);
                let what = match sym.kind {
                    varn_sem::symbol::SymbolKind::Const => Some("constant"),
                    varn_sem::symbol::SymbolKind::Class => Some("class"),
                    varn_sem::symbol::SymbolKind::Enum => Some("enum"),
                    varn_sem::symbol::SymbolKind::Var
                    | varn_sem::symbol::SymbolKind::Let
                    | varn_sem::symbol::SymbolKind::Function
                    | varn_sem::symbol::SymbolKind::Interface
                    | varn_sem::symbol::SymbolKind::TypeAlias
                    | varn_sem::symbol::SymbolKind::Parameter
                    | varn_sem::symbol::SymbolKind::Property
                    | varn_sem::symbol::SymbolKind::Method
                    | varn_sem::symbol::SymbolKind::TypeParameter
                    | varn_sem::symbol::SymbolKind::Namespace
                    | varn_sem::symbol::SymbolKind::Struct
                    | varn_sem::symbol::SymbolKind::Extension
                    | varn_sem::symbol::SymbolKind::EnumMember => None,
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
                }
            }
        }

        let value_ty = self.infer_type(rec, value, bind);
        let is_empty_array_val = value_ty.is_dynamic()
            && matches!(&arena.expr(value).kind, varn_core::ast::ExprKind::Array { elements } if elements.is_empty());
        if !is_empty_array_val && !self.types_compatible_cached(&target_ty, &value_ty, Some(bind)) {
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

        if self.pure_scope.is_some() {
            match &arena.expr(target).kind {
                varn_core::ast::ExprKind::Identifier { name } => {
                    self.pure_assign_target_ok(*name, range, bind)
                }
                varn_core::ast::ExprKind::Member { .. } => self.forbid_pure(
                    "mutate reachable state (only parameters and function locals can be assigned)",
                    range,
                ),
                varn_core::ast::ExprKind::IntLiteral { .. }
                | varn_core::ast::ExprKind::FloatLiteral { .. }
                | varn_core::ast::ExprKind::BigIntLiteral { .. }
                | varn_core::ast::ExprKind::DecimalLiteral { .. }
                | varn_core::ast::ExprKind::StrLiteral { .. }
                | varn_core::ast::ExprKind::CharLiteral { .. }
                | varn_core::ast::ExprKind::BoolLiteral { .. }
                | varn_core::ast::ExprKind::NullLiteral
                | varn_core::ast::ExprKind::RegexLiteral { .. }
                | varn_core::ast::ExprKind::Template { .. }
                | varn_core::ast::ExprKind::TaggedTemplate { .. }
                | varn_core::ast::ExprKind::Missing
                | varn_core::ast::ExprKind::This
                | varn_core::ast::ExprKind::Super
                | varn_core::ast::ExprKind::Array { .. }
                | varn_core::ast::ExprKind::Object { .. }
                | varn_core::ast::ExprKind::Tuple { .. }
                | varn_core::ast::ExprKind::Record { .. }
                | varn_core::ast::ExprKind::Unary { .. }
                | varn_core::ast::ExprKind::Update { .. }
                | varn_core::ast::ExprKind::Binary { .. }
                | varn_core::ast::ExprKind::Logical { .. }
                | varn_core::ast::ExprKind::Assign { .. }
                | varn_core::ast::ExprKind::Conditional { .. }
                | varn_core::ast::ExprKind::Call { .. }
                | varn_core::ast::ExprKind::New { .. }
                | varn_core::ast::ExprKind::Function { .. }
                | varn_core::ast::ExprKind::Arrow { .. }
                | varn_core::ast::ExprKind::Sequence { .. }
                | varn_core::ast::ExprKind::Paren { .. }
                | varn_core::ast::ExprKind::Await { .. }
                | varn_core::ast::ExprKind::Spawn { .. }
                | varn_core::ast::ExprKind::Yield { .. }
                | varn_core::ast::ExprKind::Spread { .. }
                | varn_core::ast::ExprKind::Pipeline { .. }
                | varn_core::ast::ExprKind::Range { .. }
                | varn_core::ast::ExprKind::NonNull { .. }
                | varn_core::ast::ExprKind::Try { .. }
                | varn_core::ast::ExprKind::As { .. }
                | varn_core::ast::ExprKind::Satisfies { .. }
                | varn_core::ast::ExprKind::ClassExpr { .. }
                | varn_core::ast::ExprKind::Match { .. }
                | varn_core::ast::ExprKind::Is { .. }
                | varn_core::ast::ExprKind::With { .. }
                | varn_core::ast::ExprKind::MetaAccess { .. } => {}
            }
        }
    }

    pub(super) fn check_update(&mut self, rec: &mut Recorder, operand: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        self.check_expr(rec, operand, bind);
        if self.pure_scope.is_some() {
            let range = arena.expr(operand).range;
            match &arena.expr(operand).kind {
                varn_core::ast::ExprKind::Identifier { name } => {
                    self.pure_assign_target_ok(*name, range, bind)
                }
                varn_core::ast::ExprKind::Member { .. } => self.forbid_pure(
                    "mutate reachable state (only parameters and function locals can be assigned)",
                    range,
                ),
                varn_core::ast::ExprKind::IntLiteral { .. }
                | varn_core::ast::ExprKind::FloatLiteral { .. }
                | varn_core::ast::ExprKind::BigIntLiteral { .. }
                | varn_core::ast::ExprKind::DecimalLiteral { .. }
                | varn_core::ast::ExprKind::StrLiteral { .. }
                | varn_core::ast::ExprKind::CharLiteral { .. }
                | varn_core::ast::ExprKind::BoolLiteral { .. }
                | varn_core::ast::ExprKind::NullLiteral
                | varn_core::ast::ExprKind::RegexLiteral { .. }
                | varn_core::ast::ExprKind::Template { .. }
                | varn_core::ast::ExprKind::TaggedTemplate { .. }
                | varn_core::ast::ExprKind::Missing
                | varn_core::ast::ExprKind::This
                | varn_core::ast::ExprKind::Super
                | varn_core::ast::ExprKind::Array { .. }
                | varn_core::ast::ExprKind::Object { .. }
                | varn_core::ast::ExprKind::Tuple { .. }
                | varn_core::ast::ExprKind::Record { .. }
                | varn_core::ast::ExprKind::Unary { .. }
                | varn_core::ast::ExprKind::Update { .. }
                | varn_core::ast::ExprKind::Binary { .. }
                | varn_core::ast::ExprKind::Logical { .. }
                | varn_core::ast::ExprKind::Assign { .. }
                | varn_core::ast::ExprKind::Conditional { .. }
                | varn_core::ast::ExprKind::Call { .. }
                | varn_core::ast::ExprKind::New { .. }
                | varn_core::ast::ExprKind::Function { .. }
                | varn_core::ast::ExprKind::Arrow { .. }
                | varn_core::ast::ExprKind::Sequence { .. }
                | varn_core::ast::ExprKind::Paren { .. }
                | varn_core::ast::ExprKind::Await { .. }
                | varn_core::ast::ExprKind::Spawn { .. }
                | varn_core::ast::ExprKind::Yield { .. }
                | varn_core::ast::ExprKind::Spread { .. }
                | varn_core::ast::ExprKind::Pipeline { .. }
                | varn_core::ast::ExprKind::Range { .. }
                | varn_core::ast::ExprKind::NonNull { .. }
                | varn_core::ast::ExprKind::Try { .. }
                | varn_core::ast::ExprKind::As { .. }
                | varn_core::ast::ExprKind::Satisfies { .. }
                | varn_core::ast::ExprKind::ClassExpr { .. }
                | varn_core::ast::ExprKind::Match { .. }
                | varn_core::ast::ExprKind::Is { .. }
                | varn_core::ast::ExprKind::With { .. }
                | varn_core::ast::ExprKind::MetaAccess { .. } => {}
            }
        }
        if !matches!(
            &arena.expr(operand).kind,
            varn_core::ast::ExprKind::Identifier { .. } | varn_core::ast::ExprKind::Member { .. }
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
