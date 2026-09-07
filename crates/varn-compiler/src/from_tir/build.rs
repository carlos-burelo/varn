//! `TirFunction` -> `SsaFunc` (step 3.3).
//!
//! The block / value / phi / variable machinery is a copy of `ssa/build`'s
//! `Builder` core — that one is HIR-coupled through `open_try_regions` and its
//! `expr` / `stmt` submodules and is deleted with HIR in step 3.4; this one
//! survives. Only the SSA-agnostic core is duplicated; the lowering below is
//! written fresh against the flat TIR.

//! (`emit_effect` and other core helpers are unused until the lowering below
//! grows to cover calls, field stores and closures.)
#![allow(dead_code)]

use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;

use crate::hir::{HirBinOp, HirType, HirUnOp, LocalId};
use crate::ssa::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, ValueDef, VarId};
use crate::OptError;
use varn_tir::{
    BackendTy, Resolution, TirBinOp, TirExpr, TirExprKind, TirFunction, TirModule, TirStmt, TirUnOp,
};

use super::ty::lower as lower_ty;

type Result<T> = std::result::Result<T, OptError>;

#[derive(Clone, Copy)]
struct LoopCtx {
    continue_target: BlockId,
    break_target: BlockId,
}

struct Builder<'m> {
    tir: &'m TirModule,
    /// SSA-side type table for the class-name / element handles that
    /// `lower_ty` re-interns into.
    ssa_types: crate::hir::TyTable,

    blocks: Vec<Block>,
    values: Vec<ValueDef>,
    sealed: Vec<bool>,
    terminated: Vec<bool>,
    defs: FxHashMap<(VarId, BlockId), Value>,
    var_ty: FxHashMap<VarId, HirType>,
    incomplete_phis: FxHashMap<BlockId, Vec<(VarId, Value)>>,
    loops: Vec<LoopCtx>,
    next_synthetic: u32,
    current: BlockId,
}

impl<'m> Builder<'m> {
    fn new(tir: &'m TirModule) -> Self {
        let mut b = Builder {
            tir,
            ssa_types: crate::hir::TyTable::default(),
            blocks: Vec::new(),
            values: Vec::new(),
            sealed: Vec::new(),
            terminated: Vec::new(),
            defs: FxHashMap::default(),
            var_ty: FxHashMap::default(),
            incomplete_phis: FxHashMap::default(),
            loops: Vec::new(),
            next_synthetic: 0,
            current: BlockId(0),
        };
        let entry = b.new_block();
        b.sealed[entry.0 as usize] = true;
        b.current = entry;
        b
    }

    fn ty(&mut self, bt: BackendTy) -> HirType {
        lower_ty(bt, self.tir, &mut self.ssa_types)
    }

