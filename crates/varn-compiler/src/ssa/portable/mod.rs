use std::sync::Arc;

use crate::hir::HirType;
use crate::ssa::ir::{InstKind, SsaFunc, Terminator, VarId};
use crate::ssa::liveness::Liveness;
use rustc_hash::FxHashSet;
use varn_types::ssa::{SsaBlock, SsaLoopHeader, SsaProto, SsaTerm, SsaValue};

mod inst;
mod ops;

use inst::project_inst;

pub(crate) struct Emitted<'a> {
    pub reg: &'a [u8],
    pub register_count: u16,
    pub nparams: usize,

    pub ic: &'a crate::ssa::ic::IcSlots,

    pub closure_consts: &'a [Vec<Option<u16>>],

    pub block_offset: &'a [usize],

    pub inst_off: &'a [Vec<usize>],

    pub inst_next: &'a [Vec<usize>],

    pub liveness: &'a Liveness,
}

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

pub(crate) fn project(
    ssa: &SsaFunc,
    emitted: &Emitted<'_>,
    has_this: bool,
    name: &Arc<str>,
) -> Result<SsaProto, String> {
    let value_tys: Vec<HirType> = ssa.values.iter().map(|v| v.ty).collect();

    let lv = emitted.liveness;
    let handlers = crate::ssa::suspend::try_handlers(ssa);

    let mut captured = Captured {
        vars: Vec::new(),
        nparams: emitted.nparams,
    };
    let mut blocks = Vec::with_capacity(ssa.blocks.len());
    for (b, block) in ssa.blocks.iter().enumerate() {
        let mut insts = Vec::with_capacity(block.insts.len());
        for (i, inst) in block.insts.iter().enumerate() {
            let own = emitted
                .inst_off
                .get(b)
                .and_then(|v| v.get(i))
                .copied()
                .unwrap_or(usize::MAX);
            let next = emitted
                .inst_next
                .get(b)
                .and_then(|v| v.get(i))
                .copied()
                .unwrap_or(usize::MAX);
            let site = Site {
                ic_slot: emitted.ic.of(b, i),
                closure_const: emitted
                    .closure_consts
                    .get(b)
                    .and_then(|c| c.get(i))
                    .copied()
                    .flatten(),
                landing: match &inst.kind {
                    InstKind::Try { handler } => {
                        let h = handler.0 as usize;
                        let off = emitted.block_offset.get(h).copied();
                        off.and_then(|o| u32::try_from(o).ok())
                            .map(|o| (o, &lv.live_in[h]))
                    }
                    _ => None,
                },
                own_ip: u32::try_from(own).unwrap_or(u32::MAX),
                next_ip: u32::try_from(next).unwrap_or(u32::MAX),
                resume_live: match &inst.kind {
                    InstKind::LoadModule { .. }
                    | InstKind::Await { .. }
                    | InstKind::Yield { .. } => {
                        crate::ssa::suspend::resume_live(ssa, lv, &handlers, b, i)
                            .into_iter()
                            .map(|v| v.0)
                            .collect()
                    }
                    _ => Vec::new(),
                },
            };
            let projected = project_inst(inst, &value_tys, site, &mut captured)
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
        loop_headers: loop_headers(ssa, emitted),
    })
}

fn loop_headers(ssa: &SsaFunc, emitted: &Emitted<'_>) -> Vec<SsaLoopHeader> {
    let off = |b: usize| emitted.block_offset.get(b).copied().unwrap_or(usize::MAX);
    let mut is_header = vec![false; ssa.blocks.len()];
    for (b, block) in ssa.blocks.iter().enumerate() {
        let targets = match &block.term {
            Terminator::Jump { target, .. } => vec![target.0 as usize],
            Terminator::Branch {
                then_blk, else_blk, ..
            } => vec![then_blk.0 as usize, else_blk.0 as usize],
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
        };
        for t in targets {
            if off(b) != usize::MAX && off(t) <= off(b) {
                is_header[t] = true;
            }
        }
    }
    is_header
        .iter()
        .enumerate()
        .filter(|(_, h)| **h)
        .filter_map(|(b, _)| {
            let ip = u32::try_from(off(b)).ok()?;
            let mut live: Vec<u32> = emitted.liveness.live_in[b].iter().copied().collect();
            live.sort_unstable();
            Some(SsaLoopHeader {
                block: b as u32,
                ip,
                live,
            })
        })
        .collect()
}

struct Site<'a> {
    ic_slot: Option<u8>,

    closure_const: Option<u16>,

    landing: Option<(u32, &'a FxHashSet<u32>)>,

    own_ip: u32,

    next_ip: u32,

    resume_live: Vec<u32>,
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
