//! HIR → SSA construction (Braun et al. 2013, on-the-fly).
//!
//! Variables (params + locals) become SSA [`Value`]s via per-block `write`/`read`
//! definition maps. At a merge point a read of a variable with no local
//! definition creates a **block parameter** (the phi equivalent) and recurses
//! into the predecessors for the operands; predecessors carry the operands as
//! block arguments on their terminators.
//!
//! Blocks are *sealed* once all their predecessors are known. Reads in a sealed
//! block resolve immediately; reads in an unsealed block (loop headers, whose
//! back-edge is not yet built) record an incomplete phi filled at seal time.
//!
//! Trivial phis (one distinct operand, or only self) are removed by a fixpoint
//! [`simplify_phis`] post-pass, which keeps the SSA minimal without interleaving
//! use-replacement with construction.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinOp, HirBinding, HirExpr, HirFunction, HirLogicalOp,
    HirObjectProp, HirPropKey, HirStmt, HirTemplatePart, HirType, HirUpdateOp, LocalId,
};
use crate::OptError;

use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, ValueDef};

/// A source variable being SSA-renamed: a parameter slot or a local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VarId {
    Param(u32),
    Local(LocalId),
}

type Result<T> = std::result::Result<T, OptError>;

/// The enclosing loop's branch targets, pushed while a loop body is lowered so
/// `break`/`continue` know where to jump. `continue_target` is the loop header
/// for `while`/`do-while` and the update block for a C-style `for`.
#[derive(Debug, Clone, Copy)]
struct LoopCtx {
    continue_target: BlockId,
    break_target: BlockId,
}

struct Builder {
    blocks: Vec<Block>,
    values: Vec<ValueDef>,
    sealed: Vec<bool>,
    /// Braun's `currentDef`: `(variable, block)` → its value in that block.
    defs: FxHashMap<(VarId, BlockId), Value>,
    /// Static type of each variable (for typing freshly created phis).
    var_ty: FxHashMap<VarId, HirType>,
    /// Phis created in a block before it was sealed; operands filled on seal.
    incomplete_phis: FxHashMap<BlockId, Vec<(VarId, Value)>>,
    /// Enclosing loops, innermost last (`break`/`continue` targets).
    loops: Vec<LoopCtx>,
    /// The block instructions are currently appended to.
    current: BlockId,
}

