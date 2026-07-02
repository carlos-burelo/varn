use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinding, HirExpr, HirFunction, HirObjectProp,
    HirOptionalProperty, HirPropKey, HirStmt, HirTemplatePart, HirType, HirUpvalueSrc, LocalId,
};
use crate::OptError;

use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, ValueDef, VarId};

type Result<T> = std::result::Result<T, OptError>;

#[derive(Debug, Clone, Copy)]
struct LoopCtx {
    continue_target: BlockId,
    break_target: BlockId,
    finally_depth: usize,
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
    finally_stack: Vec<Vec<HirStmt>>,
}

impl Builder {
    fn new(pinned_vars: FxHashSet<VarId>) -> Self {
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
            finally_stack: Vec::new(),
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

mod expr;
mod stmt;

fn scan_stmt(stmt: &HirStmt, pinned: &mut FxHashSet<VarId>, in_try: bool) {
    match stmt {
        HirStmt::Expr(expr) => scan_expr(expr, pinned, in_try),
        HirStmt::Let { value, .. } => scan_expr(value, pinned, in_try),
        HirStmt::Assign { target, value } => {
            scan_expr(value, pinned, in_try);
            if in_try {
                match target {
                    HirBinding::Local(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirBinding::Param(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirStmt::SetMember { object, value, .. } => {
            scan_expr(object, pinned, in_try);
            scan_expr(value, pinned, in_try);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            scan_expr(object, pinned, in_try);
            scan_expr(index, pinned, in_try);
            scan_expr(value, pinned, in_try);
        }
        HirStmt::Return(value) => {
            if let Some(e) = value {
                scan_expr(e, pinned, in_try);
            }
        }
        HirStmt::Throw(expr) => scan_expr(expr, pinned, in_try),
        HirStmt::If {
            test,
            then_body,
            else_body,
        } => {
            scan_expr(test, pinned, in_try);
            for s in then_body {
                scan_stmt(s, pinned, in_try);
            }
            for s in else_body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::While { test, body } => {
            scan_expr(test, pinned, in_try);
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::ForClassic { test, update, body } => {
            scan_expr(test, pinned, in_try);
            for s in update {
                scan_stmt(s, pinned, in_try);
            }
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::ForOf { iterable, body, .. } => {
            scan_expr(iterable, pinned, in_try);
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::ForIn { object, body, .. } => {
            scan_expr(object, pinned, in_try);
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::DoWhile { body, test } => {
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
            scan_expr(test, pinned, in_try);
        }
        HirStmt::Switch { disc, cases } => {
            scan_expr(disc, pinned, in_try);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr(t, pinned, in_try);
                }
                for s in &c.body {
                    scan_stmt(s, pinned, in_try);
                }
            }
        }
        HirStmt::Try {
            block,
            catch,
            finally,
        } => {
            for s in block {
                scan_stmt(s, pinned, true);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    scan_stmt(s, pinned, in_try);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    scan_stmt(s, pinned, in_try);
                }
            }
        }
        HirStmt::Dispose { target, .. } => {
            pinned.insert(VarId::Local(*target));
        }
        _ => {}
    }
}

fn scan_expr(expr: &HirExpr, pinned: &mut FxHashSet<VarId>, in_try: bool) {
    match expr {
        HirExpr::NonNull(inner) => scan_expr(inner, pinned, in_try),
        HirExpr::TryOp(inner) => scan_expr(inner, pinned, in_try),
        HirExpr::TypeTest { value, .. } => scan_expr(value, pinned, in_try),
        HirExpr::Sequence(exprs) => {
            for e in exprs {
                scan_expr(e, pinned, in_try);
            }
        }
        HirExpr::Range { start, end, .. } => {
            scan_expr(start, pinned, in_try);
            scan_expr(end, pinned, in_try);
        }
        HirExpr::Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(e) = p {
                    scan_expr(e, pinned, in_try);
                }
            }
        }
        HirExpr::Assign { target, value } => {
            scan_expr(value, pinned, in_try);
            if in_try {
                match &**target {
                    HirAssignTarget::Var(HirBinding::Local(id)) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirAssignTarget::Var(HirBinding::Param(i)) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            scan_expr(lhs, pinned, in_try);
            scan_expr(rhs, pinned, in_try);
        }
        HirExpr::Unary { operand, .. } => scan_expr(operand, pinned, in_try),
        HirExpr::Call { callee, args, .. } => {
            scan_expr(callee, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::SelfCall { args, .. } => {
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::Member { object, .. } | HirExpr::GetFixedField { object, .. } => {
            scan_expr(object, pinned, in_try)
        }
        HirExpr::Index { object, index, .. } => {
            scan_expr(object, pinned, in_try);
            scan_expr(index, pinned, in_try);
        }
        HirExpr::MethodCall { recv, args, .. } => {
            scan_expr(recv, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::SuperCall { args } => {
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::SuperMethodCall { args, .. } => {
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::ExtensionCall { recv, args, .. } => {
            scan_expr(recv, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::Class(cls) => {
            if let Some(sup) = &cls.super_class {
                scan_expr(sup, pinned, in_try);
            }
            for (_, init) in &cls.static_fields {
                if let Some(init) = init {
                    scan_expr(init, pinned, in_try);
                }
            }
            for deco in &cls.decorators {
                scan_expr(deco, pinned, in_try);
            }
            for uv in &cls.ctor.upvalues {
                match uv {
                    HirUpvalueSrc::ParentLocal(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
            for deco in &cls.ctor.decorators {
                scan_expr(deco, pinned, in_try);
            }
            for m in &cls.methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
                for deco in &m.decorators {
                    scan_expr(deco, pinned, in_try);
                }
            }
            for m in &cls.static_methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
                for deco in &m.decorators {
                    scan_expr(deco, pinned, in_try);
                }
            }
            for a in &cls.getters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for a in &cls.setters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for b in &cls.static_blocks {
                for uv in &b.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
        }
        HirExpr::Enum(en) => {
            for (_, init) in &en.static_fields {
                if let Some(init) = init {
                    scan_expr(init, pinned, in_try);
                }
            }
            for v in &en.variants {
                for arg in &v.const_args {
                    scan_expr(arg, pinned, in_try);
                }
            }
            for uv in &en.ctor.upvalues {
                match uv {
                    HirUpvalueSrc::ParentLocal(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
            for m in &en.methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for m in &en.static_methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for a in &en.getters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for a in &en.setters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for b in &en.static_blocks {
                for uv in &b.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
        }
        HirExpr::Closure { upvalues, .. } => {
            for uv in upvalues {
                match uv {
                    HirUpvalueSrc::ParentLocal(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirExpr::Update { target, .. } => {
            if in_try {
                match &**target {
                    HirAssignTarget::Var(HirBinding::Local(id)) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirAssignTarget::Var(HirBinding::Param(i)) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirExpr::Array(els) => {
            for el in els {
                match el {
                    HirArrayEl::Expr(e) => scan_expr(e, pinned, in_try),
                    HirArrayEl::Spread(e) => scan_expr(e, pinned, in_try),
                    HirArrayEl::Hole => {}
                }
            }
        }
        HirExpr::Object { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(e) = key {
                            scan_expr(e, pinned, in_try);
                        }
                        scan_expr(value, pinned, in_try);
                    }
                    HirObjectProp::Method { key, upvalues, .. } => {
                        if let HirPropKey::Computed(e) = key {
                            scan_expr(e, pinned, in_try);
                        }
                        for uv in upvalues {
                            match uv {
                                HirUpvalueSrc::ParentLocal(id) => {
                                    pinned.insert(VarId::Local(*id));
                                }
                                HirUpvalueSrc::ParentParam(i) => {
                                    pinned.insert(VarId::Param(*i));
                                }
                                _ => {}
                            }
                        }
                    }
                    HirObjectProp::Spread(e) => scan_expr(e, pinned, in_try),
                }
            }
        }
        HirExpr::Match { subject, cases } => {
            scan_expr(subject, pinned, in_try);
            for c in cases {
                if let Some(g) = &c.guard {
                    scan_expr(g, pinned, in_try);
                }
                for s in &c.body {
                    scan_stmt(s, pinned, in_try);
                }
                if let Some(r) = &c.result {
                    scan_expr(r, pinned, in_try);
                }
            }
        }
        HirExpr::OptionalChain { object, property } => {
            scan_expr(object, pinned, in_try);
            match property {
                HirOptionalProperty::Index(e) => scan_expr(e, pinned, in_try),
                HirOptionalProperty::Call(args) => {
                    for a in args {
                        scan_expr(a, pinned, in_try);
                    }
                }
                HirOptionalProperty::MethodCall(_, args) => {
                    for a in args {
                        scan_expr(a, pinned, in_try);
                    }
                }
                _ => {}
            }
        }
        HirExpr::Await(e) => scan_expr(e, pinned, in_try),
        HirExpr::Spawn(e) => scan_expr(e, pinned, in_try),
        HirExpr::Yield(e) => scan_expr(e, pinned, in_try),
        HirExpr::TaggedTemplate { tag, template } => {
            scan_expr(tag, pinned, in_try);
            scan_expr(template, pinned, in_try);
        }
        HirExpr::IntrinsicCall { object, args, .. } => {
            scan_expr(object, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::NativeMethodCall { object, args, .. } => {
            scan_expr(object, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::ModuleSlot { object, .. } => scan_expr(object, pinned, in_try),
        _ => {}
    }
}

fn scan_pinned_vars(func: &HirFunction) -> FxHashSet<VarId> {
    let mut pinned = FxHashSet::default();
    for param in &func.params {
        if let Some(d) = &param.default {
            scan_expr(d, &mut pinned, false);
        }
    }
    for s in &func.body {
        scan_stmt(s, &mut pinned, false);
    }
    pinned
}

pub fn build_function(func: &HirFunction, module_funcs: &[HirFunction]) -> Result<SsaFunc> {
    let pinned = scan_pinned_vars(func);
    let mut b = Builder::new(pinned);

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
        b.emit_effect(InstKind::StoreGlobal {
            name: f.name.clone(),
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
    };
    simplify_phis(&mut ssa);
    Ok(ssa)
}

fn simplify_phis(func: &mut SsaFunc) {
    loop {
        let mut removed = None;
        'scan: for b in 0..func.blocks.len() {
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
            Some((phi, same)) => func.replace_all_uses(phi, same),
            None => break,
        }
    }
}

fn trivial_operand(func: &SsaFunc, block: BlockId, pos: usize, phi: Value) -> Option<Value> {
    let mut same: Option<Value> = None;
    for &pred in &func.blocks[block.0 as usize].preds {
        let op = edge_arg(func, pred, block, pos);
        if op == phi || same == Some(op) {
            continue;
        }
        if same.is_some() {
            return None;
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
