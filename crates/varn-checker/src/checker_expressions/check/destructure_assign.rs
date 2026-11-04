use super::Checker;
use crate::checker::recorder::Recorder;
use crate::checker_expressions::patterns::SequenceShape;
use std::sync::Arc;
use varn_core::ast::{ArrayEl, ExprId, ExprKind, ObjectProp, PropKey};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl Checker<'_> {
    pub(super) fn check_destructuring_assign(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        value: ExprId,
        bind: &BindResult,
    ) {
        self.check_expr(rec, value, bind);
        let value_ty = self.infer_type(rec, value, bind);
        self.check_destructure_target(rec, target, value_ty, bind);
    }

    fn check_destructure_target(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        ty: Type,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let range = arena.expr(target).range;
        match &arena.expr(target).kind {
            ExprKind::Tuple { elements } => {
                let fixed_count = elements
                    .iter()
                    .filter(|e| !matches!(arena.expr(**e).kind, ExprKind::Spread { .. }))
                    .count();
                let has_rest = fixed_count != elements.len();
                let shape = self.sequence_shape(&ty, true, fixed_count, has_rest, range, bind);
                for (at, element) in elements.iter().enumerate() {
                    if let ExprKind::Spread { argument } = &arena.expr(*element).kind {
                        self.check_destructure_target(rec, *argument, Type::Dynamic, bind);
                    } else {
                        let item = Self::sequence_item(&shape, at);
                        self.check_destructure_target(rec, *element, item, bind);
                    }
                }
            }
            ExprKind::Array { elements } => {
                let fixed_count = elements
                    .iter()
                    .filter(|e| !matches!(e, ArrayEl::Spread(_)))
                    .count();
                let has_rest = fixed_count != elements.len();
                let shape = self.sequence_shape(&ty, false, fixed_count, has_rest, range, bind);
                for (at, element) in elements.iter().enumerate() {
                    match element {
                        ArrayEl::Expr(e) => {
                            let item = Self::sequence_item(&shape, at);
                            self.check_destructure_target(rec, *e, item, bind);
                        }
                        ArrayEl::Spread(e) => {
                            let rest =
                                Type::array(shape.element, Arc::make_mut(&mut self.ty_table));
                            self.check_destructure_target(rec, *e, rest, bind);
                        }
                        ArrayEl::Hole => {}
                    }
                }
            }
            ExprKind::Object { properties } => {
                for property in properties {
                    self.check_destructure_property(rec, property, &ty, bind);
                }
            }
            ExprKind::Assign {
                op: varn_core::ast::AssignOp::Assign,
                target: inner,
                value: default,
            } => {
                let present = if ty.is_unresolved() {
                    ty
                } else {
                    ty.non_nullified(Arc::make_mut(&mut self.ty_table))
                };
                let expected = (!present.is_unresolved()).then_some(present);
                self.with_expected(expected, |c| c.check_expr(rec, *default, bind));
                self.check_destructure_target(rec, *inner, present, bind);
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::ImportMeta(_)
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
            | ExprKind::MetaAccess { .. } => self.check_destructure_leaf(rec, target, ty, bind),
        }
    }

    fn check_destructure_property(
        &mut self,
        rec: &mut Recorder,
        property: &ObjectProp,
        ty: &Type,
        bind: &BindResult,
    ) {
        match property {
            ObjectProp::Property {
                key, value, range, ..
            } => {
                let member = match key {
                    PropKey::Identifier(name) | PropKey::Str(name) => name.clone(),
                    PropKey::Int(_) | PropKey::Computed(_) => {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::NotAssignable,
                                "a destructuring target names its fields: a computed or numeric key is not statically known",
                            )
                            .with_range(*range),
                        );
                        return;
                    }
                };
                let member_ty = if ty.is_unresolved() {
                    Type::Dynamic
                } else if let Some((found, _)) = self.find_member_info(ty, &member, bind) {
                    found
                } else {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::MissingProperty,
                            format!(
                                "property '{member}' does not exist on type '{}'",
                                ty.display(&self.ty_table, &bind.interner)
                            ),
                        )
                        .with_range(*range),
                    );
                    Type::Dynamic
                };
                self.check_destructure_target(rec, *value, member_ty, bind);
            }
            ObjectProp::Spread { argument, .. } => {
                self.check_destructure_target(rec, *argument, Type::Dynamic, bind);
            }
            ObjectProp::Method { range, .. } => {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::NotAssignable,
                        "a method cannot be a destructuring target",
                    )
                    .with_range(*range),
                );
            }
        }
    }

    fn sequence_item(shape: &SequenceShape, at: usize) -> Type {
        match &shape.fixed {
            Some(items) => items.get(at).copied().unwrap_or(Type::Dynamic),
            None => shape.element,
        }
    }

    fn check_destructure_leaf(
        &mut self,
        rec: &mut Recorder,
        leaf: ExprId,
        item_ty: Type,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let range = arena.expr(leaf).range;
        if !matches!(
            arena.expr(leaf).kind,
            ExprKind::Identifier { .. } | ExprKind::Member { .. }
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "invalid destructuring target: only variables and members can be assigned",
                )
                .with_range(range),
            );
            return;
        }
        let prev = self.is_assignment_target;
        self.is_assignment_target = true;
        self.check_expr(rec, leaf, bind);
        self.is_assignment_target = prev;
        self.check_extension_assignment(rec, leaf, bind);
        if let ExprKind::Identifier { name } = &arena.expr(leaf).kind {
            self.check_reassignable(*name, range, bind);
        }
        let target_ty = self.assign_target_type(rec, leaf, bind);
        if !target_ty.is_unresolved()
            && !item_ty.is_unresolved()
            && !self.types_compatible_cached(&target_ty, &item_ty, Some(bind))
        {
            self.emit(
                Diagnostic::error(
                    ErrorCode::TypeMismatch,
                    format!(
                        "type mismatch: cannot assign '{}' to '{}'",
                        item_ty.display(&self.ty_table, &bind.interner),
                        target_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(range),
            );
        }
        self.check_pure_assign_target(leaf, range, bind);
    }
}