impl Builder {
    fn new() -> Self {
        let mut b = Builder {
            blocks: Vec::new(),
            values: Vec::new(),
            sealed: Vec::new(),
            defs: FxHashMap::default(),
            var_ty: FxHashMap::default(),
            incomplete_phis: FxHashMap::default(),
            loops: Vec::new(),
            current: BlockId(0),
        };
        let entry = b.new_block();
        b.sealed[entry.0 as usize] = true; // entry has no predecessors
        b.current = entry;
        b
    }

    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            term: Terminator::Unreachable,
            preds: Vec::new(),
        });
        self.sealed.push(false);
        id
    }

    fn new_value(&mut self, ty: HirType) -> Value {
        let v = Value(self.values.len() as u32);
        self.values.push(ValueDef { ty });
        v
    }

    fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }

    /// The current block accepts no more instructions once it has a terminator.
    fn is_open(&self) -> bool {
        matches!(self.blocks[self.current.0 as usize].term, Terminator::Unreachable)
    }

    fn set_term(&mut self, term: Terminator) {
        self.block_mut(self.current).term = term;
    }

    fn add_pred(&mut self, block: BlockId, pred: BlockId) {
        self.block_mut(block).preds.push(pred);
    }

    /// Append an instruction defining a fresh value of `ty` to the current block.
    fn emit(&mut self, kind: InstKind, ty: HirType) -> Value {
        let dest = self.new_value(ty);
        self.block_mut(self.current).insts.push(Inst {
            dest: Some(dest),
            kind,
        });
        dest
    }

    /// Append a side-effecting instruction that produces no value (`dest: None`).
    fn emit_effect(&mut self, kind: InstKind) {
        self.block_mut(self.current).insts.push(Inst { dest: None, kind });
    }

    // ----- variable read/write (Braun) -----------------------------------

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
            .ok_or(OptError::Unsupported("ssa: read of undefined variable"))?;

        if !self.sealed[block.0 as usize] {
            // Loop header not yet sealed: emit a phi now, fill operands on seal.
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
            self.write_var(var, block, phi); // break cycles before recursing
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

    /// Fill a phi's operands: for each predecessor, append the variable's value
    /// on that edge as a block argument at the phi's parameter position.
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

    /// Append `arg` to the `pred -> block` edge's argument list (must land at
    /// index `pos`, i.e. operands are filled in parameter order).
    fn append_edge_arg(&mut self, pred: BlockId, block: BlockId, pos: usize, arg: Value) {
        match &mut self.block_mut(pred).term {
            Terminator::Jump { target, args } if *target == block => {
                debug_assert_eq!(args.len(), pos);
                args.push(arg);
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    debug_assert_eq!(then_args.len(), pos);
                    then_args.push(arg);
                }
                if *else_blk == block {
                    debug_assert_eq!(else_args.len(), pos);
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

    // ----- lowering -------------------------------------------------------

    fn lower_block(&mut self, stmts: &[HirStmt]) -> Result<()> {
        for stmt in stmts {
            if !self.is_open() {
                break; // remaining statements are unreachable
            }
            self.lower_stmt(stmt)?;
        }
        Ok(())
    }

    fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<()> {
        match stmt {
            HirStmt::Expr(e) => {
                self.lower_expr(e)?;
                Ok(())
            }
            HirStmt::Let { local, value, .. } => {
                let v = self.lower_expr(value)?;
                self.write_var(VarId::Local(*local), self.current, v);
                Ok(())
            }
            HirStmt::Assign { target, value } => {
                let var = binding_var(target)?;
                let v = self.lower_expr(value)?;
                self.write_var(var, self.current, v);
                Ok(())
            }
            HirStmt::SetMember { object, name, value } => {
                let o = self.lower_expr(object)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetProperty { object: o, name: name.clone(), value: v });
                Ok(())
            }
            HirStmt::SetIndex { object, index, value } => {
                let o = self.lower_expr(object)?;
                let i = self.lower_expr(index)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetIndex { object: o, index: i, value: v });
                Ok(())
            }
            HirStmt::Return(value) => {
                let v = match value {
                    Some(e) => Some(self.lower_expr(e)?),
                    None => None,
                };
                self.set_term(Terminator::Return(v));
                Ok(())
            }
            HirStmt::If {
                test,
                then_body,
                else_body,
            } => self.lower_if(test, then_body, else_body),
            HirStmt::While { test, body } => self.lower_while(test, body),
            HirStmt::ForClassic {
                test,
                update,
                body,
            } => self.lower_for_classic(test, update, body),
            HirStmt::DoWhile { body, test } => self.lower_do_while(body, test),
            HirStmt::Break => self.lower_jump_out(|c| c.break_target, "ssa: break outside loop"),
            HirStmt::Continue => {
                self.lower_jump_out(|c| c.continue_target, "ssa: continue outside loop")
            }
            _ => Err(OptError::Unsupported("ssa: statement kind")),
        }
    }

    /// Lower `break`/`continue`: terminate the current block with a jump to the
    /// innermost loop's chosen target and record the edge. Subsequent statements
    /// in the block are unreachable (`lower_block` stops at the closed block).
    fn lower_jump_out(
        &mut self,
        pick: impl Fn(&LoopCtx) -> BlockId,
        err: &'static str,
    ) -> Result<()> {
        let target = pick(self.loops.last().ok_or(OptError::Unsupported(err))?);
        let from = self.current;
        self.set_term(Terminator::Jump {
            target,
            args: Vec::new(),
        });
        self.add_pred(target, from);
        Ok(())
    }

    fn lower_if(
        &mut self,
        test: &HirExpr,
        then_body: &[HirStmt],
        else_body: &[HirStmt],
    ) -> Result<()> {
        let cond = self.lower_expr(test)?;
        let entry = self.current;
        let then_b = self.new_block();
        let merge = self.new_block();
        let else_b = if else_body.is_empty() {
            None
        } else {
            Some(self.new_block())
        };
        let else_target = else_b.unwrap_or(merge);

        self.set_term(Terminator::Branch {
            cond,
            then_blk: then_b,
            then_args: Vec::new(),
            else_blk: else_target,
            else_args: Vec::new(),
        });
        self.add_pred(then_b, entry);
        self.add_pred(else_target, entry);
        // then/else predecessors are fully known now; seal before lowering so
        // their single-predecessor reads don't spawn spurious phis.
        self.seal_block(then_b);
        if let Some(eb) = else_b {
            self.seal_block(eb);
        }

        self.current = then_b;
        self.lower_block(then_body)?;
        // The block that actually falls through to `merge` is whatever block is
        // current after the body — not `then_b`, which differs once the body
        // contains nested control flow.
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: merge,
                args: Vec::new(),
            });
            self.add_pred(merge, from);
        }

        if let Some(eb) = else_b {
            self.current = eb;
            self.lower_block(else_body)?;
            if self.is_open() {
                let from = self.current;
                self.set_term(Terminator::Jump {
                    target: merge,
                    args: Vec::new(),
                });
                self.add_pred(merge, from);
            }
        }

        self.seal_block(merge);
        self.current = merge;
        Ok(())
    }

    /// Branch on `cond` (truthy), evaluate one arm in each successor, and merge
    /// their values through a single block parameter (the result phi). Used for
    /// short-circuiting logicals and the ternary. The arm closures must not
    /// terminate the block (pure expressions; any unsupported sub-expr makes the
    /// whole function fall back before reaching here).
    fn lower_branch_value(
        &mut self,
        cond: Value,
        then_fn: impl FnOnce(&mut Self) -> Result<Value>,
        else_fn: impl FnOnce(&mut Self) -> Result<Value>,
        ty: HirType,
    ) -> Result<Value> {
        let entry = self.current;
        let then_b = self.new_block();
        let else_b = self.new_block();
        let merge = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: then_b,
            then_args: Vec::new(),
            else_blk: else_b,
            else_args: Vec::new(),
        });
        self.add_pred(then_b, entry);
        self.add_pred(else_b, entry);
        self.seal_block(then_b);
        self.seal_block(else_b);

        self.current = then_b;
        let vt = then_fn(self)?;
        let t_end = self.current;
        self.set_term(Terminator::Jump { target: merge, args: vec![vt] });
        self.add_pred(merge, t_end);

        self.current = else_b;
        let ve = else_fn(self)?;
        let e_end = self.current;
        self.set_term(Terminator::Jump { target: merge, args: vec![ve] });
        self.add_pred(merge, e_end);

        self.seal_block(merge);
        self.current = merge;
        // The result phi: a single block param fed by the two edge args above.
        Ok(self.add_block_param(merge, ty))
    }

    /// `while (test) body` — a pre-tested loop. The header is left unsealed while
    /// the body is lowered so reads of variables modified in the loop spawn
    /// incomplete phis (Braun); the back-edge supplies their loop-carried
    /// operand and `seal_block(header)` fills them.
    fn lower_while(&mut self, test: &HirExpr, body: &[HirStmt]) -> Result<()> {
        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);
        // header intentionally NOT sealed: the back-edge is built below.

        self.current = header;
        let cond = self.lower_expr(test)?;
        let body_b = self.new_block();
        let exit_b = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, header);
        self.add_pred(exit_b, header);
        self.seal_block(body_b); // sole predecessor (header) is known

        self.loops.push(LoopCtx {
            continue_target: header,
            break_target: exit_b,
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from); // back-edge
        }
        self.loops.pop();

        self.seal_block(header); // back-edge + any `continue` edges now present
        self.seal_block(exit_b); // header's else + any `break` edges now present
        self.current = exit_b;
        Ok(())
    }

    /// C-style `for`: `test` at the header, `update` after the body in its own
    /// block (where `continue` lands, so the increment is never skipped), then
    /// the back-edge to the header. `init` is lowered as ordinary statements
    /// before this node.
    fn lower_for_classic(
        &mut self,
        test: &HirExpr,
        update: &[HirStmt],
        body: &[HirStmt],
    ) -> Result<()> {
        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);

        self.current = header;
        let cond = self.lower_expr(test)?;
        let body_b = self.new_block();
        let update_b = self.new_block();
        let exit_b = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, header);
        self.add_pred(exit_b, header);
        self.seal_block(body_b);

        self.loops.push(LoopCtx {
            continue_target: update_b,
            break_target: exit_b,
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: update_b,
                args: Vec::new(),
            });
            self.add_pred(update_b, from);
        }
        self.loops.pop();

        self.seal_block(update_b); // body fall-through + any `continue` edges
        self.current = update_b;
        self.lower_block(update)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from); // back-edge
        }

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    /// `do body while (test)` — the body runs once before the test. The body is
    /// the loop entry and stays unsealed for the back-edge; the test lives in a
    /// dedicated latch block, which is where `continue` lands.
    fn lower_do_while(&mut self, body: &[HirStmt], test: &HirExpr) -> Result<()> {
        let pre = self.current;
        let body_b = self.new_block();
        self.set_term(Terminator::Jump {
            target: body_b,
            args: Vec::new(),
        });
        self.add_pred(body_b, pre);
        // body_b NOT sealed: the back-edge from the latch is built below.
        let latch = self.new_block();
        let exit_b = self.new_block();

        self.loops.push(LoopCtx {
            continue_target: latch,
            break_target: exit_b,
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: latch,
                args: Vec::new(),
            });
            self.add_pred(latch, from);
        }
        self.loops.pop();

        self.seal_block(latch); // body fall-through + any `continue` edges
        self.current = latch;
        let cond = self.lower_expr(test)?;
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, latch); // back-edge
        self.add_pred(exit_b, latch);
        self.seal_block(body_b); // pre + back-edge now present
        self.seal_block(exit_b); // latch's else + any `break` edges now present
        self.current = exit_b;
        Ok(())
    }

    fn lower_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Int(n) => Ok(self.emit(InstKind::ConstInt(*n), HirType::Int)),
            HirExpr::Float(n) => Ok(self.emit(InstKind::ConstFloat(*n), HirType::Float)),
            HirExpr::Bool(b) => Ok(self.emit(InstKind::ConstBool(*b), HirType::Bool)),
            HirExpr::Str(s) => Ok(self.emit(InstKind::ConstStr(s.clone()), HirType::Str)),
            HirExpr::Char(c) => Ok(self.emit(InstKind::ConstChar(*c), HirType::Int)),
            HirExpr::Null => Ok(self.emit(InstKind::ConstNull, HirType::Dynamic)),
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

