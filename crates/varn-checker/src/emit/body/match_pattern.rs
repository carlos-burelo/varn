use super::context::FnEmitter;
use super::small_utils::bool_lit;
use crate::emit::ty::NameResolver;
use std::sync::Arc;
use varn_core::ast::pattern::{MatchBinding, MatchPattern};
use varn_tir::{
    BackendTy, DynReason, LocalId, Resolution, TirBinOp, TirExpr, TirExprKind, TirStmt,
};

#[derive(Clone, Copy)]
pub(super) enum MatchDest {
    Statement,
    Return,
    Assign(LocalId),
}

impl<'a> FnEmitter<'a> {
    pub(super) fn match_pattern(
        &mut self,
        s: &TirExpr,
        pat: &MatchPattern,
    ) -> (TirExpr, Vec<TirStmt>) {
        match pat {
            MatchPattern::Wildcard => (bool_lit(true), vec![]),
            MatchPattern::Identifier(name) => {
                let name_str = self.m.interner.resolve(*name);
                if let BackendTy::Enum(eid) = s.ty.non_nullable(self.tt) {
                    if let Some(info) = self.m.enums.get(eid.0 as usize) {
                        if info.variants.iter().any(|v| v.name.as_ref() == name_str) {
                            return self.match_enum_variant(s, name_str, name_str, &[]);
                        }
                    }
                }
                let local = self.bind_local(Arc::from(name_str), s.ty);
                (
                    bool_lit(true),
                    vec![TirStmt::Let {
                        local,
                        ty: s.ty,
                        init: Some(s.clone()),
                    }],
                )
            }
            MatchPattern::Literal(lit) => {
                let l = self.lower_expr(*lit);
                let cond = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Eq,
                        lhs: Box::new(s.clone()),
                        rhs: Box::new(l),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: s.span,
                };
                (cond, vec![])
            }
            MatchPattern::Type { type_name, binding } => {
                let Some(cid) = self.m.names.class_id(self.m.interner.resolve(*type_name)) else {
                    return (bool_lit(false), vec![]);
                };
                let cond = TirExpr {
                    kind: TirExprKind::TypeTest {
                        value: Box::new(s.clone()),
                        class: cid,
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: s.span,
                };
                let mut binds = vec![];
                if let Some(name) = binding {
                    let local = self.bind_local(
                        Arc::from(self.m.interner.resolve(*name)),
                        BackendTy::Class(cid),
                    );
                    binds.push(TirStmt::Let {
                        local,
                        ty: BackendTy::Class(cid),
                        init: Some(s.clone()),
                    });
                }
                (cond, binds)
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                bindings,
            } => {
                let enum_name = self.m.interner.resolve(*enum_name);
                let variant_name = self.m.interner.resolve(*variant_name);
                self.match_enum_variant(s, enum_name, variant_name, bindings)
            }

            MatchPattern::Record { .. } | MatchPattern::Sequence(_) => (bool_lit(false), vec![]),
        }
    }

    pub(super) fn match_enum_variant(
        &mut self,
        s: &TirExpr,
        enum_name: &str,
        variant_name: &str,
        bindings: &[MatchBinding],
    ) -> (TirExpr, Vec<TirStmt>) {
        let eid = self
            .m
            .names
            .enum_id(enum_name)
            .or_else(|| match s.ty.non_nullable(self.tt) {
                BackendTy::Enum(e) => Some(e),
                BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char | BackendTy::Str | BackendTy::Bytes | BackendTy::Decimal | BackendTy::BigInt | BackendTy::Array(_) | BackendTy::Map(..) | BackendTy::Set(_) | BackendTy::Tuple(_) | BackendTy::Class(_) | BackendTy::Fn(_) | BackendTy::Nullable(_) | BackendTy::Void | BackendTy::Never | BackendTy::Dynamic(_) => None,
            });

        let Some(eid) = eid else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let Some(info) = self.m.enums.get(eid.0 as usize) else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let Some(variant) = info
            .variants
            .iter()
            .find(|v| v.name.as_ref() == variant_name)
        else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let tag = variant.tag;
        let payload: Vec<BackendTy> = variant.payload.clone();

        let disc = TirExpr {
            kind: TirExprKind::Discriminant {
                value: Box::new(s.clone()),
            },
            ty: BackendTy::Int,
            res: Resolution::None,
            span: s.span,
        };
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Eq,
                lhs: Box::new(disc),
                rhs: Box::new(TirExpr {
                    kind: TirExprKind::IntLit(tag as i64),
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: s.span,
                }),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span: s.span,
        };

        let mut binds = Vec::new();
        for (i, b) in bindings.iter().enumerate() {
            let fty = payload
                .get(i)
                .copied()
                .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
            let field = TirExpr {
                kind: TirExprKind::VariantPayload {
                    value: Box::new(s.clone()),
                    tag,
                    field: i as u16,
                },
                ty: fty,
                res: Resolution::EnumVariant { enum_id: eid, tag },
                span: s.span,
            };
            let local = self.bind_local(Arc::from(self.m.interner.resolve(b.name)), fty);
            binds.push(TirStmt::Let {
                local,
                ty: fty,
                init: Some(field),
            });
        }
        (cond, binds)
    }

    pub(super) fn match_variant_by_name(
        &mut self,
        s: &TirExpr,
        variant_name: &str,
        bindings: &[MatchBinding],
    ) -> (TirExpr, Vec<TirStmt>) {
        let dyn_ty = BackendTy::Dynamic(DynReason::NotYetSupported);
        let by_name = |n: &str| Resolution::ByName {
            name: Arc::from(n),
            why: DynReason::NotYetSupported,
        };
        let field = |recv: TirExpr, name: &str, ty: BackendTy| TirExpr {
            kind: TirExprKind::Field {
                object: Box::new(recv),
                name: Arc::from(name),
            },
            ty,
            res: by_name(name),
            span: s.span,
        };
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Eq,
                lhs: Box::new(field(s.clone(), "__variant_name__", BackendTy::Str)),
                rhs: Box::new(TirExpr {
                    kind: TirExprKind::StrLit(Arc::from(variant_name)),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span: s.span,
                }),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span: s.span,
        };
        let mut binds = Vec::new();
        for (i, b) in bindings.iter().enumerate() {
            let init = field(s.clone(), &format!("value{i}"), dyn_ty);
            let local = self.bind_local(Arc::from(self.m.interner.resolve(b.name)), dyn_ty);
            binds.push(TirStmt::Let {
                local,
                ty: dyn_ty,
                init: Some(init),
            });
        }
        (cond, binds)
    }
}
