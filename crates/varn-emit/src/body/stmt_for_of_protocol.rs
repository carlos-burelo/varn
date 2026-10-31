use super::context::FnEmitter;
use super::finally_splice;
use super::small_utils::{bool_lit, int_lit};
use super::stmt_for_of::{binary, local_var};
use std::sync::Arc;
use varn_core::ast::{Pattern, StmtId};
use varn_core::{CoreSum, MemberKey};
use varn_sem::types::Type;
use varn_tir::{
    BackendTy, DynReason, LocalId, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt,
};

pub(super) struct Protocol {
    pub(super) iterator: Option<Type>,
    pub(super) next: Type,
    pub(super) step: Type,
    pub(super) is_await: bool,
}

struct OptionTags {
    some: u16,
    none: u16,
    payload: BackendTy,
}

impl FnEmitter<'_> {
    fn closing_on_exit(&mut self, it: LocalId, it_ty: BackendTy, walk: TirStmt) -> Vec<TirStmt> {
        let dyn_ty = BackendTy::Dynamic(DynReason::Runtime);
        let close = TirStmt::Expr(self.member_call(
            local_var(it, it_ty),
            Arc::from(MemberKey::IterClose.as_str()),
            vec![],
            BackendTy::Void,
            None,
            Span::EMPTY,
        ));
        let fin = vec![close];
        let caught = self.fresh_local(dyn_ty);
        let rethrow = TirStmt::Throw(local_var(caught, dyn_ty));
        let body = finally_splice::splice_finally_before_exits(vec![walk], &fin, &mut |ty| {
            self.fresh_local(ty)
        });
        let catch_body =
            finally_splice::splice_finally_before_exits_and_throw(vec![rethrow], &fin, &mut |ty| {
                self.fresh_local(ty)
            });
        let mut out = vec![TirStmt::Try {
            body,
            catch_local: caught,
            catch_body,
        }];
        out.extend(fin);
        out
    }

    fn option_tags(&mut self, step: BackendTy) -> Option<OptionTags> {
        let BackendTy::Enum(eid) = step.non_nullable(self.tt) else {
            return None;
        };
        let info = self.m.enums.get(eid.0 as usize)?;
        let variant = |name: &str| info.variants.iter().find(|v| v.name.as_ref() == name);
        let some = variant(CoreSum::Option.success_variant())?;
        let none = variant(CoreSum::Option.failure_variant())?;
        Some(OptionTags {
            some: some.tag,
            none: none.tag,
            payload: some.payload.first().copied()?,
        })
    }

    pub(super) fn for_of_protocol(
        &mut self,
        left: &Pattern,
        source: TirExpr,
        plan: &Protocol,
        body: StmtId,
    ) -> Vec<TirStmt> {
        let mut out = Vec::new();
        let step_ty = self.lower_checked(&plan.step);
        let Some(tags) = self.option_tags(step_ty) else {
            return out;
        };
        let iterator = match &plan.iterator {
            Some(ty) => {
                let ty = self.lower_checked(ty);
                self.member_call(
                    source,
                    Arc::from(MemberKey::Iter.as_str()),
                    vec![],
                    ty,
                    None,
                    Span::EMPTY,
                )
            }
            None => source,
        };
        let it_ty = iterator.ty;
        let it = self.fresh_local(it_ty);
        out.push(TirStmt::Let {
            local: it,
            ty: it_ty,
            init: Some(iterator),
        });
        out.extend(std::mem::take(&mut self.pending));

        let next_ty = self.lower_checked(&plan.next);
        let mut next_call = self.member_call(
            local_var(it, it_ty),
            Arc::from(MemberKey::IterNext.as_str()),
            vec![],
            next_ty,
            None,
            Span::EMPTY,
        );
        if plan.is_await {
            next_call = TirExpr {
                kind: TirExprKind::Await {
                    future: Box::new(next_call),
                },
                ty: step_ty,
                res: Resolution::None,
                span: Span::EMPTY,
            };
        }
        let step = self.fresh_local(step_ty);
        let step_var = || local_var(step, step_ty);
        let finished = binary(
            TirBinOp::Eq,
            TirExpr {
                kind: TirExprKind::Discriminant {
                    value: Box::new(step_var()),
                },
                ty: BackendTy::Int,
                res: Resolution::None,
                span: Span::EMPTY,
            },
            int_lit(i64::from(tags.none)),
            BackendTy::Bool,
        );
        let value = TirExpr {
            kind: TirExprKind::VariantPayload {
                value: Box::new(step_var()),
                tag: tags.some,
                field: 0,
            },
            ty: tags.payload,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let mut loop_body = std::mem::take(&mut self.pending);
        loop_body.push(TirStmt::Let {
            local: step,
            ty: step_ty,
            init: Some(next_call),
        });
        loop_body.push(TirStmt::If {
            cond: finished,
            then_body: vec![TirStmt::Break],
            else_body: vec![],
        });
        self.bind_pattern(left, value, &mut loop_body);
        loop_body.extend(self.lower_stmt_as_block(body));
        let walk = TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        };
        if plan.is_await || !matches!(it_ty, BackendTy::Dynamic(DynReason::Yields(_))) {
            out.push(walk);
            return out;
        }
        out.extend(self.closing_on_exit(it, it_ty, walk));
        out
    }
}