fn binding_var(binding: &HirBinding) -> Result<VarId> {
    match binding {
        HirBinding::Param(i) => Ok(VarId::Param(*i)),
        HirBinding::Local(id) => Ok(VarId::Local(*id)),
        HirBinding::Global(_) => Err(OptError::Unsupported("ssa: global binding")),
        HirBinding::Upvalue(_) => Err(OptError::Unsupported("ssa: upvalue binding")),
    }
}

pub fn build_function(func: &HirFunction) -> Result<SsaFunc> {
    // Default-valued params need the `IsNull`/`Move` prologue the naive emitter
    // produces; SSA construction does not model it yet, so decline them.
    if func.params.iter().any(|p| p.default.is_some()) {
        return Err(OptError::Unsupported("ssa: default param"));
    }

    let mut b = Builder::new();
    let entry = b.current;

    for (i, param) in func.params.iter().enumerate() {
        let v = b.new_value(param.ty);
        b.block_mut(entry).params.push(v);
        b.write_var(VarId::Param(i as u32), entry, v);
    }

    b.lower_block(&func.body)?;

    if b.is_open() {
        b.set_term(Terminator::Return(None)); // implicit `return null` epilogue
    }

    let mut ssa = SsaFunc {
        name: func.name.clone(),
        entry,
        blocks: b.blocks,
        values: b.values,
    };
    simplify_phis(&mut ssa);
    Ok(ssa)
}

