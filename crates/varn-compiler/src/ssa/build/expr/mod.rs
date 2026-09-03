use std::rc::Rc;

use crate::hir::{HirBinOp, HirExpr, HirFunction, HirType, HirTypeTest, HirUnOp, HirUpvalueSrc};
use crate::ssa::ir::{InstKind, Terminator, Value};
use crate::OptError;

use super::{Builder, Result};

mod assign;
mod calls;
mod class;
mod collections;
mod match_expr;
mod ops;

impl Builder {
    pub(super) fn lower_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Int(n) => Ok(self.emit(InstKind::ConstInt(*n), HirType::Int)),
            HirExpr::Float(n) => Ok(self.emit(InstKind::ConstFloat(*n), HirType::Float)),
            HirExpr::Bool(b) => Ok(self.emit(InstKind::ConstBool(*b), HirType::Bool)),
            HirExpr::Str(s) => Ok(self.emit(InstKind::ConstStr(s.clone()), HirType::Str)),
            HirExpr::Char(c) => Ok(self.emit(InstKind::ConstChar(*c), HirType::Int)),
            HirExpr::Null => Ok(self.emit(InstKind::ConstNull, HirType::Dynamic)),
            HirExpr::Decimal(d) => Ok(self.emit(InstKind::ConstDecimal(*d), HirType::Ref)),
            HirExpr::BigInt(n) => Ok(self.emit(InstKind::ConstBigInt(*n), HirType::Ref)),
            HirExpr::Regex { pattern, flags } => Ok(self.emit(
                InstKind::ConstStr(Rc::from(format!("/{pattern}/{flags}"))),
                HirType::Ref,
            )),

            HirExpr::NonNull(inner) => {
                let v = self.lower_expr(inner)?;
                self.emit_effect(InstKind::AssertNotNull { operand: v });
                Ok(v)
            }

            HirExpr::Sequence(exprs) => {
                let mut last = None;
                for e in exprs {
                    last = Some(self.lower_expr(e)?);
                }
                match last {
                    Some(v) => Ok(v),
                    None => Ok(self.emit(InstKind::ConstNull, HirType::Dynamic)),
                }
            }
            HirExpr::MemberMaybe { object, name, ty } => {
                let o = self.lower_expr(object)?;
                Ok(self.emit(
                    InstKind::GetPropertyMaybe {
                        object: o,
                        name: name.clone(),
                    },
                    *ty,
                ))
            }
            HirExpr::ModuleSlot { object, slot, ty } => {
                let o = self.lower_expr(object)?;
                Ok(self.emit(
                    InstKind::ModuleSlot {
                        object: o,
                        slot: *slot,
                    },
                    *ty,
                ))
            }
            HirExpr::ObjectRest { object, skip_keys } => {
                let o = self.lower_expr(object)?;
                Ok(self.emit(
                    InstKind::ObjectRest {
                        object: o,
                        skip_keys: skip_keys.clone(),
                    },
                    HirType::Ref,
                ))
            }

            HirExpr::TryOp(operand) => {
                let v = self.lower_expr(operand)?;
                let tag = self.emit(InstKind::GetEnumTag { operand: v }, HirType::Int);
                let entry = self.current;
                let ok_b = self.new_block();
                let err_b = self.new_block();
                self.set_term(Terminator::Branch {
                    cond: tag,
                    then_blk: ok_b,
                    then_args: Vec::new(),
                    else_blk: err_b,
                    else_args: Vec::new(),
                });
                self.add_pred(ok_b, entry);
                self.add_pred(err_b, entry);
                self.seal_block(ok_b);
                self.seal_block(err_b);
                self.current = err_b;
                self.set_term(Terminator::Return(Some(v)));
                self.current = ok_b;
                Ok(v)
            }

            HirExpr::Cast { expr, ty } => {
                let v = self.lower_expr(expr)?;
                if self.values[v.0 as usize].ty == *ty {
                    Ok(v)
                } else {
                    Ok(self.emit(
                        InstKind::Cast {
                            operand: v,
                            ty: *ty,
                        },
                        *ty,
                    ))
                }
            }

