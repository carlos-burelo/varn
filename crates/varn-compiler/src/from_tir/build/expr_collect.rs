use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::{Resolution, TirExpr};

impl<'m> Builder<'m> {
    pub(super) fn lower_array_lit(
        &mut self,
        els: &[varn_tir::TirArrayEl],
        ty: HirType,
    ) -> Result<Value> {
        let any_spread = els
            .iter()
            .any(|el| matches!(el, varn_tir::TirArrayEl::Spread(_)));
        if any_spread {
            let mut vals: Vec<(Value, bool)> = Vec::with_capacity(els.len());
            for el in els {
                match el {
                    varn_tir::TirArrayEl::Expr(x) => vals.push((self.lower_expr(x)?, false)),
                    varn_tir::TirArrayEl::Spread(x) => vals.push((self.lower_expr(x)?, true)),
                    varn_tir::TirArrayEl::Hole => {
                        let n = self.emit(InstKind::ConstNull, HirType::Dynamic);
                        vals.push((n, false));
                    }
                }
            }
            Ok(self.emit(InstKind::BuildArraySpread { elements: vals }, ty))
        } else {
            let mut vals = Vec::with_capacity(els.len());
            for el in els {
                match el {
                    varn_tir::TirArrayEl::Expr(x) => vals.push(self.lower_expr(x)?),
                    varn_tir::TirArrayEl::Hole => {
                        let n = self.emit(InstKind::ConstNull, HirType::Dynamic);
                        vals.push(n);
                    }
                    varn_tir::TirArrayEl::Spread(_) => unreachable!(),
                }
            }
            Ok(self.emit(InstKind::BuildArray { elements: vals }, ty))
        }
    }

    pub(super) fn lower_tuple_lit(&mut self, xs: &[TirExpr], ty: HirType) -> Result<Value> {
        let mut vals = Vec::with_capacity(xs.len());
        for x in xs {
            vals.push(self.lower_expr(x)?);
        }
        Ok(self.emit(InstKind::BuildTuple { elements: vals }, ty))
    }

    pub(super) fn lower_record_lit(
        &mut self,
        fields: &[(Arc<str>, TirExpr)],
        ty: HirType,
    ) -> Result<Value> {
        let mut pairs = Vec::with_capacity(fields.len());
        for (k, v) in fields {
            let val = self.lower_expr(v)?;
            pairs.push((k.clone(), val));
        }
        Ok(self.emit(InstKind::BuildRecord { pairs }, ty))
    }

    pub(super) fn lower_object_lit(
        &mut self,
        entries: &[varn_tir::TirObjectEntry],
        ty: HirType,
    ) -> Result<Value> {
        if matches!(ty, HirType::Map(_, _)) {
            let mut pairs = Vec::with_capacity(entries.len());
            for entry in entries {
                match entry {
                    varn_tir::TirObjectEntry::Field { name, value } => {
                        let key_val = self.emit(InstKind::ConstStr(name.clone()), HirType::Str);
                        let v = self.lower_expr(value)?;
                        pairs.push((key_val, v));
                    }
                    varn_tir::TirObjectEntry::Spread(x) => {
                        let _ = self.lower_expr(x)?;
                    }
                }
            }
            return Ok(self.emit(InstKind::BuildMap { pairs }, ty));
        }
        let any_spread = entries
            .iter()
            .any(|e| matches!(e, varn_tir::TirObjectEntry::Spread(_)));
        if any_spread {
            let mut parts: Vec<(Option<Arc<str>>, Value)> = Vec::with_capacity(entries.len());
            for entry in entries {
                match entry {
                    varn_tir::TirObjectEntry::Field { name, value } => {
                        let v = self.lower_expr(value)?;
                        parts.push((Some(name.clone()), v));
                    }
                    varn_tir::TirObjectEntry::Spread(x) => {
                        let v = self.lower_expr(x)?;
                        parts.push((None, v));
                    }
                }
            }
            Ok(self.emit(InstKind::BuildObjectSpread { parts }, ty))
        } else {
            let mut pairs = Vec::with_capacity(entries.len());
            for entry in entries {
                if let varn_tir::TirObjectEntry::Field { name, value } = entry {
                    let v = self.lower_expr(value)?;
                    pairs.push((name.clone(), v));
                }
            }
            Ok(self.emit(InstKind::BuildObject { pairs }, ty))
        }
    }

    pub(super) fn lower_assign_expr(&mut self, target: &TirExpr, value: &TirExpr) -> Result<Value> {
        let v = self.lower_expr(value)?;
        let v = match (&target.kind, &target.res) {
            (varn_tir::TirExprKind::Var, Resolution::Local(id)) => {
                match self.locals_bt.get(id.0 as usize).copied() {
                    Some(declared) => self.widen_exact(v, value.ty, declared),
                    None => v,
                }
            }
            (varn_tir::TirExprKind::Var, Resolution::GlobalSlot(n)) => {
                match self.tir.globals.get(*n as usize).copied() {
                    Some(declared) => self.widen_exact(v, value.ty, declared),
                    None => v,
                }
            }
            _ => v,
        };
        self.lower_assign(target, v)?;
        Ok(v)
    }
}