/// Remove trivial block parameters (phis with ≤1 distinct non-self operand) to
/// fixpoint, replacing each removed value with its unique operand. Mirrors
/// Braun's `tryRemoveTrivialPhi`, run as a post-pass over the finished CFG.
fn simplify_phis(func: &mut SsaFunc) {
    loop {
        let mut removed = None;
        'scan: for b in 0..func.blocks.len() {
            // Entry-block parameters are the function parameters, not phis.
            if BlockId(b as u32) == func.entry {
                continue;
            }
            for pos in 0..func.blocks[b].params.len() {
                let phi = func.blocks[b].params[pos];
                if let Some(same) = trivial_operand(func, BlockId(b as u32), pos, phi) {
                    remove_param(func, BlockId(b as u32), pos);
                    removed = Some((phi, same));
                    break 'scan;
                }
            }
        }
        match removed {
            Some((phi, same)) => replace_all_uses(func, phi, same),
            None => break,
        }
    }
}

/// If the phi at `block`'s parameter `pos` has at most one distinct operand
/// besides itself, return that operand (the value it collapses to).
fn trivial_operand(func: &SsaFunc, block: BlockId, pos: usize, phi: Value) -> Option<Value> {
    let mut same: Option<Value> = None;
    for &pred in &func.blocks[block.0 as usize].preds {
        let op = edge_arg(func, pred, block, pos);
        if op == phi || same == Some(op) {
            continue;
        }
        if same.is_some() {
            return None; // ≥2 distinct real operands: not trivial
        }
        same = Some(op);
    }
    Some(same.unwrap_or(phi))
}

