//! Project the compiler's internal SSA onto the portable, serializable
//! [`varn_types::ssa::SsaProto`] that rides inside a `FunctionProto`.
//!
//! This is a *lossy by design* projection: it carries only the scalar/arith
//! family the JIT can lower directly, and returns `None` for a body that uses
//! anything else. A `None` is not an error — it is the fallback contract: the
//! function keeps its bytecode lowering, which is the pre-existing path. The
//! projection is deliberately conservative because a wrong SSA (a value whose
//! class disagrees with the bytecode) would be a miscompile, while a missing
//! SSA is only a missed optimization (Ley 10: no gain without correctness).

use std::sync::Arc;

use crate::hir::HirType;
use crate::ssa::ir::{InstKind, SsaFunc, Terminator, VarId};
use crate::ssa::liveness::Liveness;
use rustc_hash::FxHashSet;
use varn_types::ssa::{SsaBlock, SsaProto, SsaTerm, SsaValue};

mod inst;
mod ops;

use inst::project_inst;

/// What emitting the bytecode decided that the portable SSA must agree with.
pub(crate) struct Emitted<'a> {
    /// Each value's register.
    pub reg: &'a [u8],
    pub register_count: u16,
    pub nparams: usize,
    /// Each site's inline-cache slot.
    pub ic: &'a crate::ssa::ic::IcSlots,
    /// The function constant each `MakeClosure` was emitted with.
    pub closure_consts: &'a [Vec<Option<u16>>],
    /// The bytecode offset each block was emitted at.
    pub block_offset: &'a [usize],
}

/// The captured variables of a function, numbered in first-use order: the
/// index the closure ops name one by, and its frame register.
struct Captured {
    vars: Vec<VarId>,
    nparams: usize,
}

impl Captured {
    fn index(&mut self, var: VarId) -> u32 {
        match self.vars.iter().position(|v| *v == var) {
            Some(i) => i as u32,
            None => {
                self.vars.push(var);
                (self.vars.len() - 1) as u32
            }
        }
    }

    fn regs(&self) -> Vec<u32> {
        self.vars
            .iter()
            .map(|v| u32::from(crate::ssa::emit::var_reg(*v, self.nparams)))
            .collect()
    }
}

/// Build the portable SSA for `ssa` (phi-split, register-assigned, and just
/// emitted as bytecode). Returns why not when any instruction or terminator
/// is outside the projected family.
pub(crate) fn project(
    ssa: &SsaFunc,
    emitted: &Emitted<'_>,
    has_this: bool,
    name: &Arc<str>,
) -> Result<SsaProto, String> {
    let value_tys: Vec<HirType> = ssa.values.iter().map(|v| v.ty).collect();

    // Module-relative global slot each value was loaded from, so a `Call` can
    // tell the linker what to resolve. Only `LoadGlobalIdx` has this provenance;
    // a callee reached any other way stays `None` and the JIT declines it.
    let mut global_of: Vec<Option<u32>> = vec![None; ssa.values.len()];
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::LoadGlobalIdx(slot)) = (inst.dest, &inst.kind) {
                global_of[d.0 as usize] = Some(*slot);
            }
        }
    }

    // What each landing pad reads, for the `Try`s that open one.
    let has_try = ssa.blocks.iter().any(|b| {
        b.insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::Try { .. }))
    });
    let liveness = has_try.then(|| Liveness::analyze(ssa));

    let mut captured = Captured {
        vars: Vec::new(),
        nparams: emitted.nparams,
    };
    let mut blocks = Vec::with_capacity(ssa.blocks.len());
    for (b, block) in ssa.blocks.iter().enumerate() {
        let mut insts = Vec::with_capacity(block.insts.len());
        for (i, inst) in block.insts.iter().enumerate() {
            let site = Site {
                ic_slot: emitted.ic.of(b, i),
                closure_const: emitted
                    .closure_consts
                    .get(b)
                    .and_then(|c| c.get(i))
                    .copied()
                    .flatten(),
                landing: match (&inst.kind, &liveness) {
                    (InstKind::Try { handler }, Some(lv)) => {
                        let h = handler.0 as usize;
                        let off = emitted.block_offset.get(h).copied();
                        off.and_then(|o| u32::try_from(o).ok())
                            .map(|o| (o, &lv.live_in[h]))
                    }
                    _ => None,
                },
            };
            let projected = project_inst(inst, &value_tys, &global_of, site, &mut captured)
                .ok_or_else(|| why_not(&inst.kind, &value_tys))?;
            insts.push(projected);
        }
        blocks.push(SsaBlock {
            params: block.params.iter().map(|v| v.0).collect(),
            insts,
            term: project_term(&block.term),
        });
    }

    let values = ssa
        .values
        .iter()
        .map(|v| SsaValue {
            ty: super::emit::slot_kind_of(v.ty),
        })
        .collect();

    Ok(SsaProto {
        name: name.as_ref().into(),
        nparams: ssa
            .blocks
            .get(ssa.entry.0 as usize)
            .map(|b| b.params.len() as u32)
            .unwrap_or(0),
        entry: ssa.entry.0,
        blocks,
        values,
        regs: emitted.reg.iter().map(|&r| r as u32).collect(),
        register_count: emitted.register_count,
        has_this,
        captured: captured.regs(),
    })
}

/// What the bytecode baked for one instruction.
#[derive(Clone, Copy)]
struct Site<'a> {
    /// Its inline-cache slot (`ssa::ic`).
    ic_slot: Option<u8>,
    /// The function constant of a `MakeClosure`.
    closure_const: Option<u16>,
    /// A `Try`'s landing pad: its bytecode offset and the values live into it.
    landing: Option<(u32, &'a FxHashSet<u32>)>,
}

fn project_term(term: &Terminator) -> SsaTerm {
    match term {
        Terminator::Return(v) => SsaTerm::Return(v.map(|v| v.0)),
        Terminator::Throw(v) => SsaTerm::Throw(v.0),
        Terminator::Jump { target, args } => SsaTerm::Jump {
            target: target.0,
            args: args.iter().map(|v| v.0).collect(),
        },
        Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => SsaTerm::Branch {
            cond: cond.0,
            then_blk: then_blk.0,
            then_args: then_args.iter().map(|v| v.0).collect(),
            else_blk: else_blk.0,
            else_args: else_args.iter().map(|v| v.0).collect(),
        },
        Terminator::Unreachable => SsaTerm::Unreachable,
    }
}

/// Why an instruction has no portable form: its name, and for an operator
/// the operand types it was asked for.
fn why_not(kind: &InstKind, value_tys: &[HirType]) -> String {
    let ty = |v: &crate::ssa::ir::Value| value_tys.get(v.0 as usize).copied();
    match kind {
        InstKind::Binary { op, lhs, rhs, .. } => {
            format!(
                "no portable form for {op:?} on {:?} and {:?}",
                ty(lhs),
                ty(rhs)
            )
        }
        InstKind::Unary { op, operand, .. } => {
            format!("no portable form for {op:?} on {:?}", ty(operand))
        }
        other => {
            let full = format!("{other:?}");
            let end = full.find([' ', '(', '{']).unwrap_or(full.len());
            format!("no portable form for {}", &full[..end])
        }
    }
}
