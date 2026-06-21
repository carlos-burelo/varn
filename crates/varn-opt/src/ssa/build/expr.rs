//! HIR -> SSA: expression lowering (`lower_expr`).

use std::rc::Rc;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinOp, HirBinding, HirCaseTest, HirExpr, HirLogicalOp,
    HirMatchCase, HirObjectProp, HirOptionalProperty, HirPropKey, HirTemplatePart, HirType,
    HirTypeTest, HirUnOp, HirUpdateOp,
};
use crate::ssa::ir::{BlockId, InstKind, Terminator, Value};
use crate::OptError;

use super::{binding_var, Builder, Result, VarId};

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
            // `super` / `super.name` member read.
            HirExpr::Super => Ok(self.emit(InstKind::GetSuper { name: Rc::from("super") }, HirType::Dynamic)),
            HirExpr::SuperMember { name } => {
                Ok(self.emit(InstKind::GetSuper { name: name.clone() }, HirType::Dynamic))
            }
            // `super(args)` — superclass constructor call.
            HirExpr::SuperCall { args } => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(InstKind::SuperCall { args: avs }, HirType::Dynamic))
            }
            // `super.name(args)` — superclass method call.
            HirExpr::SuperMethodCall { name, args } => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::SuperMethodCall { name: name.clone(), args: avs },
                    HirType::Dynamic,
                ))
            }
            // `object?.…` optional chaining → null-check branch + result phi:
            // if `object` is null the chain yields it (short-circuit), else the
            // property is applied.
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
            HirExpr::Match { subject, cases } => self.lower_match(subject, cases),
            _ => Err(OptError::Unsupported("ssa: expression kind")),
        }
    }

    /// Apply an optional-chain property to a known-non-null `obj` (the else arm
    /// of the null check). Member/index/module-slot reads and plain/method calls
    /// map to the ordinary instructions; extension calls (which pass `obj` as the
    /// receiver, not as a plain-call arg) fall back.
    fn apply_optional(&mut self, obj: Value, property: &HirOptionalProperty) -> Result<Value> {
        match property {
            HirOptionalProperty::Member(name) => Ok(self.emit(
                InstKind::GetPropertyMaybe { object: obj, name: name.clone() },
                HirType::Dynamic,
            )),
            HirOptionalProperty::Index(index) => {
                let i = self.lower_expr(index)?;
                Ok(self.emit(InstKind::GetIndex { object: obj, index: i }, HirType::Dynamic))
            }
            HirOptionalProperty::ModuleSlot(slot) => Ok(self.emit(
                InstKind::ModuleSlot { object: obj, slot: *slot },
                HirType::Dynamic,
            )),
            HirOptionalProperty::Call(args) => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(InstKind::Call { callee: obj, args: avs }, HirType::Dynamic))
            }
            HirOptionalProperty::MethodCall(name, args) => {
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::MethodCall { recv: obj, name: name.clone(), args: avs },
                    HirType::Dynamic,
                ))
            }
            HirOptionalProperty::Extension(_) | HirOptionalProperty::ExtensionCall(..) => {
                Err(OptError::Unsupported("ssa: optional-chain extension call"))
            }
        }
    }

    /// `match subject { cases }` → a chain of pattern tests; each matching arm
    /// evaluates its body + result and jumps to a merge block whose single
    /// block parameter is the match value. Supports wildcard / literal / bind /
    /// enum-variant patterns; record patterns and guards fall back.
    fn lower_match(&mut self, subject: &HirExpr, cases: &[HirMatchCase]) -> Result<Value> {
        let subj = self.lower_expr(subject)?;
        let merge = self.new_block();
        let mut chain_alive = true;
        for case in cases {
            if !chain_alive {
                break; // a prior unconditional arm caught everything
            }
            if case.guard.is_some() {
                return Err(OptError::Unsupported("ssa: match guard"));
            }
            let test_blk = self.current;
            let body_b = self.new_block();
            let next = self.emit_match_test(&case.test, subj, body_b, test_blk)?;
            self.current = body_b;
            self.bind_pattern(&case.test, subj);
            self.lower_block(&case.body)?;
            let rv = match &case.result {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Dynamic),
            };
            if self.is_open() {
                let from = self.current;
                self.set_term(Terminator::Jump { target: merge, args: vec![rv] });
                self.add_pred(merge, from);
            }
            match next {
                Some(nb) => self.current = nb,
                None => chain_alive = false,
            }
        }
        if chain_alive {
            // No arm matched → the match yields null.
            let from = self.current;
            let nullv = self.emit(InstKind::ConstNull, HirType::Dynamic);
            self.set_term(Terminator::Jump { target: merge, args: vec![nullv] });
            self.add_pred(merge, from);
        }
        self.seal_block(merge);
        self.current = merge;
        Ok(self.add_block_param(merge, HirType::Dynamic))
    }

    /// Emit a case's pattern test in `test_blk`: on a match, control reaches
    /// `body_b`; on failure it reaches the returned next-test block (or `None`
    /// for an unconditional pattern, which ends the chain). Bindings are done in
    /// `bind_pattern` once `body_b` is current.
    fn emit_match_test(
        &mut self,
        test: &HirCaseTest,
        subj: Value,
        body_b: BlockId,
        test_blk: BlockId,
    ) -> Result<Option<BlockId>> {
        match test {
            HirCaseTest::Wildcard | HirCaseTest::Bind(_) => {
                self.set_term(Terminator::Jump { target: body_b, args: Vec::new() });
                self.add_pred(body_b, test_blk);
                self.seal_block(body_b);
                Ok(None)
            }
            HirCaseTest::Literal(lit) => {
                let litv = self.lower_expr(lit)?;
                let eq = self.emit(
                    InstKind::Binary { op: HirBinOp::Eq, lhs: subj, rhs: litv, ty: HirType::Dynamic },
                    HirType::Bool,
                );
                Ok(Some(self.branch_to(eq, body_b, test_blk)))
            }
            HirCaseTest::EnumVariant { name, .. } => {
                let tag = self.emit(
                    InstKind::GetProperty { object: subj, name: Rc::from("__variant_name__") },
                    HirType::Str,
                );
                let namev = self.emit(InstKind::ConstStr(name.clone()), HirType::Str);
                let eq = self.emit(
                    InstKind::Binary { op: HirBinOp::Eq, lhs: tag, rhs: namev, ty: HirType::Dynamic },
                    HirType::Bool,
                );
                Ok(Some(self.branch_to(eq, body_b, test_blk)))
            }
            HirCaseTest::Record { .. } => Err(OptError::Unsupported("ssa: match record pattern")),
        }
    }

    /// `branch cond, body_b, <fresh next>` from `test_blk`; returns the new next
    /// block (sealed; the chain continues there).
    fn branch_to(&mut self, cond: Value, body_b: BlockId, test_blk: BlockId) -> BlockId {
        let next = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: next,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, test_blk);
        self.add_pred(next, test_blk);
        self.seal_block(body_b);
        self.seal_block(next);
        next
    }

    /// Bind a pattern's variables in the (current) body block: `Bind` copies the
    /// subject; `EnumVariant` reads each `value{i}` payload.
    fn bind_pattern(&mut self, test: &HirCaseTest, subj: Value) {
        match test {
            HirCaseTest::Bind(local) => {
                self.write_var(VarId::Local(*local), self.current, subj);
            }
            HirCaseTest::EnumVariant { binds, .. } => {
                for (i, b) in binds.iter().enumerate() {
                    if let Some(local) = b {
                        let pv = self.emit(
                            InstKind::GetProperty { object: subj, name: Rc::from(format!("value{i}")) },
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
