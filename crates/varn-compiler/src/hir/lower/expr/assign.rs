use varn_core::ast::operators::{AssignOp};
use varn_core::ast::{Expr, ExprKind};

use super::*;

impl<'a> Lowerer<'a> {
    /// Assignment (plain, compound, destructuring) and update expressions.
    pub(super) fn lower_assign_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        let offset = expr.range.start.offset;
        match &expr.kind {
            ExprKind::Assign { op, target, value } => {
                if let ExprKind::Member {
                    object,
                    property,
                    computed,
                    ..
                } = &target.kind
                {
                    let off = target.range.start.offset;

                    if let Some(mangled) = self.extension_set_members.get(&off).cloned() {
                        let recv = self.lower_expr(object, scope)?;
                        let val = self.lower_expr(value, scope)?;
                        return Ok(self.ext_global_call(mangled, recv, vec![val]));
                    }
                    if matches!(object.kind, ExprKind::Super) {
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            if matches!(op, AssignOp::Assign) {
                                let tgt = HirAssignTarget::SuperIndex { index };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(value),
                                });
                            } else {
                                let bop = compound_to_bin(*op)?;
                                let ty = numeric_ty(self.ann, target.range.start.offset);
                                let current_val = HirExpr::Index {
                                    object: Box::new(HirExpr::Super),
                                    index: Box::new(index.clone()),
                                    ty: HirType::Dynamic,
                                    is_array: false,
                                };
                                let new_val = HirExpr::Binary {
                                    op: bop,
                                    lhs: Box::new(current_val),
                                    rhs: Box::new(value),
                                    ty,
                                };
                                let tgt = HirAssignTarget::SuperIndex { index };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(new_val),
                                });
                            }
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => {
                                    return Err(OptError::Unsupported(
                                        "hir: non-identifier super property assign",
                                    ))
                                }
                            };
                            let value = self.lower_expr(value, scope)?;
                            if matches!(op, AssignOp::Assign) {
                                let tgt = HirAssignTarget::SuperMember { name };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(value),
                                });
                            } else {
                                let bop = compound_to_bin(*op)?;
                                let ty = numeric_ty(self.ann, target.range.start.offset);
                                let current_val = HirExpr::SuperMember { name: name.clone() };
                                let new_val = HirExpr::Binary {
                                    op: bop,
                                    lhs: Box::new(current_val),
                                    rhs: Box::new(value),
                                    ty,
                                };
                                let tgt = HirAssignTarget::SuperMember { name };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(new_val),
                                });
                            }
                        }
                    }
                    if matches!(
                        op,
                        AssignOp::AndAssign | AssignOp::OrAssign | AssignOp::NullishAssign
                    ) {
                        let lop = match op {
                            AssignOp::AndAssign => HirLogicalOp::And,
                            AssignOp::OrAssign => HirLogicalOp::Or,
                            AssignOp::NullishAssign => HirLogicalOp::Nullish,
                            _ => unreachable!(),
                        };
                        let ty = numeric_ty(self.ann, target.range.start.offset);
                        let object_hir = self.lower_expr(object, scope)?;
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            let is_arr = self.ann.get_array_index(target.range.start.offset);
                            let current_val = HirExpr::Index {
                                object: Box::new(object_hir.clone()),
                                index: Box::new(index.clone()),
                                ty,
                                is_array: is_arr,
                            };
                            let tgt = HirAssignTarget::Index {
                                object: object_hir,
                                index,
                                is_array: is_arr,
                            };
                            let assign = HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(value),
                            };
                            return Ok(HirExpr::Logical {
                                op: lop,
                                lhs: Box::new(current_val),
                                rhs: Box::new(assign),
                            });
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => {
                                    return Err(OptError::Unsupported(
                                        "hir: non-identifier property assign",
                                    ))
                                }
                            };
                            let value = self.lower_expr(value, scope)?;
                            let current_val = HirExpr::Member {
                                object: Box::new(object_hir.clone()),
                                name: name.clone(),
                                ty,
                            };
                            let tgt = HirAssignTarget::Member {
                                object: object_hir,
                                name,
                            };
                            let assign = HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(value),
                            };
                            return Ok(HirExpr::Logical {
                                op: lop,
                                lhs: Box::new(current_val),
                                rhs: Box::new(assign),
                            });
                        }
                    }
                    if !matches!(op, AssignOp::Assign) {
                        let bop = compound_to_bin(*op)?;
                        let ty = numeric_ty(self.ann, target.range.start.offset);
                        let object_hir = self.lower_expr(object, scope)?;
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            let is_arr = self.ann.get_array_index(target.range.start.offset);
                            let current_val = HirExpr::Index {
                                object: Box::new(object_hir.clone()),
                                index: Box::new(index.clone()),
                                ty,
                                is_array: is_arr,
                            };
                            let new_val = HirExpr::Binary {
                                op: bop,
                                lhs: Box::new(current_val),
                                rhs: Box::new(value),
                                ty,
                            };
                            let tgt = HirAssignTarget::Index {
                                object: object_hir,
                                index,
                                is_array: is_arr,
                            };
                            return Ok(HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(new_val),
                            });
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => {
                                    return Err(OptError::Unsupported(
                                        "hir: non-identifier property assign",
                                    ))
                                }
                            };
                            let value = self.lower_expr(value, scope)?;
                            let current_val = HirExpr::Member {
                                object: Box::new(object_hir.clone()),
                                name: name.clone(),
                                ty,
                            };
                            let new_val = HirExpr::Binary {
                                op: bop,
                                lhs: Box::new(current_val),
                                rhs: Box::new(value),
                                ty,
                            };
                            let tgt = HirAssignTarget::Member {
                                object: object_hir,
                                name,
                            };
                            return Ok(HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(new_val),
                            });
                        }
                    }
                    let object_hir = self.lower_expr(object, scope)?;
                    let tgt = if *computed {
                        let index = self.lower_expr(property, scope)?;
                        let is_array = self.ann.get_array_index(target.range.start.offset);
                        HirAssignTarget::Index {
                            object: object_hir,
                            index,
                            is_array,
                        }
                    } else {
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => {
                                return Err(OptError::Unsupported(
                                    "hir: non-identifier property assign",
                                ))
                            }
                        };
                        HirAssignTarget::Member {
                            object: object_hir,
                            name,
                        }
                    };
                    let v = self.lower_expr(value, scope)?;
                    return Ok(HirExpr::Assign {
                        target: Box::new(tgt),
                        value: Box::new(v),
                    });
                }
                let binding = match &target.kind {
                    ExprKind::Identifier { name } => self.resolve(name, scope),
                    _ => return Err(OptError::Unsupported("hir: non-identifier assign target")),
                };
                let val_expr = self.lower_expr(value, scope)?;
                if matches!(
                    op,
                    AssignOp::AndAssign | AssignOp::OrAssign | AssignOp::NullishAssign
                ) {
                    let lop = match op {
                        AssignOp::AndAssign => HirLogicalOp::And,
                        AssignOp::OrAssign => HirLogicalOp::Or,
                        AssignOp::NullishAssign => HirLogicalOp::Nullish,
                        _ => unreachable!(),
                    };
                    let lhs = HirExpr::Var(binding.clone());
                    let assign = HirExpr::Assign {
                        target: Box::new(HirAssignTarget::Var(binding)),
                        value: Box::new(val_expr),
                    };
                    return Ok(HirExpr::Logical {
                        op: lop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(assign),
                    });
                }
                let value = match op {
                    AssignOp::Assign => val_expr,
                    _ => {
                        let bop = compound_to_bin(*op)?;
                        let ty = numeric_ty(self.ann, offset);
                        HirExpr::Binary {
                            op: bop,
                            lhs: Box::new(HirExpr::Var(binding.clone())),
                            rhs: Box::new(val_expr),
                            ty,
                        }
                    }
                };
                Ok(HirExpr::Assign {
                    target: Box::new(HirAssignTarget::Var(binding)),
                    value: Box::new(value),
                })
            }
            ExprKind::Update {
                op,
                prefix,
                operand,
            } => {
                let target = self.lower_assign_target(operand, scope)?;
                Ok(HirExpr::Update {
                    target: Box::new(target),
                    op: update_op(*op),
                    prefix: *prefix,
                })
            }
            other => unreachable!("lower_assign_expr: {other:?} is not handled here"),
        }
    }
}