    // ---- SSA-agnostic core (copy of ssa/build::Builder) -------------------

    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            term: Terminator::Unreachable,
            term_line: 0,
            preds: Vec::new(),
        });
        self.sealed.push(false);
        self.terminated.push(false);
        id
    }

    fn new_value(&mut self, ty: HirType) -> Value {
        let v = Value(self.values.len() as u32);
        self.values.push(ValueDef { ty });
        v
    }

    fn value_ty(&self, v: Value) -> HirType {
        self.values[v.0 as usize].ty
    }

    fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }

    fn is_open(&self) -> bool {
        !self.terminated[self.current.0 as usize]
    }

    fn set_term(&mut self, term: Terminator) {
        self.terminated[self.current.0 as usize] = true;
        self.block_mut(self.current).term = term;
    }

    fn add_pred(&mut self, block: BlockId, pred: BlockId) {
        self.block_mut(block).preds.push(pred);
    }

    fn emit(&mut self, kind: InstKind, ty: HirType) -> Value {
        let dest = self.new_value(ty);
        self.block_mut(self.current).insts.push(Inst { dest: Some(dest), kind, line: 0 });
        dest
    }

    fn emit_effect(&mut self, kind: InstKind) {
        self.block_mut(self.current).insts.push(Inst { dest: None, kind, line: 0 });
    }

    fn write_var(&mut self, var: VarId, block: BlockId, value: Value) {
        self.var_ty.insert(var, self.values[value.0 as usize].ty);
        self.defs.insert((var, block), value);
    }

    fn read_var(&mut self, var: VarId, block: BlockId) -> Result<Value> {
        if let Some(v) = self.defs.get(&(var, block)) {
            return Ok(*v);
        }
        self.read_var_recursive(var, block)
    }

    fn read_var_recursive(&mut self, var: VarId, block: BlockId) -> Result<Value> {
        let ty = *self
            .var_ty
            .get(&var)
            .ok_or(OptError::Unsupported("from_tir: read of undefined variable"))?;
        if !self.sealed[block.0 as usize] {
            let phi = self.add_block_param(block, ty);
            self.incomplete_phis.entry(block).or_default().push((var, phi));
            self.write_var(var, block, phi);
            return Ok(phi);
        }
        let preds = self.blocks[block.0 as usize].preds.clone();
        let val = if preds.len() == 1 {
            self.read_var(var, preds[0])?
        } else {
            let phi = self.add_block_param(block, ty);
            self.write_var(var, block, phi);
            self.add_phi_operands(var, block, phi)?;
            phi
        };
        self.write_var(var, block, val);
        Ok(val)
    }

    fn add_block_param(&mut self, block: BlockId, ty: HirType) -> Value {
        let v = self.new_value(ty);
        self.block_mut(block).params.push(v);
        v
    }

    fn add_phi_operands(&mut self, var: VarId, block: BlockId, phi: Value) -> Result<()> {
        let pos = self.blocks[block.0 as usize]
            .params
            .iter()
            .position(|p| *p == phi)
            .expect("phi is a param of block");
        for pred in self.blocks[block.0 as usize].preds.clone() {
            let arg = self.read_var(var, pred)?;
            self.append_edge_arg(pred, block, pos, arg);
        }
        Ok(())
    }

    fn append_edge_arg(&mut self, pred: BlockId, block: BlockId, pos: usize, arg: Value) {
        match &mut self.block_mut(pred).term {
            Terminator::Jump { target, args } if *target == block => {
                debug_assert_eq!(args.len(), pos);
                args.push(arg);
            }
            Terminator::Branch { then_blk, then_args, else_blk, else_args, .. } => {
                if *then_blk == block {
                    then_args.push(arg);
                }
                if *else_blk == block {
                    else_args.push(arg);
                }
            }
            _ => panic!("predecessor {pred:?} has no edge to {block:?}"),
        }
    }

    fn seal_block(&mut self, block: BlockId) {
        if let Some(phis) = self.incomplete_phis.remove(&block) {
            for (var, phi) in phis {
                let _ = self.add_phi_operands(var, block, phi);
            }
        }
        self.sealed[block.0 as usize] = true;
    }

    // ---- TIR lowering ----------------------------------------------------

    fn lower_block(&mut self, stmts: &[TirStmt]) -> Result<()> {
        for s in stmts {
            if !self.is_open() {
                break;
            }
            self.lower_stmt(s)?;
        }
        Ok(())
    }

    fn lower_stmt(&mut self, s: &TirStmt) -> Result<()> {
        match s {
            TirStmt::Expr(e) => {
                self.lower_expr(e)?;
            }
            TirStmt::Let { local, ty, init } => {
                let value = match init {
                    Some(e) => self.lower_expr(e)?,
                    None => {
                        let t = self.ty(*ty);
                        self.emit(InstKind::ConstNull, t)
                    }
                };
                let cur = self.current;
                self.write_var(VarId::Local(LocalId(local.0)), cur, value);
            }
            TirStmt::Return(v) => {
                let val = match v {
                    Some(e) => Some(self.lower_expr(e)?),
                    None => None,
                };
                self.set_term(Terminator::Return(val));
            }
            TirStmt::Throw(e) => {
                let v = self.lower_expr(e)?;
                self.set_term(Terminator::Throw(v));
            }
            TirStmt::Break => {
                if let Some(c) = self.loops.last().copied() {
                    let from = self.current;
                    self.set_term(Terminator::Jump { target: c.break_target, args: vec![] });
                    self.add_pred(c.break_target, from);
                }
            }
            TirStmt::Continue => {
                if let Some(c) = self.loops.last().copied() {
                    let from = self.current;
                    self.set_term(Terminator::Jump { target: c.continue_target, args: vec![] });
                    self.add_pred(c.continue_target, from);
                }
            }
            TirStmt::If { cond, then_body, else_body } => {
                let c = self.lower_expr(cond)?;
                let then_blk = self.new_block();
                let else_blk = self.new_block();
                let join = self.new_block();
                let from = self.current;
                self.set_term(Terminator::Branch {
                    cond: c,
                    then_blk,
                    then_args: vec![],
                    else_blk,
                    else_args: vec![],
                });
                self.add_pred(then_blk, from);
                self.add_pred(else_blk, from);
                self.seal_block(then_blk);
                self.seal_block(else_blk);

                self.current = then_blk;
                self.lower_block(then_body)?;
                if self.is_open() {
                    let cur = self.current;
                    self.set_term(Terminator::Jump { target: join, args: vec![] });
                    self.add_pred(join, cur);
                }

                self.current = else_blk;
                self.lower_block(else_body)?;
                if self.is_open() {
                    let cur = self.current;
                    self.set_term(Terminator::Jump { target: join, args: vec![] });
                    self.add_pred(join, cur);
                }

                self.seal_block(join);
                self.current = join;
            }
            TirStmt::Loop { cond, body } => {
                let head = self.new_block();
                let body_blk = self.new_block();
                let exit = self.new_block();
                let from = self.current;
                self.set_term(Terminator::Jump { target: head, args: vec![] });
                self.add_pred(head, from);

                self.current = head;
                let c = self.lower_expr(cond)?;
                self.set_term(Terminator::Branch {
                    cond: c,
                    then_blk: body_blk,
                    then_args: vec![],
                    else_blk: exit,
                    else_args: vec![],
                });
                self.add_pred(body_blk, head);
                self.add_pred(exit, head);
                self.seal_block(body_blk);

                self.loops.push(LoopCtx { continue_target: head, break_target: exit });
                self.current = body_blk;
                self.lower_block(body)?;
                if self.is_open() {
                    let cur = self.current;
                    self.set_term(Terminator::Jump { target: head, args: vec![] });
                    self.add_pred(head, cur);
                }
                self.loops.pop();

                self.seal_block(head);
                self.seal_block(exit);
                self.current = exit;
            }
            TirStmt::Try { body, catch_local, catch_body } => {
                let try_entry = self.current;
                let landing = self.new_block();
                let exit = self.new_block();

                let try_val = self.emit(InstKind::Try { handler: landing }, HirType::Dynamic);

                self.lower_block(body)?;
                if self.is_open() {
                    self.emit_effect(InstKind::PopTry);
                    let from = self.current;
                    self.set_term(Terminator::Jump { target: exit, args: vec![] });
                    self.add_pred(exit, from);
                }

                self.add_pred(landing, try_entry);
                self.seal_block(landing);
                self.current = landing;
                let err = self.emit(InstKind::CatchParam { try_val }, HirType::Dynamic);
                let cur = self.current;
                self.write_var(VarId::Local(LocalId(catch_local.0)), cur, err);
                self.lower_block(catch_body)?;
                if self.is_open() {
                    let from = self.current;
                    self.set_term(Terminator::Jump { target: exit, args: vec![] });
                    self.add_pred(exit, from);
                }

                self.seal_block(exit);
                self.current = exit;
            }
        }
        Ok(())
    }

    fn lower_expr(&mut self, e: &TirExpr) -> Result<Value> {
        let ty = self.ty(e.ty);
        match &e.kind {
            TirExprKind::IntLit(n) => Ok(self.emit(InstKind::ConstInt(*n), ty)),
            TirExprKind::FloatLit(f) => Ok(self.emit(InstKind::ConstFloat(*f), ty)),
            TirExprKind::BoolLit(b) => Ok(self.emit(InstKind::ConstBool(*b), ty)),
            TirExprKind::StrLit(s) => Ok(self.emit(InstKind::ConstStr(s.clone()), ty)),
            TirExprKind::CharLit(c) => Ok(self.emit(InstKind::ConstChar(*c), ty)),
            TirExprKind::NullLit => Ok(self.emit(InstKind::ConstNull, ty)),

            TirExprKind::Var => self.lower_var(&e.res, ty),

            TirExprKind::Binary { op, lhs, rhs } => {
                let l = self.lower_expr(lhs)?;
                let r = self.lower_expr(rhs)?;
                Ok(self.emit(
                    InstKind::Binary { op: bin_op(*op), lhs: l, rhs: r, ty },
                    ty,
                ))
            }
            TirExprKind::Unary { op, operand } => {
                let v = self.lower_expr(operand)?;
                match op {
                    TirUnOp::IsNull => Ok(self.emit(InstKind::IsNull { operand: v }, HirType::Bool)),
                    _ => Ok(self.emit(
                        InstKind::Unary { op: un_op(*op), operand: v, ty },
                        ty,
                    )),
                }
            }
            TirExprKind::Cast { operand } => {
                let v = self.lower_expr(operand)?;
                Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
            }
            TirExprKind::Select { cond, then_val, else_val } => {
                let c = self.lower_expr(cond)?;
                self.lower_select(c, then_val, else_val, ty)
            }

            TirExprKind::Field { object, name } => {
                let obj = self.lower_expr(object)?;
                let kind = match &e.res {
                    Resolution::FieldSlot(slot) => InstKind::GetFixedField { object: obj, slot: *slot },
                    _ => InstKind::GetProperty { object: obj, name: name.clone() },
                };
                Ok(self.emit(kind, ty))
            }
            TirExprKind::Index { object, index } => {
                let obj = self.lower_expr(object)?;
                let idx = self.lower_expr(index)?;
                let kind = if matches!(self.value_ty(obj), HirType::Array(_)) {
                    InstKind::ArrayGetIndex { object: obj, index: idx }
                } else {
                    InstKind::GetIndex { object: obj, index: idx }
                };
                Ok(self.emit(kind, ty))
            }

            TirExprKind::Call { callee, args } => {
                let cv = match &e.res {
                    Resolution::DirectFn(f) => {
                        let name = self
                            .tir
                            .function(*f)
                            .map(|tf| tf.name.clone())
                            .ok_or(OptError::Unsupported("from_tir: DirectFn out of range"))?;
                        self.emit(InstKind::LoadGlobal(name), HirType::Ref)
                    }
                    _ => self.lower_expr(callee)?,
                };
                self.lower_call(cv, args, ty)
            }
            TirExprKind::MethodCall { recv, name, args } => {
                let r = self.lower_expr(recv)?;
                let argv = self.lower_args(args)?;
                Ok(self.emit(
                    InstKind::MethodCall { recv: r, name: name.clone(), args: argv },
                    ty,
                ))
            }
            TirExprKind::New { class, args } => {
                let name = self
                    .tir
                    .class(*class)
                    .map(|ci| ci.name.clone())
                    .ok_or(OptError::Unsupported("from_tir: New class out of range"))?;
                let cv = self.emit(InstKind::LoadGlobal(name), HirType::Ref);
                self.lower_call(cv, args, ty)
            }
            TirExprKind::MakeVariant { args } => {
                // `E.V(a, b)` — call the variant constructor global by name.
                let (enum_id, tag) = match &e.res {
                    Resolution::EnumVariant { enum_id, tag } => (*enum_id, *tag),
                    _ => return Err(OptError::Unsupported("from_tir: MakeVariant without res")),
                };
                let vname = self
                    .tir
                    .enum_info(enum_id)
                    .and_then(|ei| ei.variants.iter().find(|v| v.tag == tag))
                    .map(|v| v.name.clone())
                    .ok_or(OptError::Unsupported("from_tir: variant out of range"))?;
                let cv = self.emit(InstKind::LoadGlobal(vname), HirType::Ref);
                self.lower_call(cv, args, ty)
            }

            TirExprKind::ArrayLit(els) => {
                let any_spread =
                    els.iter().any(|el| matches!(el, varn_tir::TirArrayEl::Spread(_)));
                if any_spread {
                    let mut vals: Vec<(Value, bool)> = Vec::with_capacity(els.len());
                    for el in els {
                        match el {
                            varn_tir::TirArrayEl::Expr(x) => vals.push((self.lower_expr(x)?, false)),
                            varn_tir::TirArrayEl::Spread(x) => {
                                vals.push((self.lower_expr(x)?, true))
                            }
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
            TirExprKind::TupleLit(xs) => {
                let mut vals = Vec::with_capacity(xs.len());
                for x in xs {
                    vals.push(self.lower_expr(x)?);
                }
                Ok(self.emit(InstKind::BuildTuple { elements: vals }, ty))
            }
            TirExprKind::ObjectLit { entries } => {
                let any_spread = entries
                    .iter()
                    .any(|e| matches!(e, varn_tir::TirObjectEntry::Spread(_)));
                if any_spread {
                    let mut parts: Vec<(Option<Rc<str>>, Value)> = Vec::with_capacity(entries.len());
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

            TirExprKind::Assign { target, value } => {
                let v = self.lower_expr(value)?;
                self.lower_assign(target, v)?;
                Ok(v)
            }

            TirExprKind::Await { future } => {
                let v = self.lower_expr(future)?;
                Ok(self.emit(InstKind::Await { operand: v }, ty))
            }
            TirExprKind::Yield { value, .. } => {
                let v = match value {
                    Some(e) => self.lower_expr(e)?,
                    None => self.emit(InstKind::ConstNull, HirType::Dynamic),
                };
                Ok(self.emit(InstKind::Yield { operand: v }, ty))
            }

            TirExprKind::Discriminant { value } => {
                let v = self.lower_expr(value)?;
                Ok(self.emit(InstKind::GetEnumTag { operand: v }, HirType::Int))
            }
            TirExprKind::VariantPayload { value, field, .. } => {
                let v = self.lower_expr(value)?;
                Ok(self.emit(InstKind::GetFixedField { object: v, slot: *field }, ty))
            }
            TirExprKind::TypeTest { value, class } => {
                let v = self.lower_expr(value)?;
                let cname = self
                    .tir
                    .class(*class)
                    .map(|ci| ci.name.clone())
                    .unwrap_or_else(|| Rc::from("?"));
                let name_v = self.emit(InstKind::ConstStr(cname), HirType::Str);
                Ok(self.emit(
                    InstKind::MethodCall {
                        recv: v,
                        name: Rc::from("__instanceof"),
                        args: vec![name_v],
                    },
                    HirType::Bool,
                ))
            }

            TirExprKind::Closure { func } => Ok(self.emit(
                InstKind::MakeClosure {
                    func: crate::ssa::ir::ClosureBody::Tir(func.0),
                    upvalues_src: vec![],
                },
                HirType::Ref,
            )),
        }
    }

    /// Lower a `Call` / `New` argument list. Returns the spread-tagged form
    /// when any argument is a spread; a named argument is not modelled.
    fn lower_call(&mut self, callee: Value, args: &[varn_tir::TirArg], ty: HirType) -> Result<Value> {
        let mut vals: Vec<(Value, bool)> = Vec::with_capacity(args.len());
        let mut any_spread = false;
        for a in args {
            match a {
                varn_tir::TirArg::Expr(e) => vals.push((self.lower_expr(e)?, false)),
                varn_tir::TirArg::Spread(e) => {
                    any_spread = true;
                    vals.push((self.lower_expr(e)?, true));
                }
                // Named arguments are lowered positionally in written order —
                // precise reordering against the callee signature is later work.
                varn_tir::TirArg::Named { value, .. } => vals.push((self.lower_expr(value)?, false)),
            }
        }
        if any_spread {
            Ok(self.emit(InstKind::CallSpread { callee, args: vals }, ty))
        } else {
            let plain = vals.into_iter().map(|(v, _)| v).collect();
            Ok(self.emit(InstKind::Call { callee, args: plain }, ty))
        }
    }

    fn lower_args(&mut self, args: &[varn_tir::TirArg]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            match a {
                varn_tir::TirArg::Expr(e) => out.push(self.lower_expr(e)?),
                varn_tir::TirArg::Named { value, .. } => out.push(self.lower_expr(value)?),
                varn_tir::TirArg::Spread(_) => {
                    return Err(OptError::Unsupported("from_tir: spread in this position"))
                }
            }
        }
        Ok(out)
    }

    fn lower_assign(&mut self, target: &TirExpr, value: Value) -> Result<()> {
        match &target.kind {
            TirExprKind::Var => match &target.res {
                Resolution::Local(id) => {
                    let cur = self.current;
                    self.write_var(VarId::Local(LocalId(id.0)), cur, value);
                }
                Resolution::Param(i) => {
                    let cur = self.current;
                    self.write_var(VarId::Param(*i), cur, value);
                }
                Resolution::Upvalue(uv) => {
                    self.emit_effect(InstKind::StoreUpvalue { index: *uv, value });
                }
                Resolution::ByName { name, .. } => {
                    self.emit_effect(InstKind::StoreGlobal { name: name.clone(), value });
                }
                Resolution::GlobalSlot(n) => {
                    let name = self
                        .tir
                        .global_names
                        .get(*n as usize)
                        .cloned()
                        .ok_or(OptError::Unsupported("from_tir: assign global slot"))?;
                    self.emit_effect(InstKind::StoreGlobal { name, value });
                }
                _ => return Err(OptError::Unsupported("from_tir: assign target var")),
            },
            TirExprKind::Field { object, name } => {
                let obj = self.lower_expr(object)?;
                let kind = match &target.res {
                    Resolution::FieldSlot(slot) => {
                        InstKind::SetFixedField { object: obj, value, slot: *slot }
                    }
                    _ => InstKind::SetProperty { object: obj, name: name.clone(), value },
                };
                self.emit_effect(kind);
            }
            TirExprKind::Index { object, index } => {
                let obj = self.lower_expr(object)?;
                let idx = self.lower_expr(index)?;
                self.emit_effect(InstKind::SetIndex { object: obj, index: idx, value });
            }
            _ => return Err(OptError::Unsupported("from_tir: assign target")),
        }
        Ok(())
    }

    fn lower_var(&mut self, res: &Resolution, ty: HirType) -> Result<Value> {
        match res {
            Resolution::Local(id) => {
                let var = VarId::Local(LocalId(id.0));
                self.read_var(var, self.current)
            }
            Resolution::Param(i) => self.read_var(VarId::Param(*i), self.current),
            Resolution::Upvalue(uv) => Ok(self.emit(InstKind::LoadUpvalue(*uv), ty)),
            Resolution::GlobalSlot(n) => {
                let name = self
                    .tir
                    .global_names
                    .get(*n as usize)
                    .cloned()
                    .ok_or(OptError::Unsupported("from_tir: global slot out of range"))?;
                Ok(self.emit(InstKind::LoadGlobal(name), ty))
            }
            Resolution::ModuleSlot { .. } => {
                Err(OptError::Unsupported("from_tir: module slot"))
            }
            Resolution::ByName { name, .. } => Ok(self.emit(InstKind::LoadGlobal(name.clone()), ty)),
            // A `Var` node with no resolution is `this`.
            Resolution::None => Ok(self.emit(InstKind::This, ty)),
            _ => Err(OptError::Unsupported("from_tir: var resolution")),
        }
    }

    fn lower_select(
        &mut self,
        cond: Value,
        then_val: &TirExpr,
        else_val: &TirExpr,
        ty: HirType,
    ) -> Result<Value> {
        let then_blk = self.new_block();
        let else_blk = self.new_block();
        let join = self.new_block();
        let from = self.current;
        self.set_term(Terminator::Branch {
            cond,
            then_blk,
            then_args: vec![],
            else_blk,
            else_args: vec![],
        });
        self.add_pred(then_blk, from);
        self.add_pred(else_blk, from);
        self.seal_block(then_blk);
        self.seal_block(else_blk);

        let phi = self.add_block_param(join, ty);

        self.current = then_blk;
        let tv = self.lower_expr(then_val)?;
        let tfrom = self.current;
        self.set_term(Terminator::Jump { target: join, args: vec![tv] });
        self.add_pred(join, tfrom);

        self.current = else_blk;
        let ev = self.lower_expr(else_val)?;
        let efrom = self.current;
        self.set_term(Terminator::Jump { target: join, args: vec![ev] });
        self.add_pred(join, efrom);

        self.seal_block(join);
        self.current = join;
        Ok(phi)
    }
}

fn bin_op(op: TirBinOp) -> HirBinOp {
    match op {
        TirBinOp::Add => HirBinOp::Add,
        TirBinOp::Sub => HirBinOp::Sub,
        TirBinOp::Mul => HirBinOp::Mul,
        TirBinOp::Div => HirBinOp::Div,
        TirBinOp::Mod => HirBinOp::Mod,
        TirBinOp::Pow => HirBinOp::Pow,
        TirBinOp::Eq => HirBinOp::Eq,
        TirBinOp::Ne => HirBinOp::Ne,
        TirBinOp::Lt => HirBinOp::Lt,
        TirBinOp::Le => HirBinOp::Le,
        TirBinOp::Gt => HirBinOp::Gt,
        TirBinOp::Ge => HirBinOp::Ge,
        TirBinOp::BitAnd => HirBinOp::BitAnd,
        TirBinOp::BitOr => HirBinOp::BitOr,
        TirBinOp::BitXor => HirBinOp::BitXor,
        TirBinOp::Shl => HirBinOp::Shl,
        TirBinOp::Shr => HirBinOp::Shr,
        TirBinOp::Ushr => HirBinOp::Ushr,
    }
}

fn un_op(op: TirUnOp) -> HirUnOp {
    match op {
        TirUnOp::Neg => HirUnOp::Neg,
        TirUnOp::Not => HirUnOp::Not,
        TirUnOp::BitNot => HirUnOp::BitNot,
        TirUnOp::IsNull => unreachable!("handled by lower_expr"),
    }
}

/// Build one `SsaFunc` from a `TirFunction`.
pub fn build_function(tir: &TirModule, func: &TirFunction) -> Result<SsaFunc> {
    let mut b = Builder::new(tir);
    b.next_synthetic = func.locals.len() as u32;
    let entry = b.current;

    for (i, pty) in func.params.iter().enumerate() {
        let t = b.ty(*pty);
        let v = b.new_value(t);
        b.block_mut(entry).params.push(v);
        b.write_var(VarId::Param(i as u32), entry, v);
    }

    b.lower_block(&func.body)?;
    if b.is_open() {
        b.set_term(Terminator::Return(None));
    }

    Ok(SsaFunc {
        name: func.name.clone(),
        entry,
        blocks: b.blocks,
        values: b.values,
        pinned_vars: FxHashSet::default(),
        nlocals: func.locals.len() as u32,
        is_async: func.is_async,
        is_generator: func.is_generator,
    })
}

/// Build every function in the module: top level first, then the rest.
pub fn build_module(tir: &TirModule) -> Result<Vec<SsaFunc>> {
    let mut out = Vec::with_capacity(tir.functions.len() + 1);
    out.push(build_function(tir, &tir.top_level)?);
    for f in &tir.functions {
        out.push(build_function(tir, f)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_tir::{
        BackendTy as B, ClassInfo, Resolution as R, Signature, Span, TirExpr, TirExprKind as K,
        TirModule, TyTable,
    };
    use std::rc::Rc;

    fn module(top_body: Vec<TirStmt>, params: Vec<B>) -> TirModule {
        let mut types = TyTable::default();
        let _ = types.intern(B::Never);
        TirModule {
            source_file: Rc::from("t.vn"),
            types,
            classes: vec![ClassInfo::new(Rc::from("C"), None, vec![])],
            enums: vec![],
            signatures: vec![Signature { params: vec![], return_ty: B::Void }],
            functions: vec![],
            globals: vec![], global_names: vec![],
            top_level: TirFunction {
                name: Rc::from("<module>"),
                sig: varn_tir::SigId(0),
                params,
                return_ty: B::Void,
                locals: vec![B::Int],
                body: top_body,
                has_this: false,
                this_class: None,
                is_async: false,
                is_generator: false,
            },
        }
    }

    fn e(kind: K, ty: B) -> TirExpr {
        TirExpr { kind, ty, res: R::None, span: Span::EMPTY }
    }

    #[test]
    fn a_return_of_int_arithmetic_builds() {
        let add = e(
            K::Binary {
                op: TirBinOp::Add,
                lhs: Box::new(e(K::IntLit(1), B::Int)),
                rhs: Box::new(e(K::IntLit(2), B::Int)),
            },
            B::Int,
        );
        let m = module(vec![TirStmt::Return(Some(add))], vec![]);
        let f = build_function(&m, &m.top_level).unwrap();
        assert_eq!(f.blocks.len(), 1);
        assert!(matches!(f.blocks[0].term, Terminator::Return(Some(_))));
    }

    #[test]
    fn an_if_produces_three_blocks_and_a_join() {
        let cond = e(K::BoolLit(true), B::Bool);
        let m = module(
            vec![TirStmt::If {
                cond,
                then_body: vec![TirStmt::Return(Some(e(K::IntLit(1), B::Int)))],
                else_body: vec![],
            }],
            vec![],
        );
        let f = build_function(&m, &m.top_level).unwrap();
        assert!(f.blocks.len() >= 3);
    }

    #[test]
    fn a_let_then_read_flows_through_the_variable() {
        let m = module(
            vec![
                TirStmt::Let {
                    local: varn_tir::LocalId(0),
                    ty: B::Int,
                    init: Some(e(K::IntLit(7), B::Int)),
                },
                TirStmt::Return(Some(TirExpr {
                    kind: K::Var,
                    ty: B::Int,
                    res: R::Local(varn_tir::LocalId(0)),
                    span: Span::EMPTY,
                })),
            ],
            vec![],
        );
        let f = build_function(&m, &m.top_level).unwrap();
        assert!(matches!(f.blocks[0].term, Terminator::Return(Some(_))));
    }

    #[test]
    fn a_closure_lowers_to_make_closure() {
        let m = module(
            vec![TirStmt::Expr(e(
                K::Closure { func: varn_tir::FnId(0) },
                B::Dynamic(varn_tir::DynReason::Unannotated),
            ))],
            vec![],
        );
        let f = build_function(&m, &m.top_level).unwrap();
        assert!(f.blocks[0]
            .insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::MakeClosure { .. })));
    }

    #[test]
    fn a_new_expression_lowers_to_a_call() {
        let m = module(
            vec![TirStmt::Expr(e(
                K::New { class: varn_tir::ClassId(0), args: vec![] },
                B::Class(varn_tir::ClassId(0)),
            ))],
            vec![],
        );
        let f = build_function(&m, &m.top_level).unwrap();
        assert!(f.blocks[0].insts.iter().any(|i| matches!(i.kind, InstKind::Call { .. })));
    }
}
