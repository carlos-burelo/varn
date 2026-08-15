use crate::hir::{HirArrayEl, HirExpr, HirObjectProp, HirPropKey, HirType};
use crate::ssa::ir::{InstKind, Value};

use super::{Builder, Result};

impl Builder {
    /// Array, tuple, object and record construction.
    pub(super) fn lower_collection_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Array(els) => {
                if els.iter().any(|e| matches!(e, HirArrayEl::Spread(_))) {
                    let mut elements = Vec::with_capacity(els.len());
                    for el in els {
                        let item = match el {
                            HirArrayEl::Expr(e) => (self.lower_expr(e)?, false),
                            HirArrayEl::Spread(e) => (self.lower_expr(e)?, true),
                            HirArrayEl::Hole => {
                                (self.emit(InstKind::ConstNull, HirType::Dynamic), false)
                            }
                        };
                        elements.push(item);
                    }
                    return Ok(self.emit(InstKind::BuildArraySpread { elements }, HirType::Ref));
                }
                let mut vals = Vec::with_capacity(els.len());
                for el in els {
                    match el {
                        HirArrayEl::Expr(e) => vals.push(self.lower_expr(e)?),
                        HirArrayEl::Hole => {
                            vals.push(self.emit(InstKind::ConstNull, HirType::Dynamic))
                        }
                        HirArrayEl::Spread(_) => unreachable!("spread handled above"),
                    }
                }
                Ok(self.emit(InstKind::BuildArray { elements: vals }, HirType::Ref))
            }

            HirExpr::Tuple(els) => {
                let mut vals = Vec::with_capacity(els.len());
                for el in els {
                    match el {
                        HirArrayEl::Expr(e) => vals.push(self.lower_expr(e)?),
                        HirArrayEl::Hole => {
                            vals.push(self.emit(InstKind::ConstNull, HirType::Dynamic))
                        }
                        HirArrayEl::Spread(_) => {}
                    }
                }
                Ok(self.emit(InstKind::BuildTuple { elements: vals }, HirType::Ref))
            }

            HirExpr::Object { properties } => {
                let has_computed_or_method = properties.iter().any(|p| match p {
                    HirObjectProp::Property {
                        key: HirPropKey::Computed(_),
                        ..
                    } => true,
                    HirObjectProp::Method { .. } => true,
                    _ => false,
                });
                if has_computed_or_method {
                    let obj = self.emit(InstKind::BuildObject { pairs: Vec::new() }, HirType::Ref);
                    for prop in properties {
                        match prop {
                            HirObjectProp::Property { key, value } => {
                                let v = self.lower_expr(value)?;
                                match key {
                                    HirPropKey::Static(k) => {
                                        self.emit_effect(InstKind::SetProperty {
                                            object: obj,
                                            name: k.clone(),
                                            value: v,
                                        });
                                    }
                                    HirPropKey::Computed(e) => {
                                        let k = self.lower_expr(e)?;
                                        self.emit_effect(InstKind::SetIndex {
                                            object: obj,
                                            index: k,
                                            value: v,
                                        });
                                    }
                                }
                            }
                            HirObjectProp::Method {
                                key,
                                func,
                                upvalues,
                            } => {
                                let v = self.lower_closure(func, upvalues)?;
                                match key {
                                    HirPropKey::Static(k) => {
                                        self.emit_effect(InstKind::SetProperty {
                                            object: obj,
                                            name: k.clone(),
                                            value: v,
                                        });
                                    }
                                    HirPropKey::Computed(e) => {
                                        let k = self.lower_expr(e)?;
                                        self.emit_effect(InstKind::SetIndex {
                                            object: obj,
                                            index: k,
                                            value: v,
                                        });
                                    }
                                }
                            }
                            HirObjectProp::Spread(e) => {
                                let v = self.lower_expr(e)?;
                                self.emit_effect(InstKind::ObjectMerge {
                                    target: obj,
                                    source: v,
                                });
                            }
                        }
                    }
                    return Ok(obj);
                }
                if properties
                    .iter()
                    .any(|p| matches!(p, HirObjectProp::Spread(_)))
                {
                    let mut parts = Vec::with_capacity(properties.len());
                    for prop in properties {
                        match prop {
                            HirObjectProp::Property {
                                key: HirPropKey::Static(k),
                                value,
                            } => {
                                let v = self.lower_expr(value)?;
                                parts.push((Some(k.clone()), v));
                            }
                            HirObjectProp::Spread(e) => {
                                let v = self.lower_expr(e)?;
                                parts.push((None, v));
                            }
                            _ => unreachable!(),
                        }
                    }
                    return Ok(self.emit(InstKind::BuildObjectSpread { parts }, HirType::Ref));
                }
                let mut pairs = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        HirObjectProp::Property {
                            key: HirPropKey::Static(k),
                            value,
                        } => {
                            let v = self.lower_expr(value)?;
                            pairs.push((k.clone(), v));
                        }
                        _ => unreachable!(),
                    }
                }
                Ok(self.emit(InstKind::BuildObject { pairs }, HirType::Ref))
            }

            HirExpr::Record { properties } => {
                let mut pairs = Vec::with_capacity(properties.len());
                for prop in properties {
                    if let HirObjectProp::Property {
                        key: HirPropKey::Static(k),
                        value,
                    } = prop
                    {
                        let v = self.lower_expr(value)?;
                        pairs.push((k.clone(), v));
                    }
                }
                Ok(self.emit(InstKind::BuildRecord { pairs }, HirType::Ref))
            }

            other => unreachable!("lower_collection_expr: {other:?} is not handled here"),
        }
    }
}
