use super::ir::{BlockId, Inst, InstKind, SsaFunc, Terminator};
use crate::OptError;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use vn_core::OpCode;
use vn_tir::BackendTy;
use vn_types::chunk::{Chunk, FeedbackVector, FunctionProto, PolyICSlot};

mod effects;
mod immediates;
mod order;
mod phi_edges;
mod register_meta;
mod regs;
use register_meta::derive_register_meta;
pub(crate) use register_meta::slot_kind_of;
pub(crate) use regs::var_reg;
mod terminator;
mod values;

use immediates::Immediates;
use order::emission_order;

type Result<T> = std::result::Result<T, OptError>;

pub struct FnMeta {
    pub name: Arc<str>,
    pub start_line: u32,
    pub nparams: usize,
    pub param_kinds: Vec<vn_types::register_meta::SlotKind>,
    pub return_kind: vn_types::register_meta::SlotKind,
    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub upvalue_count: u32,
}

pub(crate) fn emit_function_meta(
    mut ssa: SsaFunc,
    f: &FnMeta,
    source_file: Arc<str>,
    scope: &crate::from_tir::compile::ModuleScope,
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
    } = regs::assign_registers(&ssa, nparams).map_err(|e| e.in_function(&f.name))?;
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
    chunk.inline_scopes = std::mem::take(&mut ssa.inline_scopes);
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

    let value_tys: Vec<BackendTy> = ssa.values.iter().map(|v| v.ty).collect();

    let mut pos_of = vec![usize::MAX; n];
    for (i, &b) in order.iter().enumerate() {
        pos_of[b] = i;
    }

    for (i, &b) in order.iter().enumerate() {
        block_offset[b] = chunk.code.len();
        let insts = std::mem::take(&mut ssa.blocks[b].insts);
        for (idx, inst) in insts.iter().enumerate() {
            inst_off[b][idx] = chunk.code.len();
            chunk.scope_cursor = inst.scope;
            emit_inst(
                &mut chunk,
                inst,
                &value_tys,
                &reg,
                scratch,
                call_base,
                ic.emit_slot(b, idx),
                &source_file,
                nparams,
                &mut fixups,
                &imms,
                &mut closure_consts[b][idx],
                scope,
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
        chunk.scope_cursor = ssa.blocks[b].term_scope;
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

    chunk.scope_cursor = 0;
    let ends_in_exit = order.last().is_some_and(|&b| {
        matches!(
            ssa.blocks[b].term,
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable
        )
    });
    if !ends_in_exit {
        chunk.write(Chunk::pack_op(OpCode::LoadNull, null_reg), fn_line);
        chunk.emit1(OpCode::Return, Chunk::pack(0, null_reg), fn_line);
    }

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

    if chunk.constants_overflowed {
        return Err(OptError::Internal(format!(
            "ssa-emit: '{}' needs more than {} constants",
            f.name,
            vn_types::chunk::MAX_CONSTANTS
        )));
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
        Ok(p) => vn_types::ssa::PortableSsa::Available(std::sync::Arc::new(p)),
        Err(why) => vn_types::ssa::PortableSsa::Unavailable(Arc::from(why)),
    };

    let suspend_live = if f.is_async || f.is_generator {
        suspend_live_table(&ssa, &reg, &inst_next, nparams)
    } else {
        Vec::new()
    };

    Ok(FunctionProto {
        name: Some(Arc::from(f.name.as_ref())),
        arity: 1 + nparams,
        export_names: Vec::new(),
        import_layouts: Vec::new(),
        register_count,
        has_rest: f.has_rest,
        checks_params: false,
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
        ic_cache: Rc::new(RefCell::new(
            (0..ic.count()).map(|_| PolyICSlot::new()).collect(),
        )),
        feedback: Rc::new(RefCell::new(FeedbackVector::new(ic.count() as usize))),
        frame_layout: Default::default(),
        static_closure_val: Cell::new(0),
        jit_entry_count: Cell::new(0),
        backedge_count: Cell::new(0),
        jit_osr_entries: RefCell::new(Vec::new()),
        ssa: ssa_proto,
        suspend_live,
        param_contract: None,
        param_types: std::cell::OnceCell::new(),
    })
}

fn suspend_live_table(
    ssa: &SsaFunc,
    reg: &[u8],
    inst_next: &[Vec<usize>],
    nparams: usize,
) -> Vec<vn_types::chunk::SuspendLive> {
    super::suspend::suspend_live_regs(ssa, reg, inst_next, nparams)
        .into_iter()
        .map(|(resume_ip, regs)| vn_types::chunk::SuspendLive { resume_ip, regs })
        .collect()
}

fn emit_inst(
    chunk: &mut Chunk,
    inst: &Inst,

    value_tys: &[BackendTy],
    reg: &[u8],
    scratch: u8,
    call_base: u8,

    ic_slot: Option<u8>,
    source_file: &Arc<str>,
    nparams: usize,
    fixups: &mut Vec<(usize, BlockId)>,
    imms: &Immediates,

    closure_const: &mut Option<u16>,
    scope: &crate::from_tir::compile::ModuleScope,
) -> Result<()> {
    if let (Some(d), InstKind::ConstInt(_)) = (inst.dest, &inst.kind)
        && imms.is_elided(d)
    {
        return Ok(());
    }
    if let Some(form) = immediates::immediate_operand(&inst.kind, &imms.imm) {
        let dest = reg[inst.dest.expect("binary defines a value").0 as usize];
        chunk.emit_rrr(
            form.opcode,
            dest,
            reg[form.other.0 as usize],
            form.value as u8,
            inst.line,
        );
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
        scope,
    )
}
