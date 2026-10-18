use super::context::FnEmitter;
use crate::ty::lower_type;
use std::sync::Arc;
use varn_core::ast::{Arg, AstId};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_arg(&mut self, a: &Arg) -> TirArg {
        match a {
            Arg::Positional(e) => TirArg::Expr(self.lower_expr(*e)),
            Arg::Spread(e) => TirArg::Spread(self.lower_expr(*e)),
            Arg::Named { label, value } => TirArg::Named {
                label: Arc::from(label.as_str()),
                value: self.lower_expr(*value),
            },
        }
    }

    pub(super) fn lower_call_args(&mut self, call_id: AstId, args: &[Arg]) -> Vec<TirArg> {
        let lowered = self.lower_args_in_order(call_id, args);
        let Some(checked) = self.m.desugar.call_params.get(&call_id) else {
            return lowered;
        };
        let checked = checked.clone();
        let (names, table, interner) = (self.m.names, self.m.checker_table, self.m.interner);
        let mut spread_seen = false;
        lowered
            .into_iter()
            .enumerate()
            .map(|(i, arg)| match arg {
                TirArg::Expr(e) => match checked_widen_ty(&checked, i, table) {
                    Some(ty) if !spread_seen => {
                        let param = lower_type(&ty, table, interner, self.tt, names);
                        TirArg::Expr(self.widened(e, param))
                    }
                    Some(_) | None => TirArg::Expr(e),
                },
                spread @ TirArg::Spread(_) => {
                    spread_seen = true;
                    spread
                }
                named @ TirArg::Named { .. } => named,
            })
            .collect()
    }

    pub(super) fn checked_signature(
        &mut self,
        call_id: AstId,
        return_ty: BackendTy,
    ) -> Option<varn_tir::SigId> {
        let (params, has_rest) = self.checked_shape(call_id)?;
        let sig = varn_tir::Signature {
            params,
            return_ty,
            has_rest,
        };
        let at = match self.signatures.iter().position(|known| *known == sig) {
            Some(at) => at,
            None => {
                self.signatures.push(sig);
                self.signatures.len() - 1
            }
        };
        Some(varn_tir::SigId(at as u32))
    }

    pub(super) fn widened(&self, arg: TirExpr, param: BackendTy) -> TirExpr {
        let target = param.non_nullable(self.tt);
        let wider = matches!(
            target,
            BackendTy::Float | BackendTy::Decimal | BackendTy::BigInt
        );
        if arg.ty == BackendTy::Int && wider {
            self.cast_to(arg, target)
        } else {
            arg
        }
    }

    pub(super) fn checked_shape(
        &mut self,
        call_id: varn_core::ast::AstId,
    ) -> Option<(Vec<BackendTy>, bool)> {
        let checked = self.m.desugar.call_params.get(&call_id)?.clone();
        let (names, table, interner) = (self.m.names, self.m.checker_table, self.m.interner);
        let params = checked
            .iter()
            .map(|param| {
                let lowered = lower_type(&param.ty, table, interner, self.tt, names);
                let boxed = matches!(lowered, BackendTy::Nullable(_) | BackendTy::Dynamic(_));
                if param.optional && !param.is_rest && !boxed {
                    BackendTy::Nullable(self.tt.intern(lowered))
                } else {
                    lowered
                }
            })
            .collect();
        let has_rest = checked.last().is_some_and(|param| param.is_rest);
        Some((params, has_rest))
    }

    fn lower_args_in_order(&mut self, call_id: AstId, args: &[Arg]) -> Vec<TirArg> {
        match self.m.call_mappings.get(&call_id).cloned() {
            Some(mapping) => {
                let mut end = mapping.len();
                while end > 0 && mapping[end - 1].is_none() {
                    end -= 1;
                }
                mapping[..end]
                    .iter()
                    .map(|opt| match opt {
                        Some(i) => match &args[*i] {
                            Arg::Positional(e) | Arg::Named { value: e, .. } => {
                                TirArg::Expr(self.lower_expr(*e))
                            }
                            Arg::Spread(e) => TirArg::Spread(self.lower_expr(*e)),
                        },
                        None => TirArg::Expr(TirExpr {
                            kind: TirExprKind::NullLit,
                            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                            res: Resolution::None,
                            span: Span::EMPTY,
                        }),
                    })
                    .collect()
            }
            None => args.iter().map(|a| self.lower_arg(a)).collect(),
        }
    }
}

fn checked_widen_ty(
    checked: &[varn_sem::output::CheckedParam],
    at: usize,
    table: &varn_sem::types::CheckerTyTable,
) -> Option<varn_sem::types::Type> {
    let rest_inner = |param: &varn_sem::output::CheckedParam| match table.get(param.ty.0) {
        varn_core::TypeKind::Array(inner) => Some(varn_sem::types::Type::resolved(inner)),
        varn_core::TypeKind::Primitive(_)
        | varn_core::TypeKind::Literal(_)
        | varn_core::TypeKind::This
        | varn_core::TypeKind::Union(_)
        | varn_core::TypeKind::Intersection(_)
        | varn_core::TypeKind::Tuple(_)
        | varn_core::TypeKind::Named(..)
        | varn_core::TypeKind::Generic(..)
        | varn_core::TypeKind::TemplateLiteral(_)
        | varn_core::TypeKind::Fn(_)
        | varn_core::TypeKind::Object(_)
        | varn_core::TypeKind::Typeof(_)
        | varn_core::TypeKind::KeyOf(_)
        | varn_core::TypeKind::IndexedAccess { .. }
        | varn_core::TypeKind::Mapped { .. }
        | varn_core::TypeKind::Conditional { .. }
        | varn_core::TypeKind::Infer(_)
        | varn_core::TypeKind::EnumVariant { .. }
        | varn_core::TypeKind::TypePredicate { .. } => Some(param.ty),
    };
    if let Some(param) = checked.get(at) {
        if param.is_rest {
            return rest_inner(param);
        }
        return Some(param.ty);
    }
    checked
        .last()
        .filter(|param| param.is_rest)
        .and_then(rest_inner)
}
