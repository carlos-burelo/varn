//! HIR -> SSA: expression lowering (`lower_expr`).

use std::rc::Rc;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinOp, HirCaseTest, HirClass, HirEnum, HirExpr, HirFunction,
    HirLogicalOp, HirMatchCase, HirObjectProp, HirOptionalProperty, HirPropKey, HirTemplatePart,
    HirType, HirTypeTest, HirUnOp, HirUpvalueSrc, HirUpdateOp,
};
use crate::ssa::ir::{BlockId, InstKind, Terminator, Value};
use crate::OptError;

use super::{Builder, Result, VarId};

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
            HirExpr::Var(binding) => self.load_binding(binding),
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
                    return Ok(self.emit(InstKind::CallSpread { callee: c, args: avs }, *ty));
                }
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(InstKind::Call { callee: c, args: avs }, *ty))
            }
            // Extension call `recv.m(args)` → mangled global with `recv` as receiver.
            HirExpr::ExtensionCall { func, recv, args } => {
                let r = self.lower_expr(recv)?;
                let mut avs = Vec::with_capacity(args.len());
                for a in args {
                    avs.push(self.lower_expr(a)?);
                }
                Ok(self.emit(
                    InstKind::ExtensionCall { func: func.clone(), recv: r, args: avs },
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
            // `{ k: v, … }` object literal → `BuildObject`; with spread, an empty
            // object built up by `SetProperty` / `ObjectMerge` in order.
            HirExpr::Object { properties } => {
                if properties.iter().any(|p| matches!(p, HirObjectProp::Spread(_))) {
                    let mut parts = Vec::with_capacity(properties.len());
                    for prop in properties {
                        match prop {
                            HirObjectProp::Property { key: HirPropKey::Static(k), value } => {
                                let v = self.lower_expr(value)?;
                                parts.push((Some(k.clone()), v));
                            }
                            HirObjectProp::Spread(e) => {
                                let v = self.lower_expr(e)?;
                                parts.push((None, v));
                            }
                            _ => {
                                return Err(OptError::Unsupported("ssa: object computed/method"))
                            }
                        }
                    }
                    return Ok(self.emit(InstKind::BuildObjectSpread { parts }, HirType::Ref));
                }
                let mut pairs = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        HirObjectProp::Property { key: HirPropKey::Static(k), value } => {
                            let v = self.lower_expr(value)?;
                            pairs.push((k.clone(), v));
                        }
                        _ => {
                            return Err(OptError::Unsupported("ssa: object computed/method"))
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
            HirExpr::Closure { func, upvalues } => {
                self.lower_closure(func, upvalues)
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
                    let v = self.lower_expr(value)?;
                    self.store_binding(binding, v);
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
                    let old = self.load_binding(binding)?;
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
                    self.store_binding(binding, new);
                    Ok(if *prefix { new } else { old })
                }
                // `o.x++` / `++o.x`: read member, ±1, write back.
                HirAssignTarget::Member { object, name } => {
                    let o = self.lower_expr(object)?;
                    let old = self.emit(
                        InstKind::GetProperty { object: o, name: name.clone() },
                        HirType::Dynamic,
                    );
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary { op: bop, lhs: old, rhs: one, ty: HirType::Dynamic },
                        HirType::Dynamic,
                    );
                    self.emit_effect(InstKind::SetProperty {
                        object: o,
                        name: name.clone(),
                        value: new,
                    });
                    Ok(if *prefix { new } else { old })
                }
                // `a[i]++` / `++a[i]`: read element, ±1, write back.
                HirAssignTarget::Index { object, index } => {
                    let o = self.lower_expr(object)?;
                    let i = self.lower_expr(index)?;
                    let old = self.emit(InstKind::GetIndex { object: o, index: i }, HirType::Dynamic);
                    let one = self.emit(InstKind::ConstInt(1), HirType::Int);
                    let bop = match op {
                        HirUpdateOp::Inc => HirBinOp::Add,
                        HirUpdateOp::Dec => HirBinOp::Sub,
                    };
                    let new = self.emit(
                        InstKind::Binary { op: bop, lhs: old, rhs: one, ty: HirType::Dynamic },
                        HirType::Dynamic,
                    );
                    self.emit_effect(InstKind::SetIndex { object: o, index: i, value: new });
                    Ok(if *prefix { new } else { old })
                }
                _ => Err(OptError::Unsupported("ssa: update target")),
            },
            HirExpr::Match { subject, cases } => self.lower_match(subject, cases),
            HirExpr::Class(cls) => self.lower_class(cls),
            HirExpr::Enum(en) => self.lower_enum(en),
            HirExpr::Await(e) => {
                let val = self.lower_expr(e)?;
                Ok(self.emit(InstKind::Await { operand: val }, HirType::Dynamic))
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

    fn lower_closure(&mut self, func: &HirFunction, upvalues: &[HirUpvalueSrc]) -> Result<Value> {
        let mut uvs = Vec::with_capacity(upvalues.len());
        for uv in upvalues {
            let val = match uv {
                HirUpvalueSrc::ParentLocal(id) => self.read_var(VarId::Local(*id), self.current)?,
                HirUpvalueSrc::ParentParam(i) => self.read_var(VarId::Param(*i), self.current)?,
                HirUpvalueSrc::ParentUpvalue(uv) => {
                    self.emit(InstKind::LoadUpvalue(*uv), HirType::Dynamic)
                }
            };
            uvs.push(val);
        }
        Ok(self.emit(
            InstKind::MakeClosure {
                func: Rc::new(func.clone()),
                upvalues: uvs,
                upvalues_src: upvalues.to_vec(),
            },
            HirType::Ref,
        ))
    }

    fn lower_class(&mut self, cls: &HirClass) -> Result<Value> {
        let name_idx = cls.name.clone();
        let super_class = match &cls.super_class {
            Some(sup) => Some(self.lower_expr(sup)?),
            None => None,
        };
        let mut class_val = self.emit(InstKind::MakeClass { name: name_idx, super_class }, HirType::Ref);
        
        for field in &cls.fields {
            self.emit_effect(InstKind::DeclareField { class: class_val, name: field.clone() });
        }
        
        for (key, init) in &cls.static_fields {
            let val = match init {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Dynamic),
            };
            self.emit_effect(InstKind::DefineStatic { class: class_val, name: key.clone(), value: val });
        }
        
        self.bind_method(class_val, &cls.ctor, false)?;
        for m in &cls.methods {
            self.bind_method(class_val, m, false)?;
        }
        for m in &cls.static_methods {
            self.bind_method(class_val, m, true)?;
        }
        for a in &cls.getters {
            self.bind_member(class_val, a.key.clone(), &a.func, &a.upvalues, true, a.is_static)?;
        }
        for a in &cls.setters {
            self.bind_member(class_val, a.key.clone(), &a.func, &a.upvalues, false, a.is_static)?;
        }
        
        for b in &cls.static_blocks {
            let fn_val = self.lower_closure(&b.func, &b.upvalues)?;
            self.emit(InstKind::Call { callee: fn_val, args: Vec::new() }, HirType::Dynamic);
        }
        
        if !cls.decorators.is_empty() {
            class_val = self.apply_class_decorators(class_val, &cls.decorators)?;
        }
        
        Ok(class_val)
    }

    fn lower_enum(&mut self, en: &HirEnum) -> Result<Value> {
        let name_idx = en.name.clone();
        let class_val = self.emit(InstKind::MakeClass { name: name_idx, super_class: None }, HirType::Ref);
        
        for v in &en.variants {
            let variant_val = self.emit(InstKind::MakeEnumVariant { tag: v.tag, meta: v.meta.clone() }, HirType::Ref);
            self.emit_effect(InstKind::DefineStatic { class: class_val, name: v.name.clone(), value: variant_val });
        }
        
        for field in &en.fields {
            self.emit_effect(InstKind::DeclareField { class: class_val, name: field.clone() });
        }
        
        for (key, init) in &en.static_fields {
            let val = match init {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Dynamic),
            };
            self.emit_effect(InstKind::DefineStatic { class: class_val, name: key.clone(), value: val });
        }
        
        self.bind_method(class_val, &en.ctor, false)?;
        for m in &en.methods {
            self.bind_method(class_val, m, false)?;
        }
        for m in &en.static_methods {
            self.bind_method(class_val, m, true)?;
        }
        for a in &en.getters {
            self.bind_member(class_val, a.key.clone(), &a.func, &a.upvalues, true, a.is_static)?;
        }
        for a in &en.setters {
            self.bind_member(class_val, a.key.clone(), &a.func, &a.upvalues, false, a.is_static)?;
        }
        
        for v in &en.variants {
            if !v.const_args.is_empty() {
                let receiver = self.emit(InstKind::GetProperty { object: class_val, name: v.name.clone() }, HirType::Ref);
                let ctor = self.emit(InstKind::GetProperty { object: receiver, name: Rc::from("constructor") }, HirType::Ref);
                let mut args = vec![receiver];
                for arg in &v.const_args {
                    args.push(self.lower_expr(arg)?);
                }
                self.emit(InstKind::Call { callee: ctor, args }, HirType::Dynamic);
            }
        }
        
        if !en.static_blocks.is_empty() {
            self.emit_effect(InstKind::StoreGlobal { name: en.name.clone(), value: class_val });
        }
        
        for b in &en.static_blocks {
            let fn_val = self.lower_closure(&b.func, &b.upvalues)?;
            self.emit(InstKind::Call { callee: fn_val, args: Vec::new() }, HirType::Dynamic);
        }
        
        Ok(class_val)
    }

    fn bind_method(&mut self, class_val: Value, m: &crate::hir::HirMethod, is_static: bool) -> Result<()> {
        let mut reg = self.lower_closure(&m.func, &m.upvalues)?;
        if !m.decorators.is_empty() {
            reg = self.apply_method_decorators(reg, &m.key, is_static, m.is_private, &m.decorators)?;
        }
        self.emit_effect(InstKind::DefineMethod {
            class: class_val,
            name: m.key.clone(),
            method: reg,
            is_static,
        });
        Ok(())
    }

    fn bind_member(
        &mut self,
        class_val: Value,
        name: Rc<str>,
        func: &HirFunction,
        upvalues: &[HirUpvalueSrc],
        is_getter: bool,
        is_static: bool,
    ) -> Result<()> {
        let reg = self.lower_closure(func, upvalues)?;
        self.emit_effect(InstKind::DefineAccessor {
            class: class_val,
            name,
            accessor: reg,
            is_getter,
            is_static,
        });
        Ok(())
    }

    fn apply_method_decorators(
        &mut self,
        method_val: Value,
        key: &str,
        is_static: bool,
        is_private: bool,
        decorators: &[HirExpr],
    ) -> Result<Value> {
        let mut current_method = method_val;
        for deco in decorators.iter().rev() {
            let deco_fn = self.lower_expr(deco)?;
            let n_r = self.emit(InstKind::ConstStr(Rc::from(key)), HirType::Str);
            let kind_r = self.emit(InstKind::ConstStr(Rc::from("method")), HirType::Str);
            let static_r = self.emit(InstKind::ConstBool(is_static), HirType::Bool);
            let private_r = self.emit(InstKind::ConstBool(is_private), HirType::Bool);
            
            let pairs = vec![
                (Rc::from("name"), n_r),
                (Rc::from("kind"), kind_r),
                (Rc::from("isStatic"), static_r),
                (Rc::from("isPrivate"), private_r),
            ];
            let ctx_obj = self.emit(InstKind::BuildObject { pairs }, HirType::Ref);
            let args = vec![current_method, ctx_obj];
            let result = self.emit(InstKind::Call { callee: deco_fn, args }, HirType::Dynamic);
            let isnull = self.emit(InstKind::IsNull { operand: result }, HirType::Bool);
            current_method = self.lower_branch_value(
                isnull,
                |_| Ok(current_method),
                |_| Ok(result),
                HirType::Ref,
            )?;
        }
        Ok(current_method)
    }

    fn apply_class_decorators(&mut self, class_val: Value, decorators: &[HirExpr]) -> Result<Value> {
        let mut current_class = class_val;
        for deco in decorators.iter().rev() {
            let deco_fn = self.lower_expr(deco)?;
            let args = vec![current_class];
            let result = self.emit(InstKind::Call { callee: deco_fn, args }, HirType::Dynamic);
            let isnull = self.emit(InstKind::IsNull { operand: result }, HirType::Bool);
            current_class = self.lower_branch_value(
                isnull,
                |_| Ok(current_class),
                |_| Ok(result),
                HirType::Ref,
            )?;
        }
        Ok(current_class)
    }
}
