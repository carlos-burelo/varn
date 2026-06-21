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

use rustc_hash::FxHashMap;

use crate::hir::{HirBinding, HirFunction, HirType, LocalId};
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
    /// Next id for a synthetic loop variable (e.g. a `for-in` index). Set above
    /// the function's real local count so it never collides.
    next_synthetic: u32,
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
            next_synthetic: 0,
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

    /// A fresh synthetic loop variable (e.g. a `for-in` index), SSA-renamed by
    /// the normal Braun machinery.
    fn fresh_synthetic(&mut self) -> VarId {
        let id = VarId::Local(LocalId(self.next_synthetic));
        self.next_synthetic += 1;
        id
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
}

mod expr;
mod stmt;
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
    // Synthetic loop vars (e.g. `for-in` index) use ids above the real locals.
    b.next_synthetic = func.locals;
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
                InstKind::AssertNotNull { operand } => sub(operand),
                InstKind::GetPropertyMaybe { object, .. } => sub(object),
                InstKind::ModuleSlot { object, .. } => sub(object),
                InstKind::GetEnumTag { operand } => sub(operand),
                InstKind::IsArray { operand } => sub(operand),
                InstKind::This => {}
                InstKind::Range { start, end, .. } => {
                    sub(start);
                    sub(end);
                }
                InstKind::ObjectKeys { operand } => sub(operand),
                InstKind::GetSymbol { object, .. } => sub(object),
                InstKind::IterCall { callee, recv } => {
                    sub(callee);
                    sub(recv);
                }
                InstKind::SuperCall { args } => args.iter_mut().for_each(sub),
                InstKind::SuperMethodCall { args, .. } => args.iter_mut().for_each(sub),
                InstKind::GetSuper { .. }
                | InstKind::ConstInt(_)
                | InstKind::ConstFloat(_)
                | InstKind::ConstBool(_)
                | InstKind::ConstStr(_)
                | InstKind::ConstChar(_)
                | InstKind::ConstDecimal(_)
                | InstKind::ConstBigInt(_)
                | InstKind::ConstNull
                | InstKind::LoadGlobal(_) => {}
            }
        }
        match &mut block.term {
            Terminator::Return(Some(v)) => sub(v),
            Terminator::Throw(v) => sub(v),
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
