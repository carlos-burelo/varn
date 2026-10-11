use super::call_contract::Callee;
use super::context::FnEmitter;
use crate::ty::NameResolver;
use varn_core::ast::{Arg, AstId, ExprId, ExprKind};
use varn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_new(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let class = match &self.ast_arena.expr(callee).kind {
            ExprKind::Identifier { name } => self.class_named(self.m.interner.resolve(*name)),

            ExprKind::Member {
                property,
                computed: false,
                ..
            } => Self::member_name(self.ast_arena, *property, self.m.interner)
                .and_then(|n| self.m.names.class_id(&n)),
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
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
        };
        let targs = self.lower_call_args(call_id, args);
        let built = match ty.non_nullable(self.tt) {
            BackendTy::Class(foreign) => class.or(Some(foreign)),
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => class,
        };
        let ctor_sig =
            built.and_then(|c| crate::tables::inherited_constructor(self.m.classes, c));
        let targs = self.fill_omitted(targs, ctor_sig);
        let reached_by = if let ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind {
            let name = self.m.interner.resolve(*name);
            matches!(self.resolve_name(name), Resolution::GlobalSlot(_)).then_some(name)
        } else {
            None
        };
        let imported = built.filter(|c| {
            let binding = self
                .m
                .classes
                .get(c.0 as usize)
                .and_then(|info| info.import_binding.as_deref());
            class.is_none() && ctor_sig.is_some() && binding.is_some() && binding == reached_by
        });
        let class = class.or(imported);
        let c = self.lower_expr(callee);
        let (target, targs) = self.settle_args(Callee::Value(c), targs, ctor_sig, span);
        match class {
            Some(class) => TirExpr {
                kind: TirExprKind::New { class, args: targs },
                ty,
                res: Resolution::None,
                span,
            },
            None => TirExpr {
                kind: TirExprKind::Call {
                    callee: Box::new(target.into_expr()),
                    args: targs,
                },
                ty,
                res: Resolution::None,
                span,
            },
        }
    }

    pub(super) fn enum_variant(
        &self,
        object: ExprId,
        variant: &str,
    ) -> Option<(varn_tir::EnumId, u16)> {
        let ExprKind::Identifier { name } = &self.ast_arena.expr(object).kind else {
            return None;
        };
        let name = self.m.interner.resolve(*name);
        let at = self.m.enums.iter().position(|info| {
            info.binding.as_deref() == Some(name)
                && info
                    .variant_global(variant)
                    .is_some_and(|global| self.m.globals.contains_key(&global))
        })?;
        let v = self.m.enums[at]
            .variants
            .iter()
            .find(|v| v.name.as_ref() == variant)?;
        Some((varn_tir::EnumId(at as u32), v.tag))
    }
}
