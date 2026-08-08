use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, VarId};
use crate::hir::{HirFunction, HirUnOp, HirUpvalueSrc};
use crate::lower::bin_opcode;
use crate::OptError;
use rustc_hash::FxHashSet;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, Literal, PolyICSlot, PoolEntry};
use varn_types::value::RuntimeSymbol;
type Result<T> = std::result::Result<T, OptError>;

const LINE: u32 = 0;

pub fn emit_function(
    mut ssa: SsaFunc,
    f: &HirFunction,
    source_file: Rc<str>,
) -> Result<FunctionProto> {
    split_phi_edges(&mut ssa);

    let nparams = f.params.len();
    let (reg, scratch, null_reg, call_base, register_count) = assign_registers(&ssa, nparams)?;
    let param_kinds: Vec<_> = f.params.iter().map(|p| slot_kind_of(p.ty)).collect();
    let register_meta = derive_register_meta(&ssa, &reg, register_count, &param_kinds);
    let return_kind = slot_kind_of(f.return_ty);

    let n = ssa.blocks.len();
    let mut chunk = Chunk::new();
    chunk.source_file = source_file.clone();
    let mut block_offset = vec![usize::MAX; n];

    let mut fixups: Vec<(usize, BlockId)> = Vec::new();

    let mut cache_count: u16 = 0;

    let imms = plan_immediates(&ssa);

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
        emit_terminator(
            &mut chunk,
            &ssa,
            &reg,
            i,
            &pos_of,
            &term,
            null_reg,
            scratch,
            &block_offset,
            &mut fixups,
        )?;
    }

    chunk.write(Chunk::pack_op(OpCode::LoadNull, null_reg), LINE);
    chunk.emit1(OpCode::Return, Chunk::pack(0, null_reg), LINE);

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
        register_meta,
        exception_table: Vec::new(),
        param_kinds,
        return_kind,
        resolved_shapes: RefCell::new(Vec::new()),
        jit_entry: Cell::new(None),
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
/// re-inference in `varn-backend`, which guessed back what the checker
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
        HirType::Ref
        | HirType::Array(_)
        | HirType::Map(_, _)
        | HirType::Set(_)
        | HirType::Class(_) => SlotKind::Ref,
        // A nullable slot can hold null (an immediate) or the payload; no
        // single kind is always true of it.
        HirType::Nullable(_) | HirType::Dynamic => SlotKind::Dynamic,
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

