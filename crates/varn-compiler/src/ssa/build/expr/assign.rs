use crate::hir::{HirAssignTarget, HirBinOp, HirExpr, HirType, HirUpdateOp};
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;

use super::{Builder, Result};

impl Builder {
    /// Assignment and update (`x += 1`, `obj.p++`) over every target shape.
    pub(super) fn lower_assign_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Assign { target, value } => match &**target {
                HirAssignTarget::Var(binding) => {
                    let v = self.lower_expr(value)?;
                    self.store_binding(binding, v);
                    Ok(v)
                }
                HirAssignTarget::Member { object, name } => {
                    let o = self.lower_expr(object)?;
                    let v = self.lower_expr(value)?;
                    self.emit_effect(InstKind::SetProperty {
                        object: o,
                        name: name.clone(),
                        value: v,
                    });
                    Ok(v)
                }
                HirAssignTarget::SetFixedField { object, slot } => {
                    let o = self.lower_expr(object)?;
                    let v = self.lower_expr(value)?;
                    self.emit_effect(InstKind::SetFixedField {
                        object: o,
                        value: v,
                        slot: *slot,
                    });
                    Ok(v)
                }
                HirAssignTarget::Index {
                    object,
                    index,
                    is_array,
                } => {
                    let o = self.lower_expr(object)?;
                    let i = self.lower_expr(index)?;
                    let v = self.lower_expr(value)?;
                    if *is_array {
                        self.emit_effect(InstKind::ArraySetIndex {
                            object: o,
                            index: i,
                            value: v,
                        });
                    } else {
                        self.emit_effect(InstKind::SetIndex {
                            object: o,
                            index: i,
                            value: v,
                        });
                    }
                    Ok(v)
                }
                HirAssignTarget::ModuleSlot { slot } => {
                    let v = self.lower_expr(value)?;
                    self.emit_effect(InstKind::StoreModuleSlot {
                        value: v,
                        slot: *slot,
                    });
                    Ok(v)
                }
                HirAssignTarget::SuperMember { name } => {
                    let v = self.lower_expr(value)?;
                    let this_val = self.emit(InstKind::This, HirType::Ref);
                    self.emit_effect(InstKind::SetProperty {
                        object: this_val,
                        name: name.clone(),
                        value: v,
                    });
                    Ok(v)
                }
                HirAssignTarget::SuperIndex { index } => {
                    let i = self.lower_expr(index)?;
                    let v = self.lower_expr(value)?;
                    let this_val = self.emit(InstKind::This, HirType::Ref);
                    self.emit_effect(InstKind::SetIndex {
                        object: this_val,
                        index: i,
                        value: v,
                    });
                    Ok(v)
                }
            },

            HirExpr::Update { target, op, prefix } => match &**target {
                HirAssignTarget::Var(binding) => {
                    let old = self.load_binding(binding)?;
                    let ty = self.values[old.0 as usize].ty;
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary {
                            op: bop,
                            lhs: old,
                            rhs: one,
                            ty,
                        },
                        ty,
                    );
                    self.store_binding(binding, new);
                    Ok(if *prefix { new } else { old })
                }

                HirAssignTarget::Member { object, name } => {
                    let o = self.lower_expr(object)?;
                    let old = self.emit(
                        InstKind::GetProperty {
                            object: o,
                            name: name.clone(),
                        },
                        HirType::Dynamic,
                    );
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary {
                            op: bop,
                            lhs: old,
                            rhs: one,
                            ty: HirType::Dynamic,
                        },
                        HirType::Dynamic,
                    );
                    self.emit_effect(InstKind::SetProperty {
                        object: o,
                        name: name.clone(),
                        value: new,
                    });
                    Ok(if *prefix { new } else { old })
                }
                HirAssignTarget::SetFixedField { object, slot } => {
                    let o = self.lower_expr(object)?;
                    let old = self.emit(
                        InstKind::GetFixedField {
                            object: o,
                            slot: *slot,
                        },
                        HirType::Dynamic,
                    );
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary {
                            op: bop,
                            lhs: old,
                            rhs: one,
                            ty: HirType::Dynamic,
                        },
                        HirType::Dynamic,
                    );
                    self.emit_effect(InstKind::SetFixedField {
                        object: o,
                        value: new,
                        slot: *slot,
                    });
                    Ok(if *prefix { new } else { old })
                }

                HirAssignTarget::Index {
                    object,
                    index,
                    is_array,
                } => {
                    let o = self.lower_expr(object)?;
                    let i = self.lower_expr(index)?;
                    let get_kind = if *is_array {
                        InstKind::ArrayGetIndex {
                            object: o,
                            index: i,
                        }
                    } else {
                        InstKind::GetIndex {
                            object: o,
                            index: i,
                        }
                    };
                    let old = self.emit(get_kind, HirType::Dynamic);
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary {
                            op: bop,
                            lhs: old,
                            rhs: one,
                            ty: HirType::Dynamic,
                        },
                        HirType::Dynamic,
                    );
                    if *is_array {
                        self.emit_effect(InstKind::ArraySetIndex {
                            object: o,
                            index: i,
                            value: new,
                        });
                    } else {
                        self.emit_effect(InstKind::SetIndex {
                            object: o,
                            index: i,
                            value: new,
                        });
                    }
                    Ok(if *prefix { new } else { old })
                }
                HirAssignTarget::ModuleSlot { .. } => {
                    Err(OptError::Unsupported("ssa: update module slot"))
                }
                HirAssignTarget::SuperMember { name } => {
                    let old =
                        self.emit(InstKind::GetSuper { name: name.clone() }, HirType::Dynamic);
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary {
                            op: bop,
                            lhs: old,
                            rhs: one,
                            ty: HirType::Dynamic,
                        },
                        HirType::Dynamic,
                    );
                    let this_val = self.emit(InstKind::This, HirType::Ref);
                    self.emit_effect(InstKind::SetProperty {
                        object: this_val,
                        name: name.clone(),
                        value: new,
                    });
                    Ok(if *prefix { new } else { old })
                }
                HirAssignTarget::SuperIndex { index } => {
                    let i = self.lower_expr(index)?;
                    let this_val = self.emit(InstKind::This, HirType::Ref);
                    let old = self.emit(
                        InstKind::GetIndex {
                            object: this_val,
                            index: i,
                        },
                        HirType::Dynamic,
                    );
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary {
                            op: bop,
                            lhs: old,
                            rhs: one,
                            ty: HirType::Dynamic,
                        },
                        HirType::Dynamic,
                    );
                    self.emit_effect(InstKind::SetIndex {
                        object: this_val,
                        index: i,
                        value: new,
                    });
                    Ok(if *prefix { new } else { old })
                }
            },
            other => unreachable!("lower_assign_expr: {other:?} is not handled here"),
        }
    }
}
