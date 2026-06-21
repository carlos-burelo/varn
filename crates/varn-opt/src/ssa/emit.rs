//! Out-of-SSA: lower an [`SsaFunc`] to a [`FunctionProto`] (bytecode).
//!
//! Pipeline: split phi-carrying branch edges (so every block parameter is fed
//! only from single-successor `Jump` edges), assign one register per SSA value,
//! then emit blocks in index order. Block parameters become physical registers;
//! the phi operands on each edge are materialised as parallel `Move`s before the
//! edge's jump (cycles broken with a scratch register).
//!
//! Jumps: forward edges use `Jump`/`JumpIf*` with a deferred offset fixup;
//! backward edges (loop back-edges, merge-from-later-block) use `Loop`. A
//! conditional whose taken target is backward is reshaped so the *forward* exit
//! is the conditional and the backward edge is an unconditional `Loop` (mirrors
//! the §1 loop shape). The register-allocation post-pass (`regalloc_post`) and
//! `slot_kinds::infer` run downstream via the `VN_OPT` gate, so this emits naive
//! one-reg-per-value bytecode and lets them compress + type it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, Literal, PolyICSlot, PoolEntry};

use crate::hir::{HirFunction, HirUnOp};
use crate::lower::bin_opcode;
use crate::OptError;

use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value};

type Result<T> = std::result::Result<T, OptError>;

const LINE: u32 = 0;

pub fn emit_function(mut ssa: SsaFunc, f: &HirFunction, source_file: Rc<str>) -> Result<FunctionProto> {
    split_phi_edges(&mut ssa);

    let nparams = f.params.len();
    let (reg, scratch, null_reg, call_base, register_count) = assign_registers(&ssa, nparams)?;

    let n = ssa.blocks.len();
    let mut chunk = Chunk::new();
    chunk.source_file = source_file.clone();
    let mut block_offset = vec![usize::MAX; n];
    // Forward jumps recorded as `(operand_pos, target)`, patched once every
    // block's offset is known.
    let mut fixups: Vec<(usize, BlockId)> = Vec::new();
    // Inline-cache slots consumed by `GetProperty`/method sites, in emit order.
    let mut cache_count: u16 = 0;

    for b in 0..n {
        block_offset[b] = chunk.code.len();
        let insts = std::mem::take(&mut ssa.blocks[b].insts);
        for inst in &insts {
            emit_inst(&mut chunk, inst, &reg, scratch, call_base, &mut cache_count, &source_file)?;
        }
        let term = ssa.blocks[b].term.clone();
        emit_terminator(&mut chunk, &ssa, &reg, b, &term, null_reg, scratch, &block_offset, &mut fixups)?;
    }

    // Implicit `return null` epilogue (matches the naive `FnLower::finish`).
    chunk.write(Chunk::pack_op(OpCode::LoadNull, null_reg), LINE);
    chunk.emit1(OpCode::Return, Chunk::pack(0, null_reg), LINE);

    for (pos, target) in fixups {
        let target_off = block_offset[target.0 as usize];
        let rel = target_off as isize - pos as isize - 2;
        if rel < 0 {
            return Err(OptError::Unsupported("ssa-emit: forward jump resolved backward"));
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
        register_meta: Vec::new(),
        jit_entry: Cell::new(None),
        jit_code: RefCell::new(None),
        jit_failed: Cell::new(false),
        ic_cache: Rc::new(RefCell::new(
            (0..cache_count).map(|_| PolyICSlot::new()).collect(),
        )),
        feedback: Rc::new(RefCell::new(FeedbackVector::new(cache_count as usize))),
        static_closure_val: Cell::new(0),
    })
}

