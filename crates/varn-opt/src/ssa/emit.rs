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

use rustc_hash::FxHashSet;

use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, Literal, PolyICSlot, PoolEntry};
use varn_types::value::RuntimeSymbol;

use crate::hir::{HirFunction, HirUnOp, HirUpvalueSrc};
use crate::lower::bin_opcode;
use crate::OptError;

use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, VarId};

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
            emit_inst(
                &mut chunk,
                inst,
                &reg,
                scratch,
                call_base,
                &mut cache_count,
                &source_file,
                nparams,
                &mut fixups,
            )?;
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

    // Register reuse needs real liveness: a value used inside a loop is live
    // across the back-edge, so a naive forward "last use" frees it too early and
    // a later value would clobber it. Compute live-in/out per block by backward
    // dataflow, then build a [def, end] interval per value that is point-precise
    // within a block (def/use indices) and extended block-wide wherever the value
    // is live-out (covering loops). A linear scan packs values above the fixed
    // param+local slots, reusing registers whose interval has ended.
    // `regalloc_post` re-colours afterwards; this only has to stay within u8.
    let nvals = ssa.values.len();
    let nblocks = ssa.blocks.len();
    let mut def = vec![u32::MAX; nvals];
    let mut last = vec![0u32; nvals];
    let mut term_idx = vec![0u32; nblocks];
    let mut defs: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
    let mut uses: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); nblocks];
    let mut idx = 0u32;
    for (b, block) in ssa.blocks.iter().enumerate() {
        for p in &block.params {
            if def[p.0 as usize] == u32::MAX {
                def[p.0 as usize] = idx;
            }
            defs[b].insert(p.0);
        }
        idx += 1;
        for inst in &block.insts {
            for u in crate::ssa::verify::inst_uses(&inst.kind) {
                if last[u.0 as usize] < idx {
                    last[u.0 as usize] = idx;
                }
                uses[b].insert(u.0);
            }
            if let Some(d) = inst.dest {
                if def[d.0 as usize] == u32::MAX {
                    def[d.0 as usize] = idx;
                }
                defs[b].insert(d.0);
            }
            if let InstKind::Try { handler } = &inst.kind {
                succ[b].push(handler.0 as usize);
            }
            idx += 1;
        }
        let mut touch = |v: Value, uses: &mut FxHashSet<u32>| {
            if last[v.0 as usize] < idx {
                last[v.0 as usize] = idx;
            }
            uses.insert(v.0);
        };
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => touch(*v, &mut uses[b]),
            Terminator::Branch { cond, then_blk, then_args, else_blk, else_args } => {
                touch(*cond, &mut uses[b]);
                then_args.iter().chain(else_args).for_each(|a| touch(*a, &mut uses[b]));
                succ[b].push(then_blk.0 as usize);
                succ[b].push(else_blk.0 as usize);
            }
            Terminator::Jump { target, args } => {
                args.iter().for_each(|a| touch(*a, &mut uses[b]));
                succ[b].push(target.0 as usize);
            }
            Terminator::Return(None) | Terminator::Unreachable => {}
        }
        term_idx[b] = idx;
        idx += 1;
    }

    // Backward dataflow: live_in[B] = uses[B] ∪ (live_out[B] \ defs[B]).
    let mut live_in: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
    let mut live_out: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
    let mut changed = true;
    while changed {
        changed = false;
        for b in (0..nblocks).rev() {
            let mut out = FxHashSet::default();
            for &s in &succ[b] {
                out.extend(live_in[s].iter().copied());
            }
            let mut nin = uses[b].clone();
            nin.extend(out.iter().copied().filter(|v| !defs[b].contains(v)));
            if out != live_out[b] || nin != live_in[b] {
                live_out[b] = out;
                live_in[b] = nin;
                changed = true;
            }
        }
    }

    // [def, end]: extend last-use to the end of any block the value is live-out of.
    let mut end = last;
    for b in 0..nblocks {
        for &v in &live_out[b] {
            if end[v as usize] < term_idx[b] {
                end[v as usize] = term_idx[b];
            }
        }
    }
    for v in 0..nvals {
        if def[v] != u32::MAX && end[v] < def[v] {
            end[v] = def[v]; // a never-used value dies at its definition
        }
    }

    // Only locals actually *captured* (via Load/StoreCaptured) need a fixed slot
    // (so `MakeClosure` can capture by register); the rest are ordinary SSA
    // values. Reserve just through the highest captured slot — reserving all
    // `nlocals` wastes a register per uncaptured local and overflows big
    // functions. Temps then start above the captured region.
    let mut base = 1 + nparams as u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let InstKind::LoadCaptured { var } | InstKind::StoreCaptured { var, .. } = &inst.kind {
                base = base.max(var_reg(*var, nparams) as u32 + 1);
            }
        }
    }
    let mut order: Vec<usize> = (0..nvals).filter(|&v| !assigned[v] && def[v] != u32::MAX).collect();
    order.sort_by_key(|&v| def[v]);
    let mut next: u32 = base;
    let mut free: Vec<u32> = Vec::new();
    let mut active: Vec<(u32, u32)> = Vec::new(); // (interval end, register)
    for &v in &order {
        let d = def[v];
        let mut i = 0;
        while i < active.len() {
            if active[i].0 < d {
                free.push(active[i].1);
                active.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let r = match free.iter().copied().min() {
            Some(m) => {
                free.retain(|&x| x != m);
                m
            }
            None => {
                let r = next;
                next += 1;
                r
            }
        };
        if r > 255 {
            if std::env::var_os("VN_OPT_TRACE").is_some() {
                eprintln!("[varn-opt] regalloc overflow: peak temps need r={r} (nvals={nvals}, nblocks={nblocks})");
            }
            return Err(OptError::Unsupported("ssa-emit: register count exceeds 255"));
        }
        reg[v] = r as u8;
        assigned[v] = true;
        active.push((end[v], r));
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
                // Iterator-protocol call: just the receiver in `call_base`.
                InstKind::IterCall { .. } => 1,
                // `super(args)`: fn slot + receiver + args. `super.name(args)`:
                // fn slot + args.
                InstKind::SuperCall { args } => args.len() as u32 + 2,
                InstKind::SuperMethodCall { args, .. } => args.len() as u32 + 1,
                // Extension call: fn slot + receiver + args. Spread call: null
                // receiver + args.
                InstKind::ExtensionCall { args, .. } => args.len() as u32 + 2,
                InstKind::CallSpread { args, .. } => args.len() as u32 + 1,
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

fn var_reg(var: VarId, nparams: usize) -> u8 {
    match var {
        VarId::Param(i) => (1 + i) as u8,
        VarId::Local(id) => (1 + nparams + id.0 as usize) as u8,
    }
}

fn emit_inst(
    chunk: &mut Chunk,
    inst: &Inst,
    reg: &[u8],
    scratch: u8,
    call_base: u8,
    cache_count: &mut u16,
    source_file: &Rc<str>,
    nparams: usize,
    fixups: &mut Vec<(usize, BlockId)>,
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
        // `DefineGlobal 0, value, name` (dest byte unused).
        InstKind::StoreGlobal { name, value } => {
            let idx = chunk.add_str(name);
            chunk.emit_rrc(OpCode::DefineGlobal, 0, reg[value.0 as usize], idx, LINE);
            return Ok(());
        }
        // `StoreUpvalue uv, value` (uv in the high byte).
        InstKind::StoreUpvalue { index, value } => {
            chunk.emit1(OpCode::StoreUpvalue, Chunk::pack(*index as u8, reg[value.0 as usize]), LINE);
            return Ok(());
        }
        InstKind::StoreCaptured { var, value } => {
            let dest_reg = var_reg(*var, nparams);
            chunk.emit_rr(OpCode::Move, dest_reg, reg[value.0 as usize], LINE);
            return Ok(());
        }
        InstKind::StoreModuleSlot { value, slot } => {
            chunk.emit_rc(OpCode::StoreModuleSlot, reg[value.0 as usize], *slot, LINE);
            return Ok(());
        }
        InstKind::CloseUpvalues { targets } => {
            let lowest = targets
                .iter()
                .map(|t| var_reg(*t, nparams))
                .min()
                .unwrap_or(0);
            chunk.emit1(OpCode::CloseUpvalue, lowest as u16, LINE);
            return Ok(());
        }
        InstKind::Dispose { target, is_await } => {
            let r = var_reg(VarId::Local(*target), nparams);
            let method = if *is_await { "disposeAsync" } else { "dispose" };
            let str_idx = chunk.add_str(method);
            if *cache_count > 255 {
                return Err(OptError::Unsupported("ssa-emit: too many inline-cache sites"));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), LINE);
            chunk.write(Chunk::pack(r, r), LINE);
            chunk.write(str_idx, LINE);
            chunk.write(Chunk::pack(0u8, 0u8), LINE);
            return Ok(());
        }
        InstKind::PopTry => {
            chunk.emit(OpCode::PopTry, LINE);
            return Ok(());
        }
        InstKind::DeclareField { class, name } => {
            let class_reg = reg[class.0 as usize];
            let key_idx = chunk.add_str(name);
            chunk.emit(OpCode::DeclareField, LINE);
            chunk.write(Chunk::pack(class_reg, 0), LINE);
            chunk.write(key_idx, LINE);
            return Ok(());
        }
        InstKind::DefineStatic { class, name, value } => {
            let class_reg = reg[class.0 as usize];
            let val_reg = reg[value.0 as usize];
            let key_idx = chunk.add_str(name);
            chunk.emit(OpCode::DefineStatic, LINE);
            chunk.write(Chunk::pack(class_reg, val_reg), LINE);
            chunk.write(key_idx, LINE);
            return Ok(());
        }
        InstKind::DefineMethod { class, name, method, is_static } => {
            let class_reg = reg[class.0 as usize];
            let method_reg = reg[method.0 as usize];
            let key_idx = chunk.add_str(name);
            let op = if *is_static { OpCode::DefineStatic } else { OpCode::Method };
            chunk.emit(op, LINE);
            chunk.write(Chunk::pack(class_reg, method_reg), LINE);
            chunk.write(key_idx, LINE);
            return Ok(());
        }
        InstKind::DefineAccessor { class, name, accessor, is_getter, is_static } => {
            let class_reg = reg[class.0 as usize];
            let acc_reg = reg[accessor.0 as usize];
            let key_idx = chunk.add_str(name);
            let op = match (is_getter, is_static) {
                (true, true) => OpCode::DefineStaticGetter,
                (true, false) => OpCode::DefineGetter,
                (false, true) => OpCode::DefineStaticSetter,
                (false, false) => OpCode::DefineSetter,
            };
            chunk.emit(op, LINE);
            chunk.write(Chunk::pack(class_reg, acc_reg), LINE);
            chunk.write(key_idx, LINE);
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
        InstKind::LoadUpvalue(uv) => {
            chunk.emit(OpCode::LoadUpvalue, LINE);
            chunk.write(Chunk::pack(d, *uv as u8), LINE);
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
        InstKind::MakeClosure { func, upvalues, upvalues_src } => {
            let proto = crate::lower::lower_function(func, source_file.clone());
            let idx = chunk.add_constant(PoolEntry::Function(Rc::new(proto)));
            if upvalues.is_empty() {
                chunk.write(Chunk::pack_op(OpCode::LoadStaticFn, d), LINE);
                chunk.write(idx, LINE);
            } else {
                let uv_count = upvalues.len() as u8;
                chunk.emit(OpCode::MakeClosure, LINE);
                chunk.write(Chunk::pack(d, uv_count), LINE);
                chunk.write(idx, LINE);
                for (uv_val, uv_src) in upvalues.iter().zip(upvalues_src) {
                    let is_local = match uv_src {
                        HirUpvalueSrc::ParentLocal(_) | HirUpvalueSrc::ParentParam(_) => 1u8,
                        HirUpvalueSrc::ParentUpvalue(_) => 0u8,
                    };
                    let index = match uv_src {
                        HirUpvalueSrc::ParentLocal(_) | HirUpvalueSrc::ParentParam(_) => reg[uv_val.0 as usize],
                        HirUpvalueSrc::ParentUpvalue(idx) => *idx as u8,
                    };
                    chunk.write(Chunk::pack(is_local, index), LINE);
                }
            }
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
        InstKind::GetSymbol { object, is_async } => {
            let sym = if *is_async { RuntimeSymbol::AsyncIterator } else { RuntimeSymbol::Iterator };
            let idx = chunk.add_symbol(sym);
            chunk.emit_rrc(OpCode::GetSymbol, d, reg[object.0 as usize], idx, LINE);
        }
        // Iterator-protocol call: receiver at `call_base`, `Call` with arg_count 1.
        InstKind::IterCall { callee, recv } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[recv.0 as usize], LINE);
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), LINE);
            chunk.write(Chunk::pack(1, call_base), LINE);
        }
        // `super` / `super.name` member read.
        InstKind::GetSuper { name } => {
            let idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::GetSuper, d, idx, LINE);
        }
        // `super(args)`: GetSuper "constructor" into the fn slot, `this` (reg 0)
        // as the receiver, args after; `Call` over `[receiver, args]`.
        InstKind::SuperCall { args } => {
            let ctor_idx = chunk.add_str("constructor");
            chunk.emit_rc(OpCode::GetSuper, call_base, ctor_idx, LINE);
            chunk.emit_rr(OpCode::Move, call_base + 1, 0, LINE); // `this`
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + 2 + i as u8, reg[a.0 as usize], LINE);
            }
            let total = (args.len() + 1) as u8; // receiver + args
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(total, call_base + 1), LINE);
        }
        // `super.name(args)`: GetSuper name (bound to `this`) into the fn slot,
        // args after; `Call` over the args (no separate receiver).
        InstKind::SuperMethodCall { name, args } => {
            let name_idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::GetSuper, call_base, name_idx, LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + 1 + i as u8, reg[a.0 as usize], LINE);
            }
            let count = args.len() as u8;
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(count, if count > 0 { call_base + 1 } else { 0 }), LINE);
        }
        // Extension call: `LoadGlobal(func)` into the fn slot, `recv` as the
        // receiver, args after; `Call` over `[receiver, args]`.
        InstKind::ExtensionCall { func, recv, args } => {
            let idx = chunk.add_str(func);
            chunk.emit_rc(OpCode::LoadGlobal, call_base, idx, LINE);
            chunk.emit_rr(OpCode::Move, call_base + 1, reg[recv.0 as usize], LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + 2 + i as u8, reg[a.0 as usize], LINE);
            }
            let total = (args.len() + 1) as u8; // receiver + args
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(total, call_base + 1), LINE);
        }
        // Plain call with spread: null receiver, args after (spreads WrapSpread'd),
        // then `CallSpread`.
        InstKind::CallSpread { callee, args } => {
            chunk.emit_rr(OpCode::LoadNull, call_base, 0, LINE);
            for (i, (a, spread)) in args.iter().enumerate() {
                let op = if *spread { OpCode::WrapSpread } else { OpCode::Move };
                chunk.emit_rr(op, call_base + 1 + i as u8, reg[a.0 as usize], LINE);
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::CallSpread, LINE);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), LINE);
            chunk.write(Chunk::pack(total, call_base), LINE);
        }
        // `[a, ...b]`: empty array in `d`, then ArrayPush / ArrayExtend per item.
        InstKind::BuildArraySpread { elements } => {
            chunk.emit(OpCode::BuildArray, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(0, 0), LINE);
            for (v, spread) in elements {
                let op = if *spread { OpCode::ArrayExtend } else { OpCode::ArrayPush };
                chunk.emit_rr(op, d, reg[v.0 as usize], LINE);
            }
        }
        // `{a: 1, ...b}`: empty object in `d`, then SetProperty / ObjectMerge.
        InstKind::BuildObjectSpread { parts } => {
            chunk.emit(OpCode::BuildObject, LINE);
            chunk.write(Chunk::pack(d, 0), LINE);
            for (key, v) in parts {
                match key {
                    Some(k) => {
                        let idx = chunk.add_str(k);
                        if *cache_count > 255 {
                            return Err(OptError::Unsupported("ssa-emit: too many inline-cache sites"));
                        }
                        let cs = *cache_count as u8;
                        *cache_count += 1;
                        chunk.emit_rrc_ic(OpCode::SetProperty, d, reg[v.0 as usize], idx, cs, LINE);
                    }
                    None => chunk.emit_rr(OpCode::ObjectMerge, d, reg[v.0 as usize], LINE),
                }
            }
        }
        InstKind::LoadCaptured { var } => {
            let src = var_reg(*var, nparams);
            chunk.emit_rr(OpCode::Move, d, src, LINE);
        }
        InstKind::MakeClass { name, super_class } => {
            let name_idx = chunk.add_str(name);
            let super_reg = super_class.map(|sc| reg[sc.0 as usize]).unwrap_or(0);
            chunk.emit_rrc(OpCode::MakeClass, d, super_reg, name_idx, LINE);
        }
        InstKind::MakeEnumVariant { tag, meta } => {
            let meta_idx = chunk.add_str(meta);
            chunk.emit_load_int(scratch, *tag, LINE);
            chunk.emit(OpCode::MakeEnumVariant, LINE);
            chunk.write(Chunk::pack(d, scratch), LINE);
            chunk.write(meta_idx, LINE);
        }
        InstKind::Try { handler } => {
            chunk.emit(OpCode::Try, LINE);
            chunk.write(Chunk::pack(d, 0), LINE);
            let pos = chunk.code.len();
            chunk.write(0xFFFF, LINE);
            chunk.write(0xFFFF, LINE);
            fixups.push((pos, *handler));
        }
        InstKind::CatchParam { try_val } => {
            chunk.emit_rr(OpCode::Move, d, reg[try_val.0 as usize], LINE);
        }
        InstKind::LoadModule { source } => {
            let src_idx = chunk.add_str(source);
            chunk.emit_rc(OpCode::LoadModule, d, src_idx, LINE);
        }
        InstKind::Await { operand } => {
            chunk.emit_rr(OpCode::Await, d, reg[operand.0 as usize], LINE);
        }
        InstKind::Spawn { operand } => {
            chunk.emit2(OpCode::Spawn, Chunk::pack(d, reg[operand.0 as usize]), 0, LINE);
        }
        InstKind::Yield { operand } => {
            chunk.emit1(OpCode::Yield, Chunk::pack(d, reg[operand.0 as usize]), LINE);
        }
        // Dest-less side effects handled before the dest guard above.
        InstKind::SetProperty { .. }
        | InstKind::SetIndex { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::StoreCaptured { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::PopTry
        | InstKind::DeclareField { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. } => {
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
