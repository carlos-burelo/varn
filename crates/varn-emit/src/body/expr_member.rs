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
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
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
            | ExprKind::MetaAccess { .. } => None,
        }
    }

    pub(super) fn keyed_read_ty(&mut self, container: BackendTy, slot: BackendTy) -> BackendTy {
        match container.non_nullable(self.tt) {
            BackendTy::Map(..) | BackendTy::Dict(_) => self.nullable_of(slot),
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => slot,
        }
    }

    pub(super) fn null_or(&mut self, is_null: TirExpr, value: TirExpr, span: Span) -> TirExpr {
        let result_ty = self.nullable_of(value.ty);
        let value = self.cast_to(value, result_ty);
        self.null_or_typed(is_null, value, result_ty, span)
    }

    pub(super) fn nullable_of(&mut self, ty: BackendTy) -> BackendTy {
        match ty {
            BackendTy::Nullable(_) | BackendTy::Dynamic(_) | BackendTy::Void => ty,
            member @ BackendTy::Int
            | member @ BackendTy::Float
            | member @ BackendTy::Bool
            | member @ BackendTy::Char
            | member @ BackendTy::Str
            | member @ BackendTy::Bytes
            | member @ BackendTy::Decimal
            | member @ BackendTy::BigInt
            | member @ BackendTy::Array(_)
            | member @ BackendTy::Map(..)
            | member @ BackendTy::Dict(_)
            | member @ BackendTy::Set(_)
            | member @ BackendTy::Tuple(_)
            | member @ BackendTy::Class(_)
            | member @ BackendTy::Enum(_)
            | member @ BackendTy::Fn(_)
            | member @ BackendTy::Never => BackendTy::Nullable(self.tt.intern(member)),
        }
    }

    fn null_or_typed(
        &mut self,
        is_null: TirExpr,
        value: TirExpr,
        result_ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let null_arm = TirExpr {
            kind: TirExprKind::NullLit,
            ty: result_ty,
            res: Resolution::None,
            span,
        };
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(is_null),
                then_val: Box::new(null_arm),
                else_val: Box::new(value),
            },
            ty: result_ty,
            res: Resolution::None,
            span,
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
        if let Some(read) = self.inherited_read(object, property, computed, ty, span) {
            return read;
        }
        if !computed
            && !optional
            && let Some(key) = self.namespace_member(object, property)
        {
            return TirExpr {
                kind: TirExprKind::Var,
                ty,
                res: Resolution::GlobalSlot(self.m.globals[&key]),
                span,
            };
        }
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
            return self.null_or(is_null, access, span);
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
                BackendTy::Map(_, val) | BackendTy::Dict(val) => self.tt.get(val),
                BackendTy::Tuple(slots) => {
                    let literal = if let TirExprKind::IntLit(at) = index.kind {
                        usize::try_from(at).ok()
                    } else {
                        None
                    };
                    literal
                        .filter(|_| self.tt.contains_list(slots))
                        .and_then(|at| self.tt.get_list(slots).get(at).copied())
                        .unwrap_or(ty)
                }
                BackendTy::Int
                | BackendTy::Float
                | BackendTy::Bool
                | BackendTy::Char
                | BackendTy::Str
                | BackendTy::Bytes
                | BackendTy::Decimal
                | BackendTy::BigInt
                | BackendTy::Set(_)
                | BackendTy::Class(_)
                | BackendTy::Enum(_)
                | BackendTy::Fn(_)
                | BackendTy::Nullable(_)
                | BackendTy::Void
                | BackendTy::Never
                | BackendTy::Dynamic(_) => ty,
            };
            let node_ty = self.keyed_read_ty(obj.ty, node_ty);
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

    fn inherited_read(
        &mut self,
        object: ExprId,
        property: ExprId,
        computed: bool,
        ty: BackendTy,
        span: Span,
    ) -> Option<TirExpr> {
        if computed || !matches!(self.ast_arena.expr(object).kind, ExprKind::Super) {
            return None;
        }
        let name = Self::member_name(self.ast_arena, property, self.m.interner)?;
        let at = self.ast_arena.expr(property).range.start.offset;
        if self.m.desugar.inherited_fields.contains(&at) {
            let this = self.this_var(span);
            return Some(self.field_access(this, name, ty, span));
        }
        let accessor = self.m.desugar.inherited_getters.contains(&at);
        let member = self.inherited_member(&name, accessor, span)?;
        if !accessor {
            return Some(member);
        }
        Some(TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(member),
                args: vec![],
            },
            ty,
            res: Resolution::None,
            span,
        })
    }

    pub(super) fn field_access(
        &mut self,
        obj: TirExpr,
        name: Arc<str>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if let Some(ci) = self.class_of(obj.ty) {
            if Self::is_class_ref(&obj, &ci.name, self.m.globals) {
                if let Some(slot) = ci.static_slot(&name) {
                    let declared = ci.static_at(slot).map(|s| s.ty).unwrap_or(ty);
                    let ty = if ty != declared && ty == declared.non_nullable(self.tt) {
                        ty
                    } else {
                        declared
                    };
                    return TirExpr {
                        kind: TirExprKind::Field {
                            object: Box::new(obj),
                            name,
                        },
                        ty,
                        res: Resolution::StaticField(slot),
                        span,
                    };
                }
                return TirExpr {
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
                };
            }
            if let Some(field) = ci.field(&name).cloned() {
                let ty = if ty == field.ty || ty == field.ty.non_nullable(self.tt) {
                    ty
                } else if matches!(field.ty, BackendTy::Dynamic(_))
                    && !matches!(ty, BackendTy::Dynamic(_))
                {
                    ty
                } else {
                    field.ty
                };
                return TirExpr {
                    kind: TirExprKind::Field {
                        object: Box::new(obj),
                        name,
                    },
                    ty,
                    res: Resolution::FieldSlot(field.slot),
                    span,
                };
            }
        }
        TirExpr {
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
        }
    }

    pub(super) fn is_class_ref(
        obj: &TirExpr,
        class_name: &Arc<str>,
        globals: &rustc_hash::FxHashMap<Arc<str>, u32>,
    ) -> bool {
        if !matches!(obj.kind, varn_tir::TirExprKind::Var) {
            return false;
        }
        let varn_tir::Resolution::GlobalSlot(slot) = obj.res else {
            return false;
        };
        globals.get(class_name).is_some_and(|s| *s == slot)
    }
}