fn edge_arg(func: &SsaFunc, pred: BlockId, block: BlockId, pos: usize) -> Value {
    match &func.blocks[pred.0 as usize].term {
        Terminator::Jump { target, args } if *target == block => args[pos],
        Terminator::Branch {
            then_blk,
            then_args,
            else_blk,
            else_args,
            ..
        } => {
            if *then_blk == block {
                then_args[pos]
            } else {
                debug_assert_eq!(*else_blk, block);
                else_args[pos]
            }
        }
        _ => panic!("no edge {pred:?} -> {block:?}"),
    }
}

/// Remove block parameter `pos` from `block` and the matching argument from
/// every predecessor edge.
fn remove_param(func: &mut SsaFunc, block: BlockId, pos: usize) {
    func.blocks[block.0 as usize].params.remove(pos);
    let preds = func.blocks[block.0 as usize].preds.clone();
    for pred in preds {
        match &mut func.blocks[pred.0 as usize].term {
            Terminator::Jump { target, args } if *target == block => {
                args.remove(pos);
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    then_args.remove(pos);
                }
                if *else_blk == block {
                    else_args.remove(pos);
                }
            }
            _ => {}
        }
    }
}

/// Replace every *use* of `old` with `new`: instruction operands and terminator
/// values (condition, return value, block arguments).
fn replace_all_uses(func: &mut SsaFunc, old: Value, new: Value) {
    let sub = |v: &mut Value| {
        if *v == old {
            *v = new;
        }
    };
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            match &mut inst.kind {
                InstKind::Binary { lhs, rhs, .. } => {
                    sub(lhs);
                    sub(rhs);
                }
                InstKind::Unary { operand, .. } => sub(operand),
                InstKind::Call { callee, args } => {
                    sub(callee);
                    args.iter_mut().for_each(sub);
                }
                InstKind::SelfCall { args } => args.iter_mut().for_each(sub),
                InstKind::GetProperty { object, .. } => sub(object),
                InstKind::GetIndex { object, index } => {
                    sub(object);
                    sub(index);
                }
                InstKind::SetProperty { object, value, .. } => {
                    sub(object);
                    sub(value);
                }
                InstKind::SetIndex { object, index, value } => {
                    sub(object);
                    sub(index);
                    sub(value);
                }
                InstKind::MethodCall { recv, args, .. } => {
                    sub(recv);
                    args.iter_mut().for_each(sub);
                }
                InstKind::IsNull { operand } => sub(operand),
                InstKind::BuildArray { elements } => elements.iter_mut().for_each(sub),
                InstKind::BuildObject { pairs } => pairs.iter_mut().for_each(|(_, v)| sub(v)),
                InstKind::ToString { operand } => sub(operand),
                InstKind::BuildStr { parts } => parts.iter_mut().for_each(sub),
                InstKind::MakeClosure { .. } => {}
                InstKind::IntrinsicCall { object, args, .. } => {
                    sub(object);
                    args.iter_mut().for_each(sub);
                }
                InstKind::ConstInt(_)
                | InstKind::ConstFloat(_)
                | InstKind::ConstBool(_)
                | InstKind::ConstStr(_)
                | InstKind::ConstChar(_)
                | InstKind::ConstNull
                | InstKind::LoadGlobal(_) => {}
            }
        }
        match &mut block.term {
            Terminator::Return(Some(v)) => sub(v),
            Terminator::Return(None) | Terminator::Unreachable => {}
            Terminator::Jump { args, .. } => args.iter_mut().for_each(sub),
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                sub(cond);
                then_args.iter_mut().for_each(sub);
                else_args.iter_mut().for_each(sub);
            }
        }
    }
}
