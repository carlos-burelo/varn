use super::context::FnEmitter;
use super::small_utils::int_lit;
use std::sync::Arc;
use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::AtomInterner;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirArg, TirBinOp, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn member_name(
        ast_arena: &AstArena,
        property: ExprId,
        interner: &AtomInterner,
    ) -> Option<Arc<str>> {
        match &ast_arena.expr(property).kind {
            ExprKind::Identifier { name } => Some(Arc::from(interner.resolve(*name))),
            ExprKind::StrLiteral { value } => Some(Arc::from(value.as_str())),
            ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } | ExprKind::BigIntLiteral { .. } | ExprKind::DecimalLiteral { .. } | ExprKind::CharLiteral { .. } | ExprKind::BoolLiteral { .. } | ExprKind::NullLiteral | ExprKind::RegexLiteral { .. } | ExprKind::Template { .. } | ExprKind::TaggedTemplate { .. } | ExprKind::Missing | ExprKind::This | ExprKind::Super | ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } | ExprKind::Record { .. } | ExprKind::Unary { .. } | ExprKind::Update { .. } | ExprKind::Binary { .. } | ExprKind::Logical { .. } | ExprKind::Assign { .. } | ExprKind::Conditional { .. } | ExprKind::Member { .. } | ExprKind::Call { .. } | ExprKind::New { .. } | ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::Sequence { .. } | ExprKind::Paren { .. } | ExprKind::Await { .. } | ExprKind::Spawn { .. } | ExprKind::Yield { .. } | ExprKind::Spread { .. } | ExprKind::Pipeline { .. } | ExprKind::Range { .. } | ExprKind::NonNull { .. } | ExprKind::Try { .. } | ExprKind::As { .. } | ExprKind::Satisfies { .. } | ExprKind::ClassExpr { .. } | ExprKind::Match { .. } | ExprKind::Is { .. } | ExprKind::With { .. } | ExprKind::MetaAccess { .. } => None,
        }
    }

    pub(super) fn lower_member(
        &mut self,
        object: ExprId,
        property: ExprId,
        computed: bool,
        optional: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if optional && !computed {
            let name = Self::member_name(self.ast_arena, property, self.m.interner)
                .unwrap_or_else(|| Arc::from("<member>"));
            let recv = self.lower_expr(object);
            let recv = self.pin(recv);
            let is_null = TirExpr {
                kind: TirExprKind::Unary {
                    op: varn_tir::TirUnOp::IsNull,
                    operand: Box::new(recv.clone()),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
            let access = self.field_access(recv, name, ty, span);
            let result_ty = match access.ty {
                BackendTy::Nullable(_) | BackendTy::Dynamic(_) => access.ty,
                member @ BackendTy::Int | member @ BackendTy::Float | member @ BackendTy::Bool | member @ BackendTy::Char | member @ BackendTy::Str | member @ BackendTy::Bytes | member @ BackendTy::Decimal | member @ BackendTy::BigInt | member @ BackendTy::Array(_) | member @ BackendTy::Map(..) | member @ BackendTy::Set(_) | member @ BackendTy::Tuple(_) | member @ BackendTy::Class(_) | member @ BackendTy::Enum(_) | member @ BackendTy::Fn(_) | member @ BackendTy::Void | member @ BackendTy::Never => BackendTy::Nullable(self.tt.intern(member)),
            };
            let access = self.cast_to(access, result_ty);
            let null_arm = TirExpr {
                kind: TirExprKind::NullLit,
                ty: result_ty,
                res: Resolution::None,
                span,
            };
            return TirExpr {
                kind: TirExprKind::Select {
                    cond: Box::new(is_null),
                    then_val: Box::new(null_arm),
                    else_val: Box::new(access),
                },
                ty: result_ty,
                res: Resolution::None,
                span,
            };
        }

        let obj = self.lower_expr(object);

        if computed {
            if let ExprKind::Range {
                start,
                end,
                inclusive,
            } = &self.ast_arena.expr(property).kind
            {
                let (start, end, inclusive) = (*start, *end, *inclusive);
                let s = self.lower_expr(start);
                let mut e = self.lower_expr(end);
                if inclusive {
                    e = TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Add,
                            lhs: Box::new(e),
                            rhs: Box::new(int_lit(1)),
                        },
                        ty: BackendTy::Int,
                        res: Resolution::None,
                        span,
                    };
                }
                return TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(obj),
                        name: Arc::from("slice"),
                        args: vec![TirArg::Expr(s), TirArg::Expr(e)],
                    },
                    ty,
                    res: Resolution::ByName {
                        name: Arc::from("slice"),
                        why: DynReason::NotYetSupported,
                    },
                    span,
                };
            }

            let index = self.lower_expr(property);
            let node_ty = match obj.ty.non_nullable(self.tt) {
                BackendTy::Array(el) => self.tt.get(el),
                BackendTy::Map(_, val) => self.tt.get(val),
                BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char | BackendTy::Str | BackendTy::Bytes | BackendTy::Decimal | BackendTy::BigInt | BackendTy::Set(_) | BackendTy::Tuple(_) | BackendTy::Class(_) | BackendTy::Enum(_) | BackendTy::Fn(_) | BackendTy::Nullable(_) | BackendTy::Void | BackendTy::Never | BackendTy::Dynamic(_) => ty,
            };
            return TirExpr {
                kind: TirExprKind::Index {
                    object: Box::new(obj),
                    index: Box::new(index),
                },
                ty: node_ty,
                res: Resolution::None,
                span,
            };
        }

        let name = Self::member_name(self.ast_arena, property, self.m.interner)
            .unwrap_or_else(|| Arc::from("<member>"));

        if let Some(mangled) = self
            .m
            .desugar
            .extension_members
            .get(&self.ast_arena.expr(property).range.start.offset)
            .cloned()
        {
            return TirExpr {
                kind: TirExprKind::ExtensionCall {
                    func: mangled,
                    recv: Box::new(obj),
                    args: vec![],
                },
                ty,
                res: Resolution::None,
                span,
            };
        }

        if let Some((enum_id, tag)) = self.enum_variant(object, &name) {
            return TirExpr {
                kind: TirExprKind::MakeVariant { args: vec![] },
                ty: BackendTy::Enum(enum_id),
                res: Resolution::EnumVariant { enum_id, tag },
                span,
            };
        }

        self.field_access(obj, name, ty, span)
    }

    pub(super) fn field_access(
        &mut self,
        obj: TirExpr,
        name: Arc<str>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        match self
            .class_of(obj.ty)
            .and_then(|ci| ci.field(&name).cloned())
        {
            Some(field) => TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(obj),
                    name,
                },
                ty: field.ty,
                res: Resolution::FieldSlot(field.slot),
                span,
            },
            None => TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(obj),
                    name: name.clone(),
                },
                ty,
                res: Resolution::ByName {
                    name,
                    why: DynReason::NotYetSupported,
                },
                span,
            },
        }
    }
}