fn split_phi_edges(ssa: &mut SsaFunc) {
    let original = ssa.blocks.len();
    for b in 0..original {
        let (then_has, else_has) = match &ssa.blocks[b].term {
            Terminator::Branch {
                then_blk, else_blk, ..
            } => (
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
        Terminator::Branch {
            then_blk,
            then_args,
            else_blk,
            else_args,
            ..
        } => {
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
    if let Terminator::Branch {
        then_blk, else_blk, ..
    } = &mut ssa.blocks[br.0 as usize].term
    {
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
        let mut local_defined: FxHashSet<u32> = FxHashSet::default();
        for p in &block.params {
            if def[p.0 as usize] == u32::MAX {
                def[p.0 as usize] = idx;
            }
            defs[b].insert(p.0);
            local_defined.insert(p.0);
        }
        idx += 1;
        for inst in &block.insts {
            for u in crate::ssa::verify::inst_uses(&inst.kind) {
                if last[u.0 as usize] < idx {
                    last[u.0 as usize] = idx;
                }
                if !local_defined.contains(&u.0) {
                    uses[b].insert(u.0);
                }
            }
            if let Some(d) = inst.dest {
                if def[d.0 as usize] == u32::MAX {
                    def[d.0 as usize] = idx;
                }
                defs[b].insert(d.0);
                local_defined.insert(d.0);
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
            if !local_defined.contains(&v.0) {
                uses.insert(v.0);
            }
        };
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => touch(*v, &mut uses[b]),
            Terminator::Branch {
                cond,
                then_blk,
                then_args,
                else_blk,
                else_args,
            } => {
                touch(*cond, &mut uses[b]);
                then_args
                    .iter()
                    .chain(else_args)
                    .for_each(|a| touch(*a, &mut uses[b]));
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
            end[v] = def[v];
        }
    }

    let mut base = 1 + nparams as u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let InstKind::LoadCaptured { var } | InstKind::StoreCaptured { var, .. } = &inst.kind
            {
                base = base.max(var_reg(*var, nparams) as u32 + 1);
            }
        }
    }
    let mut order: Vec<usize> = (0..nvals)
        .filter(|&v| !assigned[v] && def[v] != u32::MAX)
        .collect();
    order.sort_by_key(|&v| def[v]);
    // Type-aware linear scan: float values and non-float values draw from
    // separate free-register pools so no physical register is ever shared
    // between a float and a non-float value. Packing the two into one register
    // (their live ranges are disjoint, so a type-agnostic allocator would)
    // makes `derive_register_meta` meet their kinds to Dynamic, which erases
    // the float type the backend needs to route native f64. A pure-int
    // function never fills the float pool, so its allocation is unchanged.
    let is_float_v = |v: usize| {
        matches!(
            slot_kind_of(ssa.values[v].ty),
            varn_types::register_meta::SlotKind::Float
        )
    };
    let mut next: u32 = base;
    let mut free_float: Vec<u32> = Vec::new();
    let mut free_other: Vec<u32> = Vec::new();
    // (interval end, register, is_float)
    let mut active: Vec<(u32, u32, bool)> = Vec::new();
    for &v in &order {
        let d = def[v];
        let vf = is_float_v(v);
        let mut i = 0;
        while i < active.len() {
            if active[i].0 < d {
                let (_, r, rf) = active[i];
                if rf {
                    free_float.push(r);
                } else {
                    free_other.push(r);
                }
                active.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let pool = if vf { &mut free_float } else { &mut free_other };
        let r = match pool.iter().copied().min() {
            Some(m) => {
                pool.retain(|&x| x != m);
                m
            }
            None => {
                let r = next;
                next += 1;
                r
            }
        };
        if r > 255 {
            return Err(OptError::Unsupported(
                "ssa-emit: register count exceeds 255",
            ));
        }
        reg[v] = r as u8;
        assigned[v] = true;
        active.push((end[v], r, vf));
    }

    let mut max_call = 0u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            let t = match &inst.kind {
                InstKind::Call { args, .. } => args.len() as u32 + 1,
                InstKind::SelfCall { args } => args.len() as u32 + 1,
                InstKind::MethodCall { args, .. } => args.len() as u32,

                InstKind::BuildArray { elements } => elements.len() as u32,
                // Object literals stage non-contiguous values into the call
                // area before BuildObjectWithShape.
                InstKind::BuildObject { pairs } => pairs.len() as u32,

                // Reserved unconditionally, including for calls that end up on
                // the windowless `IntrinsicDirect` form. The reservation must
                // be an UPPER bound on what emission uses: over-reserving
                // wastes a frame slot, under-reserving would hand the emitted
                // window registers that overlap live values.
                InstKind::IntrinsicCall { args, .. } => args.len() as u32 + 1,
                InstKind::CallNativeOp { args, .. } => args.len() as u32 + 1,

                InstKind::IterCall { .. } => 1,

                InstKind::SuperCall { args } => args.len() as u32 + 2,
                InstKind::SuperMethodCall { args, .. } => args.len() as u32 + 1,

                InstKind::ExtensionCall { args, .. } => args.len() as u32 + 2,
                InstKind::CallSpread { args, .. } => args.len() as u32 + 1,
                _ => 0,
            };
            max_call = max_call.max(t);
        }
    }

    let total = next + 2 + max_call;
    if total > 256 {
        return Err(OptError::Unsupported(
            "ssa-emit: register count exceeds 255",
        ));
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

/// Which `ConstInt`s can ride along inside an arithmetic opcode.
///
/// `AddImm`/`SubImm` carry a signed 8-bit operand, so `i + 1` needs no
/// register for the `1` and no `LoadInt` to put it there. Both opcodes were
/// fully supported by the VM, the Cranelift lowering, the disassembler and
/// register allocation, and emitted by nothing.
pub(super) struct Immediates {
    /// The immediate a value can be folded to, if it is a small `ConstInt`.
    imm: Vec<Option<i8>>,
    /// Values whose every use folds, so their `LoadInt` is never emitted.
    elided: Vec<bool>,
}

impl Immediates {
    fn is_elided(&self, v: Value) -> bool {
        self.elided.get(v.0 as usize).copied().unwrap_or(false)
    }
}

/// An `Add`/`Sub` on proven ints is the only place an immediate can land.
/// `Add` is commutative so either side may carry it; `Sub` only the right,
/// since there is no reverse-subtract opcode.
fn immediate_operand(kind: &InstKind, imm: &[Option<i8>]) -> Option<(Value, Value, i8)> {
    let InstKind::Binary {
        op,
        lhs,
        rhs,
        ty: crate::hir::HirType::Int,
    } = kind
    else {
        return None;
    };
    let get = |v: &Value| -> Option<i8> { *imm.get(v.0 as usize)? };
    match op {
        crate::hir::HirBinOp::Add => {
            if let Some(i) = get(rhs) {
                Some((*rhs, *lhs, i))
            } else {
                get(lhs).map(|i| (*lhs, *rhs, i))
            }
        }
        crate::hir::HirBinOp::Sub => get(rhs).map(|i| (*rhs, *lhs, i)),
        _ => None,
    }
}

fn plan_immediates(ssa: &SsaFunc) -> Immediates {
    let n = ssa.values.len();
    let mut imm: Vec<Option<i8>> = vec![None; n];
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::ConstInt(i)) = (inst.dest, &inst.kind) {
                if let Ok(small) = i8::try_from(*i) {
                    imm[d.0 as usize] = Some(small);
                }
            }
        }
    }

    // A constant only disappears if *every* read of it folds. `c + c` reads
    // the same value twice and can only fold one side, so it keeps its
    // register — hence counting rather than a boolean.
    let mut total = vec![0u32; n];
    let mut folded = vec![0u32; n];
    for block in &ssa.blocks {
        for inst in &block.insts {
            super::uses::visit_uses(&inst.kind, &mut |v| total[v.0 as usize] += 1);
            if let Some((carrier, _, _)) = immediate_operand(&inst.kind, &imm) {
                folded[carrier.0 as usize] += 1;
            }
        }
        super::uses::visit_term_uses(&block.term, &mut |v| total[v.0 as usize] += 1);
    }

    let elided = (0..n)
        .map(|i| imm[i].is_some() && total[i] > 0 && total[i] == folded[i])
        .collect();
    Immediates { imm, elided }
}

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
    if let Some((_, other, value)) = immediate_operand(&inst.kind, &imms.imm) {
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
        chunk.emit_rrr(opcode, dest, reg[other.0 as usize], value as u8, LINE);
        return Ok(());
    }

    match &inst.kind {
        InstKind::SetProperty {
            object,
            name,
            value,
        } => {
            let idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.emit_rrc_ic(
                OpCode::SetProperty,
                reg[object.0 as usize],
                reg[value.0 as usize],
                idx,
                cs,
                LINE,
            );
            return Ok(());
        }
        InstKind::SetFixedField {
            object,
            value,
            slot,
        } => {
            chunk.emit_rrc(
                OpCode::SetFixedField,
                reg[object.0 as usize],
                reg[value.0 as usize],
                *slot,
                LINE,
            );
            return Ok(());
        }
        InstKind::SetIndex {
            object,
            index,
            value,
        } => {
            chunk.emit_rrr(
                OpCode::SetIndex,
                reg[object.0 as usize],
                reg[index.0 as usize],
                reg[value.0 as usize],
                LINE,
            );
            return Ok(());
        }
        InstKind::ArraySetIndex {
            object,
            index,
            value,
        } => {
            chunk.emit_rrr(
                OpCode::ArraySetIndex,
                reg[object.0 as usize],
                reg[index.0 as usize],
                reg[value.0 as usize],
                LINE,
            );
            return Ok(());
        }
        InstKind::ObjectMerge { target, source } => {
            chunk.emit_rr(
                OpCode::ObjectMerge,
                reg[target.0 as usize],
                reg[source.0 as usize],
                LINE,
            );
            return Ok(());
        }

        InstKind::AssertNotNull { operand } => {
            chunk.emit1(
                OpCode::AssertNotNull,
                Chunk::pack(reg[operand.0 as usize], 0),
                LINE,
            );
            return Ok(());
        }

        InstKind::StoreGlobal { name, value } => {
            let idx = chunk.add_str(name);
            chunk.emit_rrc(OpCode::DefineGlobal, 0, reg[value.0 as usize], idx, LINE);
            return Ok(());
        }

        InstKind::StoreUpvalue { index, value } => {
            chunk.emit1(
                OpCode::StoreUpvalue,
                Chunk::pack(*index as u8, reg[value.0 as usize]),
                LINE,
            );
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
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
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
        InstKind::DefineMethod {
            class,
            name,
            method,
            is_static,
        } => {
            let class_reg = reg[class.0 as usize];
            let method_reg = reg[method.0 as usize];
            let key_idx = chunk.add_str(name);
            let op = if *is_static {
                OpCode::DefineStatic
            } else {
                OpCode::Method
            };
            chunk.emit(op, LINE);
            chunk.write(Chunk::pack(class_reg, method_reg), LINE);
            chunk.write(key_idx, LINE);
            return Ok(());
        }
        InstKind::DefineAccessor {
            class,
            name,
            accessor,
            is_getter,
            is_static,
        } => {
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
            let op = if *b {
                OpCode::LoadTrue
            } else {
                OpCode::LoadFalse
            };
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
            // `+` with a statically-proven string operand IS concatenation.
            // That is exactly what `arith::add` works out at RUN TIME, one
            // type test at a time, on every single execution — and the
            // checker already proved it here. The binary's own `ty` is
            // `Dynamic` for the common `"literal" + int`, so specializing on
            // the result type alone never reaches this.
            let str_operand = matches!(op, crate::hir::HirBinOp::Add)
                && (matches!(
                    value_tys.get(lhs.0 as usize),
                    Some(crate::hir::HirType::Str)
                ) || matches!(
                    value_tys.get(rhs.0 as usize),
                    Some(crate::hir::HirType::Str)
                ));
            let opcode = if str_operand {
                OpCode::StrConcat
            } else {
                bin_opcode(*op, *ty)
            };
            chunk.emit_rrr(opcode, d, reg[lhs.0 as usize], reg[rhs.0 as usize], LINE);
        }
        InstKind::Unary { op, operand, .. } => {
            let s = reg[operand.0 as usize];
            match op {
                HirUnOp::Neg => chunk.emit_rr(OpCode::Negate, d, s, LINE),
                HirUnOp::Not => chunk.emit_rr(OpCode::Not, d, s, LINE),
                HirUnOp::Typeof => chunk.emit_rr(OpCode::Typeof, d, s, LINE),
                HirUnOp::BitNot => {
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

        InstKind::Call { callee, args } => {
            emit_call_args(chunk, reg, call_base, args);
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), LINE);
            chunk.write(Chunk::pack(total, call_base), LINE);
        }

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
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.emit_rrc_ic(
                OpCode::GetProperty,
                d,
                reg[object.0 as usize],
                idx,
                cs,
                LINE,
            );
        }
        InstKind::GetFixedField { object, slot } => {
            chunk.emit_rrc(
                OpCode::GetFixedField,
                d,
                reg[object.0 as usize],
                *slot,
                LINE,
            );
        }
        InstKind::GetIndex { object, index } => {
            chunk.emit_rrr(
                OpCode::GetIndex,
                d,
                reg[object.0 as usize],
                reg[index.0 as usize],
                LINE,
            );
        }
        InstKind::ArrayGetIndex { object, index } => {
            chunk.emit_rrr(
                OpCode::ArrayGetIndex,
                d,
                reg[object.0 as usize],
                reg[index.0 as usize],
                LINE,
            );
        }

        InstKind::MethodCall { recv, name, args } => {
            let name_idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
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

        InstKind::BuildArray { elements } => {
            for (i, e) in elements.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], LINE);
            }
            chunk.emit(OpCode::BuildArray, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(elements.len() as u8, 0), LINE);
        }

        InstKind::BuildTuple { elements } => {
            for (i, e) in elements.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], LINE);
            }
            chunk.emit(OpCode::BuildTuple, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(elements.len() as u8, 0), LINE);
        }

        InstKind::BuildObject { pairs } => {
            let count = pairs.len();
            let mut is_contiguous = count > 0;
            let mut start_reg = call_base;
            if count > 0 {
                let first = reg[pairs[0].1 .0 as usize];
                for (i, (_, v)) in pairs.iter().enumerate() {
                    if reg[v.0 as usize] != first + i as u8 {
                        is_contiguous = false;
                        break;
                    }
                }
                if is_contiguous {
                    start_reg = first;
                } else {
                    for (i, (_, v)) in pairs.iter().enumerate() {
                        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[v.0 as usize], LINE);
                    }
                }
            }
            let keys = pairs.iter().map(|(k, _)| k.clone()).collect();
            let shape_idx = chunk.add_shape(keys);
            chunk.emit(OpCode::BuildObjectWithShape, LINE);
            chunk.write(Chunk::pack(d, start_reg), LINE);
            chunk.write(shape_idx, LINE);
        }

        InstKind::BuildRecord { pairs } => {
            let count = pairs.len();
            let mut is_contiguous = count > 0;
            let mut start_reg = call_base;
            if count > 0 {
                let first = reg[pairs[0].1 .0 as usize];
                for (i, (_, v)) in pairs.iter().enumerate() {
                    if reg[v.0 as usize] != first + i as u8 {
                        is_contiguous = false;
                        break;
                    }
                }
                if is_contiguous {
                    start_reg = first;
                } else {
                    for (i, (_, v)) in pairs.iter().enumerate() {
                        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[v.0 as usize], LINE);
                    }
                }
            }
            let keys = pairs.iter().map(|(k, _)| k.clone()).collect();
            let shape_idx = chunk.add_shape(keys);
            chunk.emit(OpCode::BuildRecord, LINE);
            chunk.write(Chunk::pack(d, start_reg), LINE);
            chunk.write(shape_idx, LINE);
        }
        InstKind::ToString { operand } => {
            chunk.emit_rr(OpCode::ToString, d, reg[operand.0 as usize], LINE);
        }

        InstKind::MakeClosure {
            func,
            upvalues,
            upvalues_src,
        } => {
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
                for (_uv_val, uv_src) in upvalues.iter().zip(upvalues_src) {
                    let is_local = match uv_src {
                        HirUpvalueSrc::ParentLocal(_) | HirUpvalueSrc::ParentParam(_) => 1u8,
                        HirUpvalueSrc::ParentUpvalue(_) => 0u8,
                    };
                    let index = match uv_src {
                        HirUpvalueSrc::ParentLocal(id) => var_reg(VarId::Local(*id), nparams),
                        HirUpvalueSrc::ParentParam(i) => var_reg(VarId::Param(*i), nparams),
                        HirUpvalueSrc::ParentUpvalue(idx) => *idx as u8,
                    };
                    chunk.write(Chunk::pack(is_local, index), LINE);
                }
            }
        }

        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } if args.len() == 1
            && varn_core::intrinsic_ops::math::is_unary_math(*wire_byte)
            && matches!(
                value_tys.get(args[0].0 as usize),
                Some(crate::hir::HirType::Float)
            ) =>
        {
            // Direct form: no receiver staged, no window, no result Move.
            // `object` was already lowered (its side effects, if any, ran);
            // a unary math dispatch never reads it, so it simply stays in
            // its own register instead of being copied into a call slot.
            //
            // The float-argument requirement is semantic, not an
            // optimization: `intrinsics::math::dispatch` re-boxes an integral
            // result back to `int` when the argument was int-tagged, and the
            // direct form has no window for that path to round-trip through.
            // An int argument keeps the windowed encoding.
            let _ = object;
            chunk.write(Chunk::pack_op(OpCode::IntrinsicDirect, d), LINE);
            chunk.write(
                ((reg[args[0].0 as usize] as u16) << 8) | *wire_byte as u16,
                LINE,
            );
        }

        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 1 + i as u8,
                    reg[a.0 as usize],
                    LINE,
                );
            }
            let arg_count = (args.len() + 1) as u16;
            chunk.write(Chunk::pack_op(OpCode::Intrinsic, call_base), LINE);
            chunk.write(((*wire_byte as u16) << 8) | arg_count, LINE);
            chunk.emit_rr(OpCode::Move, d, call_base, LINE);
        }

        InstKind::CallNativeOp {
            object,
            args,
            op_id,
        } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 1 + i as u8,
                    reg[a.0 as usize],
                    LINE,
                );
            }
            // arg_count includes the receiver; op-id stored as a constant
            // (full i64) so it survives `.vnc` serialization.
            let arg_count = (args.len() + 1) as u16;
            let cidx = chunk.add_int(*op_id as i64);
            chunk.write(Chunk::pack_op(OpCode::CallNativeOp, call_base), LINE);
            chunk.write(cidx, LINE);
            chunk.write(arg_count, LINE);
            chunk.emit_rr(OpCode::Move, d, call_base, LINE);
        }

        InstKind::BuildStr { parts } => {
            chunk.write(Chunk::pack_op(OpCode::BuildStr, d), LINE);
            chunk.write(Chunk::pack(parts.len() as u8, 0), LINE);
            for p in parts {
                chunk.write(Chunk::pack(reg[p.0 as usize], 0), LINE);
            }
        }
        InstKind::GetPropertyMaybe { object, name } => {
            let idx = chunk.add_str(name);
            chunk.emit_rrc(
                OpCode::GetPropertyMaybe,
                d,
                reg[object.0 as usize],
                idx,
                LINE,
            );
        }
        InstKind::ModuleSlot { object, slot } => {
            chunk.emit_rrc(
                OpCode::LoadModuleSlot,
                d,
                reg[object.0 as usize],
                *slot,
                LINE,
            );
        }
        InstKind::GetEnumTag { operand } => {
            chunk.emit_rr(OpCode::GetEnumTag, d, reg[operand.0 as usize], LINE);
        }
        InstKind::IsArray { operand } => {
            chunk.emit_rr(OpCode::IsArray, d, reg[operand.0 as usize], LINE);
        }

        InstKind::This => chunk.emit_rr(OpCode::Move, d, 0, LINE),

        InstKind::Range {
            start,
            end,
            inclusive,
        } => {
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
            let sym = if *is_async {
                RuntimeSymbol::AsyncIterator
            } else {
                RuntimeSymbol::Iterator
            };
            let idx = chunk.add_symbol(sym);
            chunk.emit_rrc(OpCode::GetSymbol, d, reg[object.0 as usize], idx, LINE);
        }

        InstKind::IterCall { callee, recv } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[recv.0 as usize], LINE);
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), LINE);
            chunk.write(Chunk::pack(1, call_base), LINE);
        }

        InstKind::GetSuper { name } => {
            let idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::GetSuper, d, idx, LINE);
        }

        InstKind::SuperCall { args } => {
            let ctor_idx = chunk.add_str("constructor");
            chunk.emit_rc(OpCode::GetSuper, call_base, ctor_idx, LINE);
            chunk.emit_rr(OpCode::Move, call_base + 1, 0, LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 2 + i as u8,
                    reg[a.0 as usize],
                    LINE,
                );
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(call_base, call_base), LINE);
            chunk.write(Chunk::pack(total, call_base + 1), LINE);
            chunk.emit_rr(OpCode::Move, d, call_base + 1, LINE);
        }

        InstKind::SuperMethodCall { name, args } => {
            let name_idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::GetSuper, call_base, name_idx, LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 1 + i as u8,
                    reg[a.0 as usize],
                    LINE,
                );
            }
            let count = args.len() as u8;
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(
                Chunk::pack(count, if count > 0 { call_base + 1 } else { 0 }),
                LINE,
            );
        }

        InstKind::ExtensionCall { func, recv, args } => {
            let idx = chunk.add_str(func);
            chunk.emit_rc(OpCode::LoadGlobal, call_base, idx, LINE);
            chunk.emit_rr(OpCode::Move, call_base + 1, reg[recv.0 as usize], LINE);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 2 + i as u8,
                    reg[a.0 as usize],
                    LINE,
                );
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(total, call_base + 1), LINE);
        }

        InstKind::CallSpread { callee, args } => {
            chunk.emit_rr(OpCode::LoadNull, call_base, 0, LINE);
            for (i, (a, spread)) in args.iter().enumerate() {
                let op = if *spread {
                    OpCode::WrapSpread
                } else {
                    OpCode::Move
                };
                chunk.emit_rr(op, call_base + 1 + i as u8, reg[a.0 as usize], LINE);
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::CallSpread, LINE);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), LINE);
            chunk.write(Chunk::pack(total, call_base), LINE);
        }

        InstKind::BuildArraySpread { elements } => {
            chunk.emit(OpCode::BuildArray, LINE);
            chunk.write(Chunk::pack(d, call_base), LINE);
            chunk.write(Chunk::pack(0, 0), LINE);
            for (v, spread) in elements {
                let op = if *spread {
                    OpCode::ArrayExtend
                } else {
                    OpCode::ArrayPush
                };
                chunk.emit_rr(op, d, reg[v.0 as usize], LINE);
            }
        }

        InstKind::BuildObjectSpread { parts } => {
            chunk.emit(OpCode::BuildObject, LINE);
            chunk.write(Chunk::pack(d, 0), LINE);
            for (key, v) in parts {
                match key {
                    Some(k) => {
                        let idx = chunk.add_str(k);
                        if *cache_count > 255 {
                            return Err(OptError::Unsupported(
                                "ssa-emit: too many inline-cache sites",
                            ));
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
            // Same 2-word shape as Await: dest in the opcode word, task
            // register in the operand word (varn_types::bytecode).
            chunk.emit_rr(OpCode::Spawn, d, reg[operand.0 as usize], LINE);
        }
        InstKind::Yield { operand } => {
            chunk.emit1(OpCode::Yield, Chunk::pack(d, reg[operand.0 as usize]), LINE);
        }

        InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::ObjectMerge { .. }
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

fn emit_call_args(chunk: &mut Chunk, reg: &[u8], call_base: u8, args: &[Value]) {
    chunk.emit_rr(OpCode::LoadNull, call_base, 0, LINE);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 1 + i as u8,
            reg[a.0 as usize],
            LINE,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_terminator(
    chunk: &mut Chunk,
    ssa: &SsaFunc,
    reg: &[u8],
    cur_pos: usize,
    pos_of: &[usize],
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
            emit_goto(chunk, cur_pos, pos_of, *target, block_offset, fixups);
        }
        Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => {
            if !then_args.is_empty() || !else_args.is_empty() {
                return Err(OptError::Unsupported(
                    "ssa-emit: branch edge carries args after split",
                ));
            }
            emit_branch(
                chunk,
                cur_pos,
                pos_of,
                reg[cond.0 as usize],
                *then_blk,
                *else_blk,
                block_offset,
                fixups,
            )?;
        }
    }
    Ok(())
}

fn emit_edge_copies(
    chunk: &mut Chunk,
    ssa: &SsaFunc,
    reg: &[u8],
    target: BlockId,
    args: &[Value],
    scratch: u8,
) {
    let params = &ssa.blocks[target.0 as usize].params;
    let mut copies: Vec<(u8, u8)> = params
        .iter()
        .zip(args)
        .map(|(p, a)| (reg[p.0 as usize], reg[a.0 as usize]))
        .filter(|(d, s)| d != s)
        .collect();

    while !copies.is_empty() {
        if let Some(pos) = copies
            .iter()
            .position(|(d, _)| !copies.iter().any(|(_, s)| s == d))
        {
            let (d, s) = copies.remove(pos);
            chunk.emit_rr(OpCode::Move, d, s, LINE);
        } else {
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

fn emit_goto(
    chunk: &mut Chunk,
    cur_pos: usize,
    pos_of: &[usize],
    target: BlockId,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
) {
    let tp = pos_of[target.0 as usize];
    if tp == cur_pos + 1 {
        return;
    }
    if tp <= cur_pos {
        chunk.emit_loop(block_offset[target.0 as usize], LINE);
    } else {
        let pos = chunk.emit_jump(OpCode::Jump, LINE);
        fixups.push((pos, target));
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_branch(
    chunk: &mut Chunk,
    cur_pos: usize,
    pos_of: &[usize],
    cond: u8,
    then_blk: BlockId,
    else_blk: BlockId,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
) -> Result<()> {
    let tb = then_blk.0 as usize;
    let eb = else_blk.0 as usize;
    let then_fwd = pos_of[tb] > cur_pos;
    let else_fwd = pos_of[eb] > cur_pos;

    match (then_fwd, else_fwd) {
        (true, true) => {
            if pos_of[tb] == cur_pos + 1 {
                let pos = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, LINE);
                fixups.push((pos, else_blk));
            } else if pos_of[eb] == cur_pos + 1 {
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
            return Err(OptError::Unsupported(
                "ssa-emit: branch with both targets backward",
            ));
        }
    }
    Ok(())
}