            HirExpr::TypeTest { value, kind } => {
                let v = self.lower_expr(value)?;
                match kind {
                    HirTypeTest::IsNull => {
                        Ok(self.emit(InstKind::IsNull { operand: v }, HirType::Bool))
                    }
                    HirTypeTest::IsArray => {
                        Ok(self.emit(InstKind::IsArray { operand: v }, HirType::Bool))
                    }
                    HirTypeTest::TypeofEq(name) => {
                        let t = self.emit(
                            InstKind::Unary {
                                op: HirUnOp::Typeof,
                                operand: v,
                                ty: HirType::Str,
                            },
                            HirType::Str,
                        );
                        let s = self.emit(InstKind::ConstStr(name.clone()), HirType::Str);
                        Ok(self.emit(
                            InstKind::Binary {
                                op: HirBinOp::Eq,
                                lhs: t,
                                rhs: s,
                                ty: HirType::Dynamic,
                            },
                            HirType::Bool,
                        ))
                    }
                    HirTypeTest::Instanceof(name) => {
                        let cls = self.emit(InstKind::LoadGlobal(name.clone()), HirType::Ref);
                        Ok(self.emit(
                            InstKind::Binary {
                                op: HirBinOp::Instanceof,
                                lhs: v,
                                rhs: cls,
                                ty: HirType::Dynamic,
                            },
                            HirType::Bool,
                        ))
                    }
                    HirTypeTest::AlwaysFalse => {
                        Ok(self.emit(InstKind::ConstBool(false), HirType::Bool))
                    }
                }
            }
            HirExpr::This => Ok(self.emit(InstKind::This, HirType::Ref)),
            HirExpr::Range {
                start,
                end,
                inclusive,
            } => {
                let s = self.lower_expr(start)?;
                let e = self.lower_expr(end)?;
                Ok(self.emit(
                    InstKind::Range {
                        start: s,
                        end: e,
                        inclusive: *inclusive,
                    },
                    HirType::Ref,
                ))
            }
            HirExpr::Var(binding) => self.load_binding(binding),
            HirExpr::Call { .. }
            | HirExpr::ExtensionCall { .. }
            | HirExpr::SelfCall { .. }
            | HirExpr::Member { .. }
            | HirExpr::GetFixedField { .. }
            | HirExpr::Index { .. }
            | HirExpr::MethodCall { .. }
            | HirExpr::Super
            | HirExpr::SuperMember { .. }
            | HirExpr::SuperCall { .. }
            | HirExpr::SuperMethodCall { .. }
            | HirExpr::OptionalChain { .. }
            | HirExpr::IntrinsicCall { .. }
            | HirExpr::NativeMethodCall { .. } => self.lower_call_expr(expr),

            HirExpr::Array(_)
            | HirExpr::Tuple(_)
            | HirExpr::Object { .. }
            | HirExpr::Record { .. } => self.lower_collection_expr(expr),

            HirExpr::Logical { .. }
            | HirExpr::Template(_)
            | HirExpr::Conditional { .. }
            | HirExpr::Binary { .. }
            | HirExpr::Unary { .. } => self.lower_operator_expr(expr),

            HirExpr::Assign { .. } | HirExpr::Update { .. } => self.lower_assign_expr(expr),

            HirExpr::Closure { func, upvalues } => self.lower_closure(func, upvalues),
            HirExpr::Match { subject, cases } => self.lower_match(subject, cases),
            HirExpr::Class(cls) => self.lower_class(cls),
            HirExpr::Enum(en) => self.lower_enum(en),
            HirExpr::Await(e, ty) => {
                let val = self.lower_expr(e)?;
                Ok(self.emit(InstKind::Await { operand: val }, *ty))
            }
            HirExpr::Spawn(e) => {
                let val = self.lower_expr(e)?;
                Ok(self.emit(InstKind::Spawn { operand: val }, HirType::Dynamic))
            }
            HirExpr::Yield(e) => {
                let val = self.lower_expr(e)?;
                Ok(self.emit(InstKind::Yield { operand: val }, HirType::Dynamic))
            }
            _ => Err(OptError::Unsupported("ssa: expression kind")),
        }
    }

    /// No carga los valores capturados: el descriptor de `MakeClosure`
    /// referencia el slot del frame padre, así que materializarlos sólo
    /// producía cargas muertas (una por captura y closure).
    fn lower_closure(&mut self, func: &HirFunction, upvalues: &[HirUpvalueSrc]) -> Result<Value> {
        Ok(self.emit(
            InstKind::MakeClosure {
                func: Rc::new(func.clone()),
                upvalues_src: upvalues.to_vec(),
            },
            HirType::Ref,
        ))
    }
}