/// Insert a pad block on every branch edge whose target has block parameters,
/// so all phi operands are subsequently fed from single-successor `Jump` edges
/// (no critical edges). The pad carries the edge's arguments and jumps to the
/// real target.
fn split_phi_edges(ssa: &mut SsaFunc) {
    let original = ssa.blocks.len();
    for b in 0..original {
        let (then_has, else_has) = match &ssa.blocks[b].term {
            Terminator::Branch { then_blk, else_blk, .. } => (
                !ssa.blocks[then_blk.0 as usize].params.is_empty(),
                !ssa.blocks[else_blk.0 as usize].params.is_empty(),
            ),
            _ => continue,
        };
        if then_has {
            split_one(ssa, BlockId(b as u32), true);
        }
        if else_has {
            split_one(ssa, BlockId(b as u32), false);
        }
    }
}

fn split_one(ssa: &mut SsaFunc, br: BlockId, is_then: bool) {
    let (target, args) = match &mut ssa.blocks[br.0 as usize].term {
        Terminator::Branch { then_blk, then_args, else_blk, else_args, .. } => {
            if is_then {
                (*then_blk, std::mem::take(then_args))
            } else {
                (*else_blk, std::mem::take(else_args))
            }
        }
        _ => return,
    };
    let pad = BlockId(ssa.blocks.len() as u32);
    ssa.blocks.push(Block {
        params: Vec::new(),
        insts: Vec::new(),
        term: Terminator::Jump { target, args },
        preds: vec![br],
    });
    if let Terminator::Branch { then_blk, else_blk, .. } = &mut ssa.blocks[br.0 as usize].term {
        if is_then {
            *then_blk = pad;
        } else {
            *else_blk = pad;
        }
    }
    for p in ssa.blocks[target.0 as usize].preds.iter_mut() {
        if *p == br {
            *p = pad;
            break;
        }
    }
}

/// Assign one physical register per defined SSA value. Entry-block parameters
/// are the function parameters → registers `1..=nparams` (register 0 is the
/// receiver). Returns `(reg_of_value, scratch, null_reg, call_base, register_count)`.
/// `call_base` starts a contiguous block (sized to the widest call) for the
/// plain-call ABI's receiver+args, above every value register.
fn assign_registers(ssa: &SsaFunc, nparams: usize) -> Result<(Vec<u8>, u8, u8, u8, u16)> {
    let mut reg = vec![0u8; ssa.values.len()];
    let mut assigned = vec![false; ssa.values.len()];

    let entry_params = &ssa.blocks[ssa.entry.0 as usize].params;
    if entry_params.len() != nparams {
        return Err(OptError::Unsupported("ssa-emit: entry params != arity"));
    }
    for (i, p) in entry_params.iter().enumerate() {
        reg[p.0 as usize] = (1 + i) as u8;
        assigned[p.0 as usize] = true;
    }

    let mut next: u32 = 1 + nparams as u32;
    let alloc = |v: Value, reg: &mut [u8], assigned: &mut [bool], next: &mut u32| -> Result<()> {
        if !assigned[v.0 as usize] {
            if *next > 255 {
                return Err(OptError::Unsupported("ssa-emit: register count exceeds 255"));
            }
            reg[v.0 as usize] = *next as u8;
            assigned[v.0 as usize] = true;
            *next += 1;
        }
        Ok(())
    };
    for block in &ssa.blocks {
        for p in &block.params {
            alloc(*p, &mut reg, &mut assigned, &mut next)?;
        }
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                alloc(d, &mut reg, &mut assigned, &mut next)?;
            }
        }
    }
    // Widest call (receiver + args) → the contiguous call-ABI scratch block.
    let mut max_call = 0u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            let t = match &inst.kind {
                // Call/SelfCall: null receiver + args. MethodCall: args only
                // (receiver passed separately).
                InstKind::Call { args, .. } => args.len() as u32 + 1,
                InstKind::SelfCall { args } => args.len() as u32 + 1,
                InstKind::MethodCall { args, .. } => args.len() as u32,
                // Array elements share the contiguous `call_base` block.
                InstKind::BuildArray { elements } => elements.len() as u32,
                // Intrinsic operands: object + args, contiguous in `call_base`.
                InstKind::IntrinsicCall { args, .. } => args.len() as u32 + 1,
                _ => 0,
            };
            max_call = max_call.max(t);
        }
    }
    // Reserved: scratch (copy-cycle / `~`), null slot (return null), then the call
    // block. All registers are `u8`, so the top must stay within 0..=255.
    let total = next + 2 + max_call;
    if total > 256 {
        return Err(OptError::Unsupported("ssa-emit: register count exceeds 255"));
    }
    let scratch = next as u8;
    let null_reg = (next + 1) as u8;
    let call_base = (next + 2) as u8;
    let register_count = total.max(1) as u16;
    Ok((reg, scratch, null_reg, call_base, register_count))
}

