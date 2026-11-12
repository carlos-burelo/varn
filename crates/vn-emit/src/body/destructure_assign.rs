use super::context::FnEmitter;
use super::expr_assign::AssignValue;
use std::sync::Arc;
use vn_core::ast::operators::AssignOp;
use vn_core::ast::{ArrayEl, ExprId, ExprKind, ObjectProp, PropKey};
use vn_tir::{BackendTy, DynReason, Resolution, TirExpr, TirExprKind, TirStmt, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_destructure_assign(&mut self, target: ExprId, value: ExprId) -> TirExpr {
        let src = self.lower_expr(value);
        let src = self.pin(src);
        self.assign_destructured(target, &src);
        src
    }

    fn assign_destructured(&mut self, target: ExprId, src: &TirExpr) {
        let arena = self.ast_arena;
        match &arena.expr(target).kind {
            ExprKind::Tuple { elements } => {
                for (at, element) in elements.iter().enumerate() {
                    if let ExprKind::Spread { argument } = &arena.expr(*element).kind {
                        let tail = self.tail_read(src, at);
                        self.assign_destructured(*argument, &tail);
                    } else {
                        let item = self.element_read(src, at);
                        self.assign_destructured(*element, &item);
                    }
                }
            }
            ExprKind::Array { elements } => {
                for (at, element) in elements.iter().enumerate() {
                    match element {
                        ArrayEl::Expr(e) => {
                            let item = self.element_read(src, at);
                            self.assign_destructured(*e, &item);
                        }
                        ArrayEl::Spread(e) => {
                            let tail = self.tail_read(src, at);
                            self.assign_destructured(*e, &tail);
                        }
                        ArrayEl::Hole => {}
                    }
                }
            }
            ExprKind::Object { properties } => {
                let mut named: Vec<Arc<str>> = Vec::new();
                for property in properties {
                    match property {
                        ObjectProp::Property {
                            key: PropKey::Identifier(name) | PropKey::Str(name),
                            value,
                            ..
                        } => {
                            let name: Arc<str> = Arc::from(name.as_str());
                            let item = self.field_read(src, name.clone());
                            named.push(name);
                            self.assign_destructured(*value, &item);
                        }
                        ObjectProp::Spread { argument, .. } => {
                            let rest = TirExpr {
                                kind: TirExprKind::ObjectRest {
                                    object: Box::new(src.clone()),
                                    skip_keys: named.clone(),
                                },
                                ty: BackendTy::Dynamic(DynReason::Declared),
                                res: Resolution::None,
                                span: src.span,
                            };
                            self.assign_destructured(*argument, &rest);
                        }
                        ObjectProp::Property { .. } | ObjectProp::Method { .. } => {}
                    }
                }
            }
            ExprKind::Assign {
                op: AssignOp::Assign,
                target: inner,
                value: default,
            } => {
                let fallback = self.lower_expr(*default);
                let target_ty = if fallback.ty == src.ty.non_nullable(self.tt) {
                    fallback.ty
                } else {
                    src.ty
                };
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(src.clone()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: src.span,
                };
                let chosen = TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(is_null),
                        then_val: Box::new(self.cast_to(fallback, target_ty)),
                        else_val: Box::new(self.cast_to(src.clone(), target_ty)),
                    },
                    ty: target_ty,
                    res: Resolution::None,
                    span: src.span,
                };
                self.assign_destructured(*inner, &chosen);
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
            | ExprKind::MetaAccess { .. } => self.assign_destructured_leaf(target, src.clone()),
        }
    }

    fn assign_destructured_leaf(&mut self, leaf: ExprId, value: TirExpr) {
        let span = value.span;
        if matches!(
            &self.ast_arena.expr(leaf).kind,
            ExprKind::Identifier { name }
                if self.m.interner.resolve(*name) == vn_core::TokenKind::Placeholder.as_str()
        ) {
            return;
        }
        let void = BackendTy::Void;
        let value = AssignValue::Lowered(value);
        let offset = self.ast_arena.expr(leaf).range.start.offset;
        let assigned = if matches!(&self.ast_arena.expr(leaf).kind, ExprKind::Member { .. })
            && self.m.desugar.extension_set_members.contains_key(&offset)
        {
            self.lower_extension_assign(leaf, value, void, span)
        } else if let ExprKind::Member {
            object,
            property,
            computed: true,
            optional: false,
        } = &self.ast_arena.expr(leaf).kind
        {
            let (object, property) = (*object, *property);
            self.lower_index_assign(AssignOp::Assign, object, property, value, void, span)
        } else {
            self.lower_plain_assign(AssignOp::Assign, leaf, value, void, span)
        };
        self.pending.push(TirStmt::Expr(assigned));
    }
}
