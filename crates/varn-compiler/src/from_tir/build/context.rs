use crate::hir::LocalId;
use crate::ssa::ir::{Block, BlockId, Value, ValueDef, VarId};
use crate::OptError;
use rustc_hash::{FxHashMap, FxHashSet};
use varn_tir::{BackendTy, TirModule, TirStmt};

pub(crate) type Result<T> = std::result::Result<T, OptError>;

#[derive(Clone, Copy)]
pub(crate) struct LoopCtx {
    pub(super) continue_target: BlockId,
    pub(super) break_target: BlockId,
    pub(super) try_depth: usize,
}

pub(crate) struct Builder<'m> {
    pub(super) tir: &'m TirModule,
    pub(super) self_fn: Option<varn_tir::FnId>,
    pub(super) defaulted: Vec<Option<varn_tir::BackendTy>>,
    pub(super) line_starts: Vec<u32>,
    pub(super) cur_line: u32,

    pub(super) blocks: Vec<Block>,
    pub(super) values: Vec<ValueDef>,
    pub(super) sealed: Vec<bool>,
    pub(super) terminated: Vec<bool>,
    pub(super) defs: FxHashMap<(VarId, BlockId), Value>,
    pub(super) var_ty: FxHashMap<VarId, BackendTy>,
    pub(super) incomplete_phis: FxHashMap<BlockId, Vec<(VarId, Value)>>,
    pub(super) loops: Vec<LoopCtx>,
    pub(super) try_depth: usize,
    pub(super) pinned: FxHashSet<VarId>,
    pub(super) next_synthetic: u32,
    pub(super) current: BlockId,
    pub(super) inlining: Vec<InlineFrame>,
    pub(super) locals_bt: Vec<BackendTy>,
    pub(super) return_bt: Option<BackendTy>,
    pub(super) inline_serial: u32,
}

pub(super) struct InlineFrame {
    pub(super) func: varn_tir::FnId,
    pub(super) params: Vec<Value>,
    pub(super) this: Option<Value>,
    pub(super) body: Option<InlineBody>,
}

pub(super) struct InlineBody {
    pub(super) func: varn_tir::FnId,
    pub(super) base: u32,
    pub(super) param_base: u32,
    pub(super) exit: BlockId,
    pub(super) result: VarId,
    pub(super) ret_ty: BackendTy,
}

impl<'m> Builder<'m> {
    pub(super) fn with_pinned(tir: &'m TirModule, pinned: FxHashSet<VarId>, lines: &[u32]) -> Self {
        let mut b = Builder {
            tir,
            self_fn: None,
            defaulted: Vec::new(),
            line_starts: lines.to_vec(),
            cur_line: 1,
            blocks: Vec::new(),
            values: Vec::new(),
            sealed: Vec::new(),
            terminated: Vec::new(),
            defs: FxHashMap::default(),
            var_ty: FxHashMap::default(),
            incomplete_phis: FxHashMap::default(),
            loops: Vec::new(),
            try_depth: 0,
            pinned,
            next_synthetic: 0,
            current: BlockId(0),
            inlining: Vec::new(),
            locals_bt: Vec::new(),
            return_bt: None,
            inline_serial: 0,
        };
        let entry = b.new_block();
        b.sealed[entry.0 as usize] = true;
        b.current = entry;
        b
    }

    pub(super) fn line_of(&self, offset: u32) -> u32 {
        self.line_starts.partition_point(|&s| s <= offset) as u32
    }

    pub(super) fn set_line_at(&mut self, offset: u32) {
        self.cur_line = self.line_of(offset);
    }

    pub(super) fn loop_body_pinned(&self, body: &[TirStmt]) -> Vec<VarId> {
        fn walk(stmts: &[TirStmt], pinned: &FxHashSet<VarId>, base: u32, out: &mut Vec<VarId>) {
            for s in stmts {
                for nested in crate::from_tir::tir_children::seq_bodies(s) {
                    walk(nested, pinned, base, out);
                }
                match s {
                    TirStmt::Let { local, .. } => {
                        let v = VarId::Local(LocalId(local.0 + base));
                        if pinned.contains(&v) && !out.contains(&v) {
                            out.push(v);
                        }
                    }
                    TirStmt::If {
                        then_body,
                        else_body,
                        ..
                    } => {
                        walk(then_body, pinned, base, out);
                        walk(else_body, pinned, base, out);
                    }
                    TirStmt::Try {
                        body, catch_body, ..
                    } => {
                        walk(body, pinned, base, out);
                        walk(catch_body, pinned, base, out);
                    }
                    TirStmt::Expr(_)
                    | TirStmt::Return(_)
                    | TirStmt::Loop { .. }
                    | TirStmt::Break
                    | TirStmt::Continue
                    | TirStmt::Throw(_)
                    | TirStmt::BuildClass(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        let base = self
            .inlining
            .last()
            .and_then(|f| f.body.as_ref())
            .map_or(0, |b| b.base);
        walk(body, &self.pinned, base, &mut out);
        out
    }
}