fn emit_inst(
    chunk: &mut Chunk,
    inst: &Inst,
    reg: &[u8],
    scratch: u8,
    call_base: u8,
    cache_count: &mut u16,
    source_file: &Rc<str>,
) -> Result<()> {
    // Side-effecting writes have no `dest`; handle them before the dest guard.
    match &inst.kind {
        InstKind::SetProperty { object, name, value } => {
            let idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported("ssa-emit: too many inline-cache sites"));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.emit_rrc_ic(OpCode::SetProperty, reg[object.0 as usize], reg[value.0 as usize], idx, cs, LINE);
            return Ok(());
        }
        InstKind::SetIndex { object, index, value } => {
            chunk.emit_rrr(OpCode::SetIndex, reg[object.0 as usize], reg[index.0 as usize], reg[value.0 as usize], LINE);
            return Ok(());
        }
        // `expr!`: assert the operand is non-null in place (operand reg in the
        // high byte; the value passes through). No dest.
        InstKind::AssertNotNull { operand } => {
            chunk.emit1(OpCode::AssertNotNull, Chunk::pack(reg[operand.0 as usize], 0), LINE);
            return Ok(());
        }
        _ => {}
    }

    let Some(dest) = inst.dest else { return Ok(()) };
    let d = reg[dest.0 as usize];
    match &inst.kind {
        InstKind::ConstInt(n) => chunk.emit_load_int(d, *n, LINE),
        InstKind::ConstFloat(f) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Float(*f)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, LINE);
        }
        InstKind::ConstBool(b) => {
            let op = if *b { OpCode::LoadTrue } else { OpCode::LoadFalse };
            chunk.emit_rr(op, d, 0, LINE);
        }
        InstKind::ConstStr(s) => {
            let idx = chunk.add_str(s);
            chunk.emit_rc(OpCode::LoadConst, d, idx, LINE);
        }
        InstKind::ConstChar(c) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Char(*c)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, LINE);
        }
        InstKind::ConstDecimal(dec) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Decimal(*dec)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, LINE);
        }
        InstKind::ConstBigInt(n) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::BigInt(*n)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, LINE);
        }
        InstKind::ConstNull => chunk.emit_rr(OpCode::LoadNull, d, 0, LINE),
        InstKind::Binary { op, lhs, rhs, ty } => {
            let opcode = bin_opcode(*op, *ty);
            chunk.emit_rrr(opcode, d, reg[lhs.0 as usize], reg[rhs.0 as usize], LINE);
        }
        InstKind::Unary { op, operand, .. } => {
            let s = reg[operand.0 as usize];
            match op {
                HirUnOp::Neg => chunk.emit_rr(OpCode::Negate, d, s, LINE),
                HirUnOp::Not => chunk.emit_rr(OpCode::Not, d, s, LINE),
                HirUnOp::Typeof => chunk.emit_rr(OpCode::Typeof, d, s, LINE),
                HirUnOp::BitNot => {
                    // `~x` == `x ^ -1` (mirrors the §1 emitter).
                    let idx = chunk.add_constant(PoolEntry::Literal(Literal::Int(-1)));
                    chunk.emit_rc(OpCode::LoadConst, scratch, idx, LINE);
                    chunk.emit_rrr(OpCode::BitXor, d, s, scratch, LINE);
                }
            }
        }
        InstKind::LoadGlobal(name) => {
            let idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::LoadGlobal, d, idx, LINE);
        }
        // Plain-call ABI: null receiver at `call_base`, args contiguous after it,
        // then `Call dest=d, callee` over `[receiver, args]`.
        InstKind::Call { callee, args } => {
            emit_call_args(chunk, reg, call_base, args);
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), LINE);
            chunk.write(Chunk::pack(total, call_base), LINE);
        }
        // Self-recursion ABI: no callee load; `CallSelf dest=d` over the same block.
        InstKind::SelfCall { args } => {
            emit_call_args(chunk, reg, call_base, args);
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::CallSelf, LINE);
            chunk.write(Chunk::pack(d, 0), LINE);
            chunk.write(Chunk::pack(total, call_base), LINE);
        }
        InstKind::GetProperty { object, name } => {
            let idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported("ssa-emit: too many inline-cache sites"));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.emit_rrc_ic(OpCode::GetProperty, d, reg[object.0 as usize], idx, cs, LINE);
        }
        InstKind::GetIndex { object, index } => {
            chunk.emit_rrr(OpCode::GetIndex, d, reg[object.0 as usize], reg[index.0 as usize], LINE);
        }
        // `recv.name(args)`: args at `call_base` (no null receiver — the receiver
        // is passed separately in the opcode), then `CallMethod` with an IC slot.
        InstKind::MethodCall { recv, name, args } => {
            let name_idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported("ssa-emit: too many inline-cache sites"));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[a.0 as usize], LINE);
            }
            let argc = args.len() as u8;
            chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), LINE);
            chunk.write(Chunk::pack(d, reg[recv.0 as usize]), LINE);
            chunk.write(name_idx, LINE);
            chunk.write(Chunk::pack(argc, call_base), LINE);
        }
        InstKind::IsNull { operand } => {
            chunk.emit_rr(OpCode::IsNull, d, reg[operand.0 as usize], LINE);
        }
        // `[…]`: elements contiguous from `call_base`, then `BuildArray d, start, count`.
        InstKind::BuildArray { elements } => {
            for (i, e) in elements.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], LINE);
            }
            chunk.emit(OpCode::BuildArray, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(elements.len() as u8, 0), LINE);
        }
        // `{…}`: explicit (key const, value reg) pairs — no contiguity needed.
        InstKind::BuildObject { pairs } => {
            chunk.emit(OpCode::BuildObject, LINE);
            chunk.write(Chunk::pack(d, pairs.len() as u8), LINE);
            for (k, v) in pairs {
                let key_idx = chunk.add_str(k);
                chunk.write(key_idx, LINE);
                chunk.write(Chunk::pack(reg[v.0 as usize], 0), LINE);
            }
        }
        InstKind::ToString { operand } => {
            chunk.emit_rr(OpCode::ToString, d, reg[operand.0 as usize], LINE);
        }
        // Capture-free closure → compile the nested fn to a proto constant
        // (`lower_function` re-enters the SSA gate per-function), then LoadStaticFn.
        InstKind::MakeClosure { func } => {
            let proto = crate::lower::lower_function(func, source_file.clone());
            let idx = chunk.add_constant(PoolEntry::Function(Rc::new(proto)));
            chunk.write(Chunk::pack_op(OpCode::LoadStaticFn, d), LINE);
            chunk.write(idx, LINE);
        }
        // `Intrinsic`: object + args contiguous from `call_base`, result lands in
        // `call_base`, then copied down to the value's register.
        InstKind::IntrinsicCall { object, args, wire_byte } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + 1 + i as u8, reg[a.0 as usize], LINE);
            }
            let arg_count = (args.len() + 1) as u16;
            chunk.write(Chunk::pack_op(OpCode::Intrinsic, call_base), LINE);
            chunk.write(((*wire_byte as u16) << 8) | arg_count, LINE);
            chunk.emit_rr(OpCode::Move, d, call_base, LINE);
        }
        // `BuildStr d, count` followed by one packed reg per part.
        InstKind::BuildStr { parts } => {
            chunk.write(Chunk::pack_op(OpCode::BuildStr, d), LINE);
            chunk.write(Chunk::pack(parts.len() as u8, 0), LINE);
            for p in parts {
                chunk.write(Chunk::pack(reg[p.0 as usize], 0), LINE);
            }
        }
        InstKind::GetPropertyMaybe { object, name } => {
            let idx = chunk.add_str(name);
            chunk.emit_rrc(OpCode::GetPropertyMaybe, d, reg[object.0 as usize], idx, LINE);
        }
        InstKind::ModuleSlot { object, slot } => {
            chunk.emit_rrc(OpCode::LoadModuleSlot, d, reg[object.0 as usize], *slot, LINE);
        }
        InstKind::GetEnumTag { operand } => {
            chunk.emit_rr(OpCode::GetEnumTag, d, reg[operand.0 as usize], LINE);
        }
        InstKind::IsArray { operand } => {
            chunk.emit_rr(OpCode::IsArray, d, reg[operand.0 as usize], LINE);
        }
        // The receiver lives in register 0; copy it into this value's register.
        InstKind::This => chunk.emit_rr(OpCode::Move, d, 0, LINE),
        // `start..end` → `InvokeRuntimeStatic __range__`. The VM reads `start`
        // from arg_start and `end` from end_reg separately (no contiguity).
        InstKind::Range { start, end, inclusive } => {
            let method = chunk.add_str(varn_core::well_known::RUNTIME_RANGE);
            let flag = if *inclusive { 1u8 } else { 0u8 };
            chunk.emit(OpCode::InvokeRuntimeStatic, LINE);
            chunk.write(Chunk::pack(d, 0), LINE);
            chunk.write(method, LINE);
            chunk.write(Chunk::pack(2, reg[start.0 as usize]), LINE);
            chunk.write(Chunk::pack(reg[end.0 as usize], flag), LINE);
        }
        InstKind::ObjectKeys { operand } => {
            chunk.emit_rr(OpCode::ObjectKeys, d, reg[operand.0 as usize], LINE);
        }
        // Dest-less side effects handled before the dest guard above.
        InstKind::SetProperty { .. } | InstKind::SetIndex { .. } | InstKind::AssertNotNull { .. } => {
            unreachable!()
        }
    }
    Ok(())
}

