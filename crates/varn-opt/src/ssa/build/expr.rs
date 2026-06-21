//! HIR -> SSA: expression lowering (`lower_expr`).

use std::rc::Rc;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinOp, HirBinding, HirExpr, HirLogicalOp, HirObjectProp,
    HirPropKey, HirTemplatePart, HirType, HirTypeTest, HirUnOp, HirUpdateOp,
};
use crate::ssa::ir::{InstKind, Terminator, Value};
use crate::OptError;

use super::{binding_var, Builder, Result};

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
            // `expr!` — assert non-null (side effect), value passes through.
            HirExpr::NonNull(inner) => {
                let v = self.lower_expr(inner)?;
                self.emit_effect(InstKind::AssertNotNull { operand: v });
                Ok(v)
            }
            // Comma sequence: evaluate all, yield the last.
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
                    InstKind::GetPropertyMaybe { object: o, name: name.clone() },
                    *ty,
                ))
            }
            HirExpr::ModuleSlot { object, slot, ty } => {
                let o = self.lower_expr(object)?;
                Ok(self.emit(InstKind::ModuleSlot { object: o, slot: *slot }, *ty))
            }
            // `expr?` try operator: if the operand's enum tag is falsy (Err/None),
            // early-return it; else continue with it. A branch with a return arm.
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
            // `expr is Type` runtime test → a concrete check yielding a bool.
            HirExpr::TypeTest { value, kind } => {
                let v = self.lower_expr(value)?;
                match kind {
                    HirTypeTest::IsNull => Ok(self.emit(InstKind::IsNull { operand: v }, HirType::Bool)),
                    HirTypeTest::IsArray => Ok(self.emit(InstKind::IsArray { operand: v }, HirType::Bool)),
                    HirTypeTest::TypeofEq(name) => {
                        let t = self.emit(
                            InstKind::Unary { op: HirUnOp::Typeof, operand: v, ty: HirType::Str },
                            HirType::Str,
                        );
                        let s = self.emit(InstKind::ConstStr(name.clone()), HirType::Str);
                        Ok(self.emit(
                            InstKind::Binary { op: HirBinOp::Eq, lhs: t, rhs: s, ty: HirType::Dynamic },
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
                    HirTypeTest::AlwaysFalse => Ok(self.emit(InstKind::ConstBool(false), HirType::Bool)),
                }
            }
            HirExpr::This => Ok(self.emit(InstKind::This, HirType::Ref)),
            HirExpr::Range { start, end, inclusive } => {
                let s = self.lower_expr(start)?;
                let e = self.lower_expr(end)?;
                Ok(self.emit(
                    InstKind::Range { start: s, end: e, inclusive: *inclusive },
                    HirType::Ref,
                ))
            }
            HirExpr::Var(HirBinding::Global(name)) => {
                Ok(self.emit(InstKind::LoadGlobal(name.clone()), HirType::Dynamic))
            }
            HirExpr::Var(binding) => {
                let var = binding_var(binding)?;
                self.read_var(var, self.current)
            }
            HirExpr::Call { callee, args, ty } => {
                let c = self.lower_expr(callee)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(InstKind::Call { callee: c, args: avs }, *ty))
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
                    InstKind::GetProperty { object: o, name: name.clone() },
                    *ty,
                ))
            }
            HirExpr::Index { object, index, ty } => {
                let o = self.lower_expr(object)?;
                let i = self.lower_expr(index)?;
                Ok(self.emit(InstKind::GetIndex { object: o, index: i }, *ty))
            }
            HirExpr::MethodCall { recv, name, args, ty } => {
                let r = self.lower_expr(recv)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::MethodCall { recv: r, name: name.clone(), args: avs },
                    *ty,
                ))
            }
            // Short-circuiting `&&`/`||`/`??` → branch + result phi.
            HirExpr::Logical { op, lhs, rhs } => {
                let l = self.lower_expr(lhs)?;
                match op {
                    // `a && b`: a truthy → b, else a.
                    HirLogicalOp::And => {
                        self.lower_branch_value(l, |s| s.lower_expr(rhs), |_| Ok(l), HirType::Dynamic)
                    }
                    // `a || b`: a truthy → a, else b.
                    HirLogicalOp::Or => {
                        self.lower_branch_value(l, |_| Ok(l), |s| s.lower_expr(rhs), HirType::Dynamic)
                    }
                    // `a ?? b`: a null → b, else a.
                    HirLogicalOp::Nullish => {
                        let isnull = self.emit(InstKind::IsNull { operand: l }, HirType::Bool);
                        self.lower_branch_value(isnull, |s| s.lower_expr(rhs), |_| Ok(l), HirType::Dynamic)
                    }
                }
            }
            // `[a, b, …]` array literal (no spread/holes) → `BuildArray`.
            HirExpr::Array(els) => {
                let mut vals = Vec::with_capacity(els.len());
                for el in els {
                    match el {
                        HirArrayEl::Expr(e) => vals.push(self.lower_expr(e)?),
                        _ => return Err(OptError::Unsupported("ssa: array spread/hole")),
                    }
                }
                Ok(self.emit(InstKind::BuildArray { elements: vals }, HirType::Ref))
            }
            // `{ k: v, … }` object literal (static keys, value props) → `BuildObject`.
            HirExpr::Object { properties } => {
                let mut pairs = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        HirObjectProp::Property { key: HirPropKey::Static(k), value } => {
                            let v = self.lower_expr(value)?;
                            pairs.push((k.clone(), v));
                        }
                        _ => {
                            return Err(OptError::Unsupported(
                                "ssa: object computed/method/spread",
                            ))
                        }
                    }
                }
                Ok(self.emit(InstKind::BuildObject { pairs }, HirType::Ref))
            }
            // Template literal `` `a${x}b` `` → `BuildStr` over stringified parts.
            HirExpr::Template(parts) => {
                let mut pvals = Vec::with_capacity(parts.len());
                for part in parts {
                    match part {
                        HirTemplatePart::Str(s) => {
                            pvals.push(self.emit(InstKind::ConstStr(s.clone()), HirType::Str))
                        }
                        HirTemplatePart::Expr(e) => {
                            let v = self.lower_expr(e)?;
                            pvals.push(self.emit(InstKind::ToString { operand: v }, HirType::Str));
                        }
                    }
                }
                if pvals.is_empty() {
                    Ok(self.emit(InstKind::ConstStr(Rc::from("")), HirType::Str))
                } else {
                    Ok(self.emit(InstKind::BuildStr { parts: pvals }, HirType::Str))
                }
            }
            // Capture-free closure/arrow/nested fn → `LoadStaticFn`. Closures
            // that capture upvalues are deferred (SSA renaming vs. slot capture).
            HirExpr::Closure { func, upvalues } => {
                if upvalues.is_empty() {
                    Ok(self.emit(
                        InstKind::MakeClosure { func: Rc::new((**func).clone()) },
                        HirType::Ref,
                    ))
                } else {
                    Err(OptError::Unsupported("ssa: closure with upvalues"))
                }
            }
            // VM intrinsic `obj.fn(args)` (`Math.*`, etc.) → `Intrinsic` opcode.
            HirExpr::IntrinsicCall { object, args, wire_byte, ty } => {
                let o = self.lower_expr(object)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::IntrinsicCall { object: o, args: avs, wire_byte: *wire_byte },
                    *ty,
                ))
            }
            // Ternary `test ? cons : alt` → branch + result phi.
            HirExpr::Conditional { test, cons, alt } => {
                let t = self.lower_expr(test)?;
                self.lower_branch_value(
                    t,
                    |s| s.lower_expr(cons),
                    |s| s.lower_expr(alt),
                    HirType::Dynamic,
                )
            }
            HirExpr::Binary { op, lhs, rhs, ty } => {
                let l = self.lower_expr(lhs)?;
                let r = self.lower_expr(rhs)?;
                Ok(self.emit(
                    InstKind::Binary {
                        op: *op,
                        lhs: l,
                        rhs: r,
                        ty: *ty,
                    },
                    *ty,
                ))
            }
            HirExpr::Unary { op, operand, ty } => {
                let o = self.lower_expr(operand)?;
                Ok(self.emit(
                    InstKind::Unary {
                        op: *op,
                        operand: o,
                        ty: *ty,
                    },
                    *ty,
                ))
            }
            // Assignment-as-expression on a scalar binding (e.g. a `for` update
            // clause `i = i + 1`): write the new SSA value and yield it.
            HirExpr::Assign { target, value } => match &**target {
                HirAssignTarget::Var(binding) => {
                    let var = binding_var(binding)?;
                    let v = self.lower_expr(value)?;
                    self.write_var(var, self.current, v);
                    Ok(v)
                }
                HirAssignTarget::Member { object, name } => {
                    let o = self.lower_expr(object)?;
                    let v = self.lower_expr(value)?;
                    self.emit_effect(InstKind::SetProperty { object: o, name: name.clone(), value: v });
                    Ok(v)
                }
                HirAssignTarget::Index { object, index } => {
                    let o = self.lower_expr(object)?;
                    let i = self.lower_expr(index)?;
                    let v = self.lower_expr(value)?;
                    self.emit_effect(InstKind::SetIndex { object: o, index: i, value: v });
                    Ok(v)
                }
                _ => Err(OptError::Unsupported("ssa: assign target")),
            },
            // `++`/`--` on a scalar binding (prefix yields the new value, postfix
            // the old).
            HirExpr::Update { target, op, prefix } => match &**target {
                HirAssignTarget::Var(binding) => {
                    let var = binding_var(binding)?;
                    let old = self.read_var(var, self.current)?;
                    let ty = self.values[old.0 as usize].ty;
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary { op: bop, lhs: old, rhs: one, ty },
                        ty,
                    );
                    self.write_var(var, self.current, new);
                    Ok(if *prefix { new } else { old })
                }
                _ => Err(OptError::Unsupported("ssa: update target")),
            },
            _ => Err(OptError::Unsupported("ssa: expression kind")),
        }
    }
}
