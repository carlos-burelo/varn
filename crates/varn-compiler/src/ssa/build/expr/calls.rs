use std::rc::Rc;

use crate::hir::{HirExpr, HirType};
use crate::ssa::ir::{InstKind, Value};

use super::{Builder, Result};

impl Builder {
    /// Every call and member-access form: plain calls, method calls, extension
    /// and native calls, `super`, optional chaining, fixed-slot field reads.
    pub(super) fn lower_call_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Call { callee, args, ty } => {
                let c = self.lower_expr(callee)?;
                if args.iter().any(|a| matches!(a, HirExpr::Spread(_))) {
                    let mut avs = Vec::with_capacity(args.len());
                    for a in args {
                        match a {
                            HirExpr::Spread(inner) => avs.push((self.lower_expr(inner)?, true)),
                            _ => avs.push((self.lower_expr(a)?, false)),
                        }
                    }
                    return Ok(self.emit(
                        InstKind::CallSpread {
                            callee: c,
                            args: avs,
                        },
                        *ty,
                    ));
                }
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::Call {
                        callee: c,
                        args: avs,
                    },
                    *ty,
                ))
            }

            HirExpr::ExtensionCall { func, recv, args } => {
                let r = self.lower_expr(recv)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::ExtensionCall {
                        func: func.clone(),
                        recv: r,
                        args: avs,
                    },
                    HirType::Dynamic,
                ))
            }
            HirExpr::SelfCall { args, ty } => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(InstKind::SelfCall { args: avs }, *ty))
            }
            HirExpr::Member { object, name, ty } => {
                let o = self.lower_expr(object)?;
                Ok(self.emit(
                    InstKind::GetProperty {
                        object: o,
                        name: name.clone(),
                    },
                    *ty,
                ))
            }
            HirExpr::GetFixedField { object, slot, ty } => {
                let o = self.lower_expr(object)?;
                Ok(self.emit(
                    InstKind::GetFixedField {
                        object: o,
                        slot: *slot,
                    },
                    *ty,
                ))
            }
            HirExpr::Index {
                object,
                index,
                ty,
                is_array,
            } => {
                let o = self.lower_expr(object)?;
                let i = self.lower_expr(index)?;
                let kind = if *is_array {
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
                Ok(self.emit(kind, *ty))
            }
            HirExpr::MethodCall {
                recv,
                name,
                args,
                ty,
            } => {
                let r = self.lower_expr(recv)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::MethodCall {
                        recv: r,
                        name: name.clone(),
                        args: avs,
                    },
                    *ty,
                ))
            }

            HirExpr::Super => Ok(self.emit(
                InstKind::GetSuper {
                    name: Rc::from("super"),
                },
                HirType::Dynamic,
            )),
            HirExpr::SuperMember { name } => {
                Ok(self.emit(InstKind::GetSuper { name: name.clone() }, HirType::Dynamic))
            }

            HirExpr::SuperCall { args } => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(InstKind::SuperCall { args: avs }, HirType::Dynamic))
            }

            HirExpr::SuperMethodCall { name, args } => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::SuperMethodCall {
                        name: name.clone(),
                        args: avs,
                    },
                    HirType::Dynamic,
                ))
            }

            HirExpr::OptionalChain { object, property } => {
                let obj = self.lower_expr(object)?;
                let isnull = self.emit(InstKind::IsNull { operand: obj }, HirType::Bool);
                self.lower_branch_value(
                    isnull,
                    |_| Ok(obj),
                    |s| s.apply_optional(obj, property),
                    HirType::Dynamic,
                )
            }

            HirExpr::IntrinsicCall {
                object,
                args,
                wire_byte,
                ty,
            } => {
                let o = self.lower_expr(object)?;
                if *wire_byte == varn_core::intrinsic_ops::int::IntOp::ToString.wire()
                    && args.is_empty()
                {
                    return Ok(self.emit(InstKind::ToString { operand: o }, HirType::Str));
                }
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::IntrinsicCall {
                        object: o,
                        args: avs,
                        wire_byte: *wire_byte,
                    },
                    *ty,
                ))
            }

            HirExpr::NativeMethodCall {
                object,
                args,
                op_id,
                ty,
            } => {
                let o = self.lower_expr(object)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::CallNativeOp {
                        object: o,
                        args: avs,
                        op_id: *op_id,
                    },
                    *ty,
                ))
            }

            other => unreachable!("lower_call_expr: {other:?} is not handled here"),
        }
    }
}