/// Lay out a call's receiver (null) + argument registers contiguously from
/// `call_base`, copying each argument value into its slot.
fn emit_call_args(chunk: &mut Chunk, reg: &[u8], call_base: u8, args: &[Value]) {
    chunk.emit_rr(OpCode::LoadNull, call_base, 0, LINE);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(OpCode::Move, call_base + 1 + i as u8, reg[a.0 as usize], LINE);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_terminator(
    chunk: &mut Chunk,
    ssa: &SsaFunc,
    reg: &[u8],
    b: usize,
    term: &Terminator,
    null_reg: u8,
    scratch: u8,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
) -> Result<()> {
    match term {
        Terminator::Return(Some(v)) => {
            chunk.emit1(OpCode::Return, Chunk::pack(0, reg[v.0 as usize]), LINE);
        }
        Terminator::Return(None) | Terminator::Unreachable => {
            chunk.write(Chunk::pack_op(OpCode::LoadNull, null_reg), LINE);
            chunk.emit1(OpCode::Return, Chunk::pack(0, null_reg), LINE);
        }
        Terminator::Throw(v) => {
            chunk.emit1(OpCode::Throw, Chunk::pack(reg[v.0 as usize], 0), LINE);
        }
        Terminator::Jump { target, args } => {
            emit_edge_copies(chunk, ssa, reg, *target, args, scratch);
            emit_goto(chunk, b, *target, block_offset, fixups);
        }
        Terminator::Branch { cond, then_blk, then_args, else_blk, else_args } => {
            if !then_args.is_empty() || !else_args.is_empty() {
                return Err(OptError::Unsupported("ssa-emit: branch edge carries args after split"));
            }
            emit_branch(chunk, b, reg[cond.0 as usize], *then_blk, *else_blk, block_offset, fixups)?;
        }
    }
    Ok(())
}

/// Materialise phi operands for a `Jump` edge as parallel `Move`s (param reg <-
/// arg reg), breaking cycles via the scratch register.
fn emit_edge_copies(chunk: &mut Chunk, ssa: &SsaFunc, reg: &[u8], target: BlockId, args: &[Value], scratch: u8) {
    let params = &ssa.blocks[target.0 as usize].params;
    let mut copies: Vec<(u8, u8)> = params
        .iter()
        .zip(args)
        .map(|(p, a)| (reg[p.0 as usize], reg[a.0 as usize]))
        .filter(|(d, s)| d != s)
        .collect();

    while !copies.is_empty() {
        if let Some(pos) = copies.iter().position(|(d, _)| !copies.iter().any(|(_, s)| s == d)) {
            let (d, s) = copies.remove(pos);
            chunk.emit_rr(OpCode::Move, d, s, LINE);
        } else {
            // Every remaining destination is also a source → a cycle. Spill one
            // source to scratch, then it is no longer blocked.
            let s0 = copies[0].1;
            chunk.emit_rr(OpCode::Move, scratch, s0, LINE);
            for c in copies.iter_mut() {
                if c.1 == s0 {
                    c.1 = scratch;
                }
            }
        }
    }
}

/// Unconditional transfer to `target`: fall through when it is the next block,
/// `Loop` when already emitted (backward), else a forward `Jump` + fixup.
fn emit_goto(chunk: &mut Chunk, b: usize, target: BlockId, block_offset: &[usize], fixups: &mut Vec<(usize, BlockId)>) {
    let tb = target.0 as usize;
    if tb == b + 1 {
        return; // fall through
    }
    if tb <= b {
        chunk.emit_loop(block_offset[tb], LINE);
    } else {
        let pos = chunk.emit_jump(OpCode::Jump, LINE);
        fixups.push((pos, target));
    }
}

fn emit_branch(
    chunk: &mut Chunk,
    b: usize,
    cond: u8,
    then_blk: BlockId,
    else_blk: BlockId,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
) -> Result<()> {
    let tb = then_blk.0 as usize;
    let eb = else_blk.0 as usize;
    let then_fwd = tb > b;
    let else_fwd = eb > b;

    match (then_fwd, else_fwd) {
        (true, true) => {
            if tb == b + 1 {
                let pos = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, LINE);
                fixups.push((pos, else_blk));
            } else if eb == b + 1 {
                let pos = chunk.emit_cond_jump(OpCode::JumpIfTrue, cond, LINE);
                fixups.push((pos, then_blk));
            } else {
                let p1 = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, LINE);
                fixups.push((p1, else_blk));
                let p2 = chunk.emit_jump(OpCode::Jump, LINE);
                fixups.push((p2, then_blk));
            }
        }
        (true, false) => {
            // else is backward → forward conditional to `then`, unconditional Loop to `else`.
            let pos = chunk.emit_cond_jump(OpCode::JumpIfTrue, cond, LINE);
            fixups.push((pos, then_blk));
            chunk.emit_loop(block_offset[eb], LINE);
        }
        (false, true) => {
            let pos = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, LINE);
            fixups.push((pos, else_blk));
            chunk.emit_loop(block_offset[tb], LINE);
        }
        (false, false) => {
            return Err(OptError::Unsupported("ssa-emit: branch with both targets backward"));
        }
    }
    Ok(())
}
