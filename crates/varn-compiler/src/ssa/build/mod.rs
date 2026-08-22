use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;

use crate::hir::{HirBinding, HirFunction, HirStmt, HirType, LocalId};
use crate::OptError;

use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, ValueDef, VarId};

mod expr;
mod phi;
mod pinned;
mod stmt;

use phi::simplify_phis;
use pinned::scan_pinned_vars;

type Result<T> = std::result::Result<T, OptError>;

#[derive(Debug, Clone, Copy)]
struct LoopCtx {
    continue_target: BlockId,
    break_target: BlockId,
    try_region_depth: usize,
}

struct Builder {
    blocks: Vec<Block>,
    values: Vec<ValueDef>,
    sealed: Vec<bool>,

    defs: FxHashMap<(VarId, BlockId), Value>,

    var_ty: FxHashMap<VarId, HirType>,

    incomplete_phis: FxHashMap<BlockId, Vec<(VarId, Value)>>,

    loops: Vec<LoopCtx>,

    next_synthetic: u32,

    current: BlockId,
    pinned_vars: FxHashSet<VarId>,
    open_try_regions: Vec<Vec<HirStmt>>,
    source_file: Option<Rc<str>>,
}

impl Builder {
    fn new(pinned_vars: FxHashSet<VarId>, source_file: Option<Rc<str>>) -> Self {
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
            pinned_vars,
            open_try_regions: Vec::new(),
            source_file,
        };
        let entry = b.new_block();
        b.sealed[entry.0 as usize] = true;
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

    fn is_open(&self) -> bool {
        matches!(
            self.blocks[self.current.0 as usize].term,
            Terminator::Unreachable
        )
    }

    fn set_term(&mut self, term: Terminator) {
        self.block_mut(self.current).term = term;
    }

    fn add_pred(&mut self, block: BlockId, pred: BlockId) {
        self.block_mut(block).preds.push(pred);
    }

    fn emit(&mut self, kind: InstKind, ty: HirType) -> Value {
        let dest = self.new_value(ty);
        self.block_mut(self.current).insts.push(Inst {
            dest: Some(dest),
            kind,
        });
        dest
    }

    fn emit_effect(&mut self, kind: InstKind) {
        self.block_mut(self.current)
            .insts
            .push(Inst { dest: None, kind });
    }

    fn fresh_synthetic(&mut self) -> VarId {
        let id = VarId::Local(LocalId(self.next_synthetic));
        self.next_synthetic += 1;
        id
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
            .ok_or(OptError::Unsupported("ssa: read of undefined variable"))?;

        if !self.sealed[block.0 as usize] {
            let phi = self.add_block_param(block, ty);
            self.incomplete_phis
                .entry(block)
                .or_default()
                .push((var, phi));
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

    pub(super) fn load_binding(&mut self, binding: &HirBinding) -> Result<Value> {
        let var = match binding {
            HirBinding::Param(i) => VarId::Param(*i),
            HirBinding::Local(id) => VarId::Local(*id),
            HirBinding::Global(name) => {
                return Ok(self.emit(InstKind::LoadGlobal(name.clone()), HirType::Dynamic));
            }
            HirBinding::Upvalue(uv) => {
                return Ok(self.emit(InstKind::LoadUpvalue(*uv), HirType::Dynamic));
            }
        };
        if self.pinned_vars.contains(&var) {
            let ty = *self.var_ty.get(&var).unwrap_or(&HirType::Dynamic);
            Ok(self.emit(InstKind::LoadCaptured { var }, ty))
        } else {
            self.read_var(var, self.current)
        }
    }

    pub(super) fn store_binding(&mut self, binding: &HirBinding, value: Value) {
        let var = match binding {
            HirBinding::Param(i) => VarId::Param(*i),
            HirBinding::Local(id) => VarId::Local(*id),
            HirBinding::Global(name) => {
                self.emit_effect(InstKind::StoreGlobal {
                    name: name.clone(),
                    value,
                });
                return;
            }
            HirBinding::Upvalue(uv) => {
                self.emit_effect(InstKind::StoreUpvalue { index: *uv, value });
                return;
            }
        };
        if self.pinned_vars.contains(&var) {
            self.var_ty.insert(var, self.values[value.0 as usize].ty);
            self.emit_effect(InstKind::StoreCaptured { var, value });
        } else {
            self.write_var(var, self.current, value);
        }
    }
}

pub fn build_function(
    func: &HirFunction,
    module_funcs: &[HirFunction],
    source_file: Option<Rc<str>>,
) -> Result<SsaFunc> {
    let pinned = scan_pinned_vars(func);
    let mut b = Builder::new(pinned, source_file.clone());

    b.next_synthetic = func.locals;
    let entry = b.current;

    for (i, param) in func.params.iter().enumerate() {
        let v = b.new_value(param.ty);
        b.block_mut(entry).params.push(v);
        b.write_var(VarId::Param(i as u32), entry, v);
    }

    for f in module_funcs {
        let fn_val = b.emit(
            InstKind::MakeClosure {
                func: Rc::new(f.clone()),
                upvalues: Vec::new(),
                upvalues_src: Vec::new(),
            },
            HirType::Ref,
        );
        let name = if let Some(ref src) = source_file {
            Rc::from(format!("{}::{}", src.replace('\\', "/"), f.name))
        } else {
            f.name.clone()
        };
        b.emit_effect(InstKind::StoreGlobal {
            name,
            value: fn_val,
        });
    }

    for (i, param) in func.params.iter().enumerate() {
        if let Some(default) = &param.default {
            let pv = b.read_var(VarId::Param(i as u32), b.current)?;
            let isnull = b.emit(InstKind::IsNull { operand: pv }, HirType::Bool);
            let resolved =
                b.lower_branch_value(isnull, |s| s.lower_expr(default), |_| Ok(pv), param.ty)?;
            let cur = b.current;
            b.write_var(VarId::Param(i as u32), cur, resolved);
        }
    }

    b.lower_block(&func.body)?;

    if b.is_open() {
        b.set_term(Terminator::Return(None));
    }

    let mut ssa = SsaFunc {
        name: func.name.clone(),
        entry,
        blocks: b.blocks,
        values: b.values,
        pinned_vars: b.pinned_vars,
        nlocals: func.locals,
        is_async: func.is_async,
        is_generator: func.is_generator,
    };
    simplify_phis(&mut ssa);
    Ok(ssa)
}
