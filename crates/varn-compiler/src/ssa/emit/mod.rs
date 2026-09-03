//! Bytecode emission: SSA -> `FunctionProto`.
//!
//! `emit_function` is the driver. The steps it composes live in their own
//! modules: critical-edge splitting (`phi_edges`), register assignment
//! (`regs`), immediate folding (`immediates`), the two halves of per-instruction
//! emission (`effects`, `values`), and block terminators (`terminator`).

use super::ir::{BlockId, Inst, InstKind, SsaFunc, Terminator};
use crate::hir::HirFunction;
use crate::OptError;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, PolyICSlot};

mod effects;
mod immediates;
mod phi_edges;
mod regs;
mod terminator;
mod values;

use immediates::Immediates;

type Result<T> = std::result::Result<T, OptError>;

pub fn emit_function(
    mut ssa: SsaFunc,
    f: &HirFunction,
    source_file: Rc<str>,
) -> Result<FunctionProto> {
    phi_edges::split_phi_edges(&mut ssa);

    let fn_line = if f.start_line > 0 { f.start_line } else { 1 };
    let nparams = f.params.len();
    let (reg, scratch, null_reg, call_base, register_count) =
        regs::assign_registers(&ssa, nparams)?;
    let param_kinds: Vec<_> = f.params.iter().map(|p| slot_kind_of(p.ty)).collect();
    let register_meta = derive_register_meta(&ssa, &reg, register_count, &param_kinds);
    let return_kind = slot_kind_of(f.return_ty);

    let n = ssa.blocks.len();
    let mut chunk = Chunk::new();
    chunk.source_file = source_file.clone();
    let mut block_offset = vec![usize::MAX; n];

    let mut fixups: Vec<(usize, BlockId)> = Vec::new();

    let mut cache_count: u16 = 0;

    let imms = immediates::plan_immediates(&ssa);

    let value_tys: Vec<crate::hir::HirType> = ssa.values.iter().map(|v| v.ty).collect();

    let order = emission_order(&ssa);
    let mut pos_of = vec![usize::MAX; n];
    for (i, &b) in order.iter().enumerate() {
        pos_of[b] = i;
    }

    for (i, &b) in order.iter().enumerate() {
        block_offset[b] = chunk.code.len();
        let insts = std::mem::take(&mut ssa.blocks[b].insts);
        for inst in &insts {
            emit_inst(
                &mut chunk,
                inst,
                &value_tys,
                &reg,
                scratch,
                call_base,
                &mut cache_count,
                &source_file,
                nparams,
                &mut fixups,
                &imms,
            )?;
        }
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

    Ok(FunctionProto {
        name: Some(f.name.clone()),
        arity: 1 + nparams,
        export_names: Vec::new(),
        register_count,
        has_rest: f.has_rest,
        is_async: f.is_async,
        is_generator: f.is_generator,
        has_this: f.has_this,
        upvalue_count: f.upvalue_count as usize,
        cache_count: cache_count as usize,
        chunk,
        required_caps: Vec::new(),
        state_size: 0,
        register_meta,
        exception_table: Vec::new(),
        param_kinds,
        return_kind,
        resolved_shapes: RefCell::new(Vec::new()),
        jit_entry: Cell::new(None),
        globals_id: Cell::new(0),
        clif_raw: Cell::new(0),
        jit_code: RefCell::new(None),
        jit_failed: Cell::new(false),
        jit_epoch: Cell::new(0),
        jit_serial: Cell::new(0),
        backedge_memo: Cell::new(0),
        ic_cache: Rc::new(RefCell::new(
            (0..cache_count).map(|_| PolyICSlot::new()).collect(),
        )),
        feedback: Rc::new(RefCell::new(FeedbackVector::new(cache_count as usize))),
        static_closure_val: Cell::new(0),
        jit_entry_count: Cell::new(0),
        backedge_count: Cell::new(0),
        jit_osr_entry: Cell::new(None),
        jit_osr_epoch: Cell::new(0),
        jit_osr_ip: Cell::new(0),
        jit_osr_code: RefCell::new(None),
        jit_osr_failed: Cell::new(false),
    })
}

/// Per-register slot kinds from checker-proven SSA value types: the meet of
/// every value a register hosts, plus `Dynamic` for caller-written slots
/// (callee, params, `this`) and helper registers that host no SSA value
/// (scratch, call staging, null). Replaces the old opcode-walking
/// re-inference in `varn-regalloc`, which guessed back what the checker
/// already proved. `regalloc_post` re-permutes this when it coalesces.
fn derive_register_meta(
    ssa: &SsaFunc,
    reg: &[u8],
    register_count: u16,
    param_kinds: &[varn_types::register_meta::SlotKind],
) -> Vec<varn_types::register_meta::RegisterMeta> {
    use varn_types::register_meta::{RegisterMeta, SlotKind};
    let n = register_count as usize;
    let mut kinds: Vec<Option<SlotKind>> = vec![None; n];
    // r0 is the callee/`this` staging slot the caller writes; keep it Dynamic
    // (it may host a heap ref during call staging, and the GC must flush it).
    // Params (at r1+i, matching the JIT's entry contract) carry their declared
    // kind: an immediate param (int/float/bool) is a proven fact the backend
    // uses — it skips the GC flush and, for float, routes to native f64 — while
    // a heap-ref param stays non-immediate and still flushes. The value meet
    // below downgrades any param register the allocator reuses for a
    // differently-typed value back to Dynamic.
    if n > 0 {
        kinds[0] = Some(SlotKind::Dynamic);
    }
    for (i, pk) in param_kinds.iter().enumerate() {
        if 1 + i < n {
            kinds[1 + i] = Some(*pk);
        }
    }
    for (vi, def) in ssa.values.iter().enumerate() {
        let Some(&r) = reg.get(vi) else { continue };
        let r = r as usize;
        if r >= n {
            continue;
        }
        let k = slot_kind_of(def.ty);
        kinds[r] = Some(match kinds[r] {
            None => k,
            Some(cur) if cur == k => cur,
            Some(_) => SlotKind::Dynamic,
        });
    }
    kinds
        .into_iter()
        .map(|k| RegisterMeta {
            kind: k.unwrap_or(SlotKind::Dynamic),
        })
        .collect()
}

fn slot_kind_of(ty: crate::hir::HirType) -> varn_types::register_meta::SlotKind {
    use crate::hir::HirType;
    use varn_types::register_meta::SlotKind;
    match ty {
        HirType::Int => SlotKind::Int,
        HirType::Float => SlotKind::Float,
        HirType::Bool => SlotKind::Bool,
        HirType::Str => SlotKind::Str,
        HirType::Class(id) => SlotKind::Class(id.0),
        HirType::Array(id) => SlotKind::Array(id.0),
        HirType::Ref
        | HirType::Map(_, _)
        | HirType::Set(_) => SlotKind::Ref,
        HirType::Nullable(id) => SlotKind::Nullable(id.0),
        HirType::Dynamic => SlotKind::Dynamic,
    }
}

/// Loop-aware emission order: reverse postorder from the entry, visiting
/// `else` before `then` so a loop header's body lands immediately after the
/// header (fall-through entry) and the exit lands after the whole body.
/// Nested loop bodies stay contiguous because the DFS nests. Plain
/// `0..n` order emitted inner-loop headers behind a forward `Jump`, which
/// disqualified every nested loop from the JIT's loop-invariant hoisting
/// (`header_reachable_by_fallthrough`). Unreachable blocks are appended in
/// numeric order so every block is still emitted.
fn emission_order(ssa: &SsaFunc) -> Vec<usize> {
    let n = ssa.blocks.len();
    let mut visited = vec![false; n];
    let mut post: Vec<usize> = Vec::with_capacity(n);
    let mut stack: Vec<(usize, u8)> = Vec::with_capacity(n);
    let entry = ssa.entry.0 as usize;
    visited[entry] = true;
    stack.push((entry, 0));
    while let Some(top) = stack.last_mut() {
        let (b, stage) = *top;
        top.1 += 1;
        let succ = match &ssa.blocks[b].term {
            Terminator::Jump { target, .. } if stage == 0 => Some(target.0 as usize),
            Terminator::Branch { else_blk, .. } if stage == 0 => Some(else_blk.0 as usize),
            Terminator::Branch { then_blk, .. } if stage == 1 => Some(then_blk.0 as usize),
            _ => None,
        };
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
    // Static type of every SSA value, so a binary can be specialized on what
    // its OPERANDS are proven to be and not only on its own result type.
    value_tys: &[crate::hir::HirType],
    reg: &[u8],
    scratch: u8,
    call_base: u8,
    cache_count: &mut u16,
    source_file: &Rc<str>,
    nparams: usize,
    fixups: &mut Vec<(usize, BlockId)>,
    imms: &Immediates,
) -> Result<()> {
    // A constant that only ever rides inside an `AddImm`/`SubImm` needs no
    // instruction of its own.
    if let (Some(d), InstKind::ConstInt(_)) = (inst.dest, &inst.kind) {
        if imms.is_elided(d) {
            return Ok(());
        }
    }
    if let Some((_, other, value)) = immediates::immediate_operand(&inst.kind, &imms.imm) {
        let dest = reg[inst.dest.expect("binary defines a value").0 as usize];
        // `a - c` subtracts the immediate; `c - a` never reaches here.
        let opcode = match &inst.kind {
            InstKind::Binary {
                op: crate::hir::HirBinOp::Sub,
                ..
            } => OpCode::SubImm,
            _ => OpCode::AddImm,
        };
        // Same shape as any three-register op: dest in the opcode word, then
        // `(src, imm)` — the immediate rides in the byte a second register
        // would occupy, which is why it is 8-bit and signed.
        chunk.emit_rrr(opcode, dest, reg[other.0 as usize], value as u8, inst.line);
        return Ok(());
    }

    if effects::emit_effect(chunk, inst, reg, cache_count, nparams)? {
        return Ok(());
    }

    let d = match inst.dest {
        Some(dest) => reg[dest.0 as usize],
        // DCE clears the destination of a call whose result nothing reads
        // (`crate::passes::dce::dest_droppable`). The call must still run, so
        // it emits as usual with its result thrown into `scratch` — written,
        // never read. Any other destination-less instruction that reached
        // here is not one `emit_value` knows how to emit; `emit_effect` above
        // was its only handler.
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
        cache_count,
        source_file,
        nparams,
        fixups,
    )
}
