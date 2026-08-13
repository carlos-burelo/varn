use std::rc::Rc;

use crate::hir::{HirBinOp, HirCaseTest, HirExpr, HirMatchCase, HirOptionalProperty, HirType};
use crate::ssa::ir::{InstKind, Terminator, Value};

use super::super::VarId;
use super::{Builder, Result};


impl Builder {
    pub(super) fn apply_optional(&mut self, obj: Value, property: &HirOptionalProperty) -> Result<Value> {
        match property {
            HirOptionalProperty::Member(name) => Ok(self.emit(
                InstKind::GetPropertyMaybe {
                    object: obj,
                    name: name.clone(),
                },
                HirType::Dynamic,
            )),
            HirOptionalProperty::Index(index) => {
                let i = self.lower_expr(index)?;
                Ok(self.emit(
                    InstKind::GetIndex {
                        object: obj,
                        index: i,
                    },
                    HirType::Dynamic,
                ))
            }
            HirOptionalProperty::ModuleSlot(slot) => Ok(self.emit(
                InstKind::ModuleSlot {
                    object: obj,
                    slot: *slot,
                },
                HirType::Dynamic,
            )),
            HirOptionalProperty::Call(args) => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::Call {
                        callee: obj,
                        args: avs,
                    },
                    HirType::Dynamic,
                ))
            }
            HirOptionalProperty::MethodCall(name, args) => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::MethodCall {
                        recv: obj,
                        name: name.clone(),
                        args: avs,
                    },
                    HirType::Dynamic,
                ))
            }
            HirOptionalProperty::Extension(func) => Ok(self.emit(
                InstKind::ExtensionCall {
                    func: func.clone(),
                    recv: obj,
                    args: Vec::new(),
                },
                HirType::Dynamic,
            )),
            HirOptionalProperty::ExtensionCall(func, args) => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::ExtensionCall {
                        func: func.clone(),
                        recv: obj,
                        args: avs,
                    },
                    HirType::Dynamic,
                ))
            }
        }
    }

    pub(super) fn lower_match(&mut self, subject: &HirExpr, cases: &[HirMatchCase]) -> Result<Value> {
        let subj = self.lower_expr(subject)?;
        let merge = self.new_block();
        let mut chain_alive = true;
        for case in cases {
            if !chain_alive {
                break;
            }
            let test_blk = self.current;
            let has_guard = case.guard.is_some();

            let cond = match &case.test {
                HirCaseTest::Wildcard | HirCaseTest::Bind(_) | HirCaseTest::Record { .. } => None,
                HirCaseTest::Literal(lit) => {
                    let litv = self.lower_expr(lit)?;
                    Some(self.emit(
                        InstKind::Binary {
                            op: HirBinOp::Eq,
                            lhs: subj,
                            rhs: litv,
                            ty: HirType::Dynamic,
                        },
                        HirType::Bool,
                    ))
                }
                HirCaseTest::EnumVariant { name, .. } => {
                    let tag = self.emit(
                        InstKind::GetProperty {
                            object: subj,
                            name: Rc::from("__variant_name__"),
                        },
                        HirType::Str,
                    );
                    let namev = self.emit(InstKind::ConstStr(name.clone()), HirType::Str);
                    Some(self.emit(
                        InstKind::Binary {
                            op: HirBinOp::Eq,
                            lhs: tag,
                            rhs: namev,
                            ty: HirType::Dynamic,
                        },
                        HirType::Bool,
                    ))
                }
            };

            let is_unconditional = cond.is_none() && !has_guard;
            let body_b = self.new_block();
            let mut fail_blk_opt = None;

            if is_unconditional {
                self.set_term(Terminator::Jump {
                    target: body_b,
                    args: Vec::new(),
                });
                self.add_pred(body_b, test_blk);
                self.seal_block(body_b);
                self.current = body_b;
                self.bind_pattern(&case.test, subj);
            } else {
                let fail_blk = self.new_block();
                fail_blk_opt = Some(fail_blk);
                if has_guard {
                    if let Some(c) = cond {
                        let guard_b = self.new_block();
                        self.set_term(Terminator::Branch {
                            cond: c,
                            then_blk: guard_b,
                            then_args: Vec::new(),
                            else_blk: fail_blk,
                            else_args: Vec::new(),
                        });
                        self.add_pred(guard_b, test_blk);
                        self.add_pred(fail_blk, test_blk);
                        self.seal_block(guard_b);

                        self.current = guard_b;
                        self.bind_pattern(&case.test, subj);
                        let g_val = self.lower_expr(case.guard.as_ref().unwrap())?;
                        self.set_term(Terminator::Branch {
                            cond: g_val,
                            then_blk: body_b,
                            then_args: Vec::new(),
                            else_blk: fail_blk,
                            else_args: Vec::new(),
                        });
                        self.add_pred(body_b, guard_b);
                        self.add_pred(fail_blk, guard_b);
                        self.seal_block(body_b);
                        self.seal_block(fail_blk);
                    } else {
                        self.bind_pattern(&case.test, subj);
                        let g_val = self.lower_expr(case.guard.as_ref().unwrap())?;
                        self.set_term(Terminator::Branch {
                            cond: g_val,
                            then_blk: body_b,
                            then_args: Vec::new(),
                            else_blk: fail_blk,
                            else_args: Vec::new(),
                        });
                        self.add_pred(body_b, test_blk);
                        self.add_pred(fail_blk, test_blk);
                        self.seal_block(body_b);
                        self.seal_block(fail_blk);
                    }
                } else {
                    let c = cond.unwrap();
                    self.set_term(Terminator::Branch {
                        cond: c,
                        then_blk: body_b,
                        then_args: Vec::new(),
                        else_blk: fail_blk,
                        else_args: Vec::new(),
                    });
                    self.add_pred(body_b, test_blk);
                    self.add_pred(fail_blk, test_blk);
                    self.seal_block(body_b);
                    self.seal_block(fail_blk);

                    self.current = body_b;
                    self.bind_pattern(&case.test, subj);
                }

                self.current = body_b;
            }

            self.lower_block(&case.body)?;
            let rv = match &case.result {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Dynamic),
            };
            if self.is_open() {
                let from = self.current;
                self.set_term(Terminator::Jump {
                    target: merge,
                    args: vec![rv],
                });
                self.add_pred(merge, from);
            }

            if is_unconditional {
                chain_alive = false;
            } else {
                self.current = fail_blk_opt.unwrap();
            }
        }
        if chain_alive {
            let from = self.current;
            let nullv = self.emit(InstKind::ConstNull, HirType::Dynamic);
            self.set_term(Terminator::Jump {
                target: merge,
                args: vec![nullv],
            });
            self.add_pred(merge, from);
        }
        self.seal_block(merge);
        self.current = merge;
        Ok(self.add_block_param(merge, HirType::Dynamic))
    }

    fn bind_pattern(&mut self, test: &HirCaseTest, subj: Value) {
        match test {
            HirCaseTest::Bind(local) => {
                self.write_var(VarId::Local(*local), self.current, subj);
            }
            HirCaseTest::EnumVariant { binds, .. } => {
                for (i, b) in binds.iter().enumerate() {
                    if let Some(local) = b {
                        let pv = self.emit(
                            InstKind::GetProperty {
                                object: subj,
                                name: Rc::from(format!("value{i}")),
                            },
                            HirType::Dynamic,
                        );
                        self.write_var(VarId::Local(*local), self.current, pv);
                    }
                }
            }
            HirCaseTest::Record { fields } => {
                for (name, local_opt) in fields {
                    if let Some(local) = local_opt {
                        let pv = self.emit(
                            InstKind::GetProperty {
                                object: subj,
                                name: name.clone(),
                            },
                            HirType::Dynamic,
                        );
                        self.write_var(VarId::Local(*local), self.current, pv);
                    }
                }
            }
            _ => {}
        }
    }

}
