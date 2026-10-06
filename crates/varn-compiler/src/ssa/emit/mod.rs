use super::ir::{BlockId, Inst, InstKind, SsaFunc, Terminator};
use crate::OptError;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, PolyICSlot};

mod effects;
mod immediates;
mod phi_edges;
mod register_meta;
mod regs;
use register_meta::derive_register_meta;
pub(crate) use register_meta::slot_kind_of;
pub(crate) use regs::var_reg;
mod terminator;
mod values;

use immediates::Immediates;

type Result<T> = std::result::Result<T, OptError>;

pub struct FnMeta {
    pub name: Arc<str>,
    pub start_line: u32,
    pub nparams: usize,
    pub param_kinds: Vec<varn_types::register_meta::SlotKind>,
    pub return_kind: varn_types::register_meta::SlotKind,
    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub upvalue_count: u32,
}

pub fn emit_function_meta(
    mut ssa: SsaFunc,
    f: &FnMeta,
    source_file: Arc<str>,
) -> Result<FunctionProto> {
    phi_edges::split_phi_edges(&mut ssa);

    let fn_line = if f.start_line > 0 { f.start_line } else { 1 };
    let nparams = f.nparams;
    let regs::Assignment {
        reg,
        scratch,
        null_reg,
        call_base,
        register_count,
        liveness,
    } = regs::assign_registers(&ssa, nparams)?;
    let param_kinds = f.param_kinds.clone();
    let register_meta = derive_register_meta(&ssa, &reg, register_count, &param_kinds);
    let return_kind = f.return_kind;

    let order = emission_order(&ssa);
    let ic = super::ic::IcSlots::number(&ssa, &order)?;

    let mut closure_consts: Vec<Vec<Option<u16>>> = ssa
        .blocks
        .iter()
        .map(|b| vec![None; b.insts.len()])
        .collect();

    let n = ssa.blocks.len();
    let mut chunk = Chunk::new();
    chunk.source_file = Arc::from(source_file.as_ref());
    let mut block_offset = vec![usize::MAX; n];

    let mut inst_off: Vec<Vec<usize>> = ssa
        .blocks
        .iter()
        .map(|b| vec![usize::MAX; b.insts.len()])
        .collect();
    let mut inst_next: Vec<Vec<usize>> = ssa
        .blocks
        .iter()
        .map(|b| vec![usize::MAX; b.insts.len()])
        .collect();

    let mut fixups: Vec<(usize, BlockId)> = Vec::new();

    let imms = immediates::plan_immediates(&ssa);

    let value_tys: Vec<crate::hir::HirType> = ssa.values.iter().map(|v| v.ty).collect();

    let mut pos_of = vec![usize::MAX; n];
    for (i, &b) in order.iter().enumerate() {
        pos_of[b] = i;
    }

    for (i, &b) in order.iter().enumerate() {
        block_offset[b] = chunk.code.len();
        let insts = std::mem::take(&mut ssa.blocks[b].insts);
        for (idx, inst) in insts.iter().enumerate() {
            inst_off[b][idx] = chunk.code.len();
            emit_inst(
                &mut chunk,
                inst,
                &value_tys,
                &reg,
                scratch,
                call_base,
                ic.of(b, idx),
                &source_file,
                nparams,
                &mut fixups,
                &imms,
                &mut closure_consts[b][idx],
            )?;
            inst_next[b][idx] = chunk.code.len();
        }
        ssa.blocks[b].insts = insts;
        let term = ssa.blocks[b].term.clone();
        let term_line = if ssa.blocks[b].term_line > 0 {
            ssa.blocks[b].term_line
        } else {
            fn_line
        };
        terminator::emit_terminator(
            &mut chunk,
            &ssa,
            &reg,
            i,
            &pos_of,
            &term,
            term_line,
            null_reg,
            scratch,
            &block_offset,
            &mut fixups,
        )?;
    }

    chunk.write(Chunk::pack_op(OpCode::LoadNull, null_reg), fn_line);
    chunk.emit1(OpCode::Return, Chunk::pack(0, null_reg), fn_line);

    for (pos, target) in fixups {
        let target_off = block_offset[target.0 as usize];
        let rel = target_off as isize - pos as isize - 2;
        if rel < 0 {
            return Err(OptError::Unsupported(
                "ssa-emit: forward jump resolved backward",
            ));
        }
        let off = rel as u32;
        chunk.code[pos] = (off >> 16) as u16;
        chunk.code[pos + 1] = (off & 0xFFFF) as u16;
    }

    let emitted = super::portable::Emitted {
        reg: &reg,
        register_count,
        nparams,
        ic: &ic,
        closure_consts: &closure_consts,
        block_offset: &block_offset,
        inst_off: &inst_off,
        inst_next: &inst_next,
        liveness: &liveness,
    };
    let ssa_proto = match super::portable::project(&ssa, &emitted, f.has_this, &f.name) {
        Ok(p) => varn_types::ssa::PortableSsa::Available(std::sync::Arc::new(p)),
        Err(why) => varn_types::ssa::PortableSsa::Unavailable(Arc::from(why)),
    };

    let suspend_live = if f.is_async || f.is_generator {
        suspend_live_table(&ssa, &reg, &inst_next)
    } else {
        Vec::new()
    };

    Ok(FunctionProto {
        name: Some(Arc::from(f.name.as_ref())),
        arity: 1 + nparams,
        export_names: Vec::new(),
        register_count,
        has_rest: f.has_rest,
        is_async: f.is_async,
        is_generator: f.is_generator,
        has_this: f.has_this,
        upvalue_count: f.upvalue_count as usize,
        cache_count: ic.count() as usize,
        chunk,
        required_caps: Vec::new(),
        state_size: 0,

        global_count: 0,
        register_meta,
        exception_table: Vec::new(),
        param_kinds,
        return_kind,
        resolved_shapes: RefCell::new(Vec::new()),
        jit_entry: Cell::new(0),
        jit_native: Cell::new(0),
        jit_native_sig: Cell::new(0),
        jit_code: RefCell::new(None),
        jit_failed: Cell::new(false),
        jit_epoch: Cell::new(0),
        backedge_memo: Cell::new(0),
        resume_memo: Cell::new(0),
        ic_cache: Rc::new(RefCell::new(
            (0..ic.count()).map(|_| PolyICSlot::new()).collect(),
        )),
        feedback: Rc::new(RefCell::new(FeedbackVector::new(ic.count() as usize))),
        frame_layout: Default::default(),
        static_closure_val: Cell::new(0),
        jit_entry_count: Cell::new(0),
        backedge_count: Cell::new(0),
        jit_osr_entry: Cell::new(None),
        jit_osr_epoch: Cell::new(0),
        jit_osr_ip: Cell::new(0),
        jit_osr_code: RefCell::new(None),
        jit_osr_failed: Cell::new(false),
        ssa: ssa_proto,
        suspend_live,
    })
}

fn suspend_live_table(
    ssa: &SsaFunc,
    reg: &[u8],
    inst_next: &[Vec<usize>],
) -> Vec<varn_types::chunk::SuspendLive> {
    super::suspend::suspend_live_regs(ssa, reg, inst_next)
        .into_iter()
        .map(|(resume_ip, regs)| varn_types::chunk::SuspendLive { resume_ip, regs })
        .collect()
}

fn emission_order(ssa: &SsaFunc) -> Vec<usize> {
    let n = ssa.blocks.len();

    let succs = |b: usize| -> Vec<usize> {
        let mut s = match &ssa.blocks[b].term {
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
            Terminator::Jump { target, .. } => vec![target.0 as usize],
            Terminator::Branch {
                then_blk, else_blk, ..
            } => {
                vec![else_blk.0 as usize, then_blk.0 as usize]
            }
        };
        for inst in &ssa.blocks[b].insts {
            if let InstKind::Try { handler } = &inst.kind {
                s.push(handler.0 as usize);
            }
        }
        s
    };
    let mut visited = vec![false; n];
    let mut post: Vec<usize> = Vec::with_capacity(n);
    let mut stack: Vec<(usize, u8)> = Vec::with_capacity(n);
    let entry = ssa.entry.0 as usize;
    visited[entry] = true;
    stack.push((entry, 0));
    while let Some(top) = stack.last_mut() {
        let (b, stage) = *top;
        top.1 += 1;
        let succ = succs(b).get(stage as usize).copied();
        match succ {
            Some(s) => {
                if !visited[s] {
                    visited[s] = true;
                    stack.push((s, 0));
                }
            }
            None => {
                post.push(b);
                stack.pop();
            }
        }
    }
    let mut order: Vec<usize> = post.into_iter().rev().collect();
    for (b, seen) in visited.iter().enumerate() {
        if !seen {
            order.push(b);
        }
    }
    order
}

#[allow(clippy::too_many_arguments)]
fn emit_inst(
    chunk: &mut Chunk,
    inst: &Inst,

    value_tys: &[crate::hir::HirType],
    reg: &[u8],
    scratch: u8,
    call_base: u8,

    ic_slot: Option<u8>,
    source_file: &Arc<str>,
    nparams: usize,
    fixups: &mut Vec<(usize, BlockId)>,
    imms: &Immediates,

    closure_const: &mut Option<u16>,
) -> Result<()> {
    if let (Some(d), InstKind::ConstInt(_)) = (inst.dest, &inst.kind) {
        if imms.is_elided(d) {
            return Ok(());
        }
    }
    if let Some((_, other, value)) = immediates::immediate_operand(&inst.kind, &imms.imm) {
        let dest = reg[inst.dest.expect("binary defines a value").0 as usize];

        let opcode = match &inst.kind {
            InstKind::Binary {
                op: crate::hir::HirBinOp::Sub,
                ..
            } => OpCode::SubImm,
            _ => OpCode::AddImm,
        };

        chunk.emit_rrr(opcode, dest, reg[other.0 as usize], value as u8, inst.line);
        return Ok(());
    }

    if effects::emit_effect(chunk, inst, reg, ic_slot, nparams)? {
        return Ok(());
    }

    let d = match inst.dest {
        Some(dest) => reg[dest.0 as usize],

        None if crate::passes::dce::dest_droppable(&inst.kind) => scratch,
        None => return Ok(()),
    };
    values::emit_value(
        chunk,
        inst,
        d,
        value_tys,
        reg,
        scratch,
        call_base,
        ic_slot,
        source_file,
        nparams,
        fixups,
        closure_const,
    )
}
