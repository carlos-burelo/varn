//! Bytecode → CLIF lowering for the typed, alloc-free subset (phase 5a).
//!
//! Lowered from BYTECODE, not from the compiler's SSA: cached `.vnc` runs
//! only have bytecode, and the typed opcode variants (`AddInt`, `LtInt`, …)
//! ARE the checker's serialized proofs. `cranelift-frontend` Variables (one
//! per VM register, all `I64`) rebuild SSA for free.
//!
//! Two functions per compilation, one buffer:
//! * the RAW function — unboxed `fn(i64 × nparams) -> i64`, the entire body
//!   in native registers, i48 wrap after every arith op, recursion as a
//!   direct hardware call to its own entry;
//! * the WRAPPER — the template JIT's `JitFn` ABI. It clears the
//!   caller-prepush flag (protocol: every JIT prologue consumes it), loads
//!   the boxed args from the VM stack, sign-extends the i48 payloads, calls
//!   the raw function and re-tags the result.
//!
//! Anything outside the subset bails to the template JIT — same authority
//! model: the `match` below is the complete support list.
//!
//! v1 limitations (documented, suite-gated): no native stack-limit guard
//! (deep CallSelf recursion aborts instead of raising the VM's depth
//! error), and back-edges carry no GC safepoint — sound here because the
//! subset admits no allocating ops, so a routed function cannot create GC
//! pressure.

use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, ExternalName, Function, InstBuilder, MemFlags, Signature,
    UserFuncName,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::{FunctionProto, VmValue};

use super::debug::ClifDebugSink;
use super::kinds::{apply_kinds, is_boxed_kind, kind_flow, K};
use crate::mem::JitBuffer;
use crate::JitHelpers;

use super::alloc::{self, AllocCtx};
use super::emit;
use super::methods;
use super::arrays;
use super::emit::{
    box_for_target, box_f64, box_int, box_or_pass, call_helper, def_const, def_const_bool, def_const_int,
    emit_array_payload, emit_return_value, meta_is_float, meta_is_int, patch_rel32, retag_raw_return,
    unbox_bool, unbox_f64_coerce, use_boxed, use_f64, use_int, wrap_i48,
};
use super::fields;
use super::floats;
use super::generic;
use super::globals;

/// Compiled artifact: `entry` (the wrapper, `JitFn` ABI) and `raw` (the
/// unboxed body, callable clif→clif) both point into `buffer`, which lives
/// as long as the owning `FunctionProto` — raw addresses handed to other
/// compilations stay valid.
pub struct ClifArtifact {
    pub buffer: JitBuffer,
    pub entry: *const u8,
    pub raw: *const u8,
    /// Whether `raw` takes the frame-aware ABI (extra base+closure params).
    /// Such a function must NOT be called through the clif→clif fast path
    /// (which assumes the bare `(exec_ctx, args)` ABI); the linker rejects it
    /// so the call takes the wrapper-based fallback instead.
    pub frame_aware: bool,
}

/// A statically linkable call target: the CURRENT closure a global slot
/// holds, bound to that closure's proto.
pub struct ClifTarget {
    /// Address of the callee proto's `clif_raw` cell — NOT the entry itself.
    /// The call site loads it on every call, so a callee compiled after its
    /// caller still gets called directly. `0` means "no direct entry yet"
    /// (uncompiled, failed, or frame-aware) and selects the fallback.
    pub raw_slot: usize,
    /// The exact boxed `VmValue` bits of the closure the link was made
    /// against. The call site guards on equality: a rebound (or GC-moved)
    /// global mismatches and takes the generic fallback — never a wrong
    /// call, at worst a slow one.
    pub expected_bits: u64,
    pub param_kinds: Vec<SlotKind>,
    pub return_kind: SlotKind,
}

/// VM-side resolver for clif→clif static calls. Implemented over the live
/// `ExecCtx` at compile time (globals are runtime state, so the JIT crate
/// cannot see them itself).
pub trait ClifLinker {
    fn static_target(&self, global_idx: usize) -> Option<ClifTarget>;
}

/// Linker that never links — used by paths without a VM context.
pub struct NoLinker;
impl ClifLinker for NoLinker {
    fn static_target(&self, _global_idx: usize) -> Option<ClifTarget> {
        None
    }
}

/// Why a lowering came out frame-aware, in the order the flag tests them,
/// plus `resume` when the body can hand control back to the INTERPRETER at a
/// bytecode ip (`Try`'s catch, `Yield`/`Await`'s suspension), which is what
/// reads registers back out of the home slots.
///
/// `frame_aware` is what zeroes `clif_raw` and so denies a function the direct
/// clif→clif entry. Sizing that gap needs the split: a function marked only
/// `alloc` is one a stack-map rooting model could set free, whereas one that
/// also says `resume` needs its home slots regardless.
pub fn frame_aware_reasons(proto: &FunctionProto) -> Vec<&'static str> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let mut r = Vec::new();
    if proto.has_this {
        r.push("this");
    }
    if alloc::has_alloc(code, pool).unwrap_or(true) {
        r.push("alloc");
    }
    if proto.upvalue_count > 0 {
        r.push("upvalue");
    }
    if proto.is_generator {
        r.push("generator");
    }
    if proto.is_async {
        r.push("async");
    }
    let mut ip = 0usize;
    while ip < code.len() {
        let Some(info) = decode(code, ip, pool) else {
            break;
        };
        if matches!(
            OpCode::from_u8(code[ip] as u8),
            Some(OpCode::Try) | Some(OpCode::Yield) | Some(OpCode::Await) | Some(OpCode::LoadModule)
        ) {
            r.push("resume");
            break;
        }
        ip += info.len;
    }
    r
}

pub fn try_compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
    let nparams = proto.arity.saturating_sub(1);
    if proto.param_kinds.len() != nparams {
        return Err("clif: missing param kinds".into());
    }
    floats::check_float_writes(&proto.chunk.code, &proto.chunk.constants, &proto.register_meta)?;

    let has_alloc = alloc::has_alloc(&proto.chunk.code, &proto.chunk.constants)?;
    let frame_aware = proto.has_this || has_alloc || proto.upvalue_count > 0 || proto.is_generator || proto.is_async;
    let raw = lower_raw(proto, constants, helpers, isa, linker, has_alloc, debug.as_deref_mut())?;
    let wrapper = build_wrapper(proto, helpers, isa, frame_aware)?;

    // Concatenate: raw at 0, wrapper 16-aligned after it, then resolve the
    // only two relocation targets we admit (self-recursion inside raw, and
    // the wrapper's call to raw) by hand.
    let wrapper_off = (raw.code.len() + 15) & !15;
    let total = wrapper_off + wrapper.code.len();
    let mut buf = JitBuffer::new(total.max(16))?;
    {
        let slice = buf.as_mut_slice();
        slice[..raw.code.len()].copy_from_slice(&raw.code);
        slice[wrapper_off..wrapper_off + wrapper.code.len()].copy_from_slice(&wrapper.code);
        for r in &raw.call_reloc_offsets {
            patch_rel32(slice, *r, 0);
        }
        for r in &wrapper.call_reloc_offsets {
            patch_rel32(slice, wrapper_off + *r, 0);
        }
    }
    super::debug::capture_code(&mut debug, &mut buf, raw.code.len(), wrapper_off, total);
    buf.make_executable()?;
    let raw_ptr = buf.as_ptr();
    let entry = unsafe { buf.as_ptr().add(wrapper_off) };
    Ok(ClifArtifact {
        buffer: buf,
        entry,
        raw: raw_ptr,
        frame_aware,
    })
}

struct CompiledPiece {
    code: Vec<u8>,
    /// Offsets of rel32 call displacements that must resolve to raw@0.
    call_reloc_offsets: Vec<usize>,
    /// `(bytecode ip, roots declared)` per emitted stack map, joined through
    /// the srclocs stamped in the dispatch loop. Empty unless roots were asked
    /// for — with no marking Cranelift emits no maps at all.
    stack_maps: Vec<(usize, usize)>,
    /// Maps whose PC fell in no stamped srcloc range.
    maps_unmatched: usize,
}

fn compile_piece(func: Function, isa: &OwnedTargetIsa) -> Result<CompiledPiece, String> {
    super::with_ctx(func, isa.as_ref(), |compiled| {
        let srclocs = compiled.buffer.get_srclocs_sorted();
        let mut stack_maps = Vec::new();
        let mut maps_unmatched = 0usize;
        for (offset, _, map) in compiled.buffer.user_stack_maps() {
            // The map's PC is the safepoint instruction. Attribute it to the
            // NEAREST PRECEDING stamped srcloc rather than to a range that
            // strictly contains it: regalloc interleaves spills and reloads
            // around a call, and those carry no srcloc of their own, so a
            // containment test drops the map even though the emitting opcode
            // is unambiguous.
            match srclocs
                .iter()
                .filter(|l| l.start <= *offset && !l.loc.is_default())
                .max_by_key(|l| l.start)
            {
                Some(l) => stack_maps.push((l.loc.bits() as usize, map.entries().count())),
                None => maps_unmatched += 1,
            }
        }
        let mut call_reloc_offsets = Vec::new();
        for reloc in compiled.buffer.relocs() {
            // The only symbol either piece may reference is user func 0 — the
            // raw function itself.
            match &reloc.target {
                cranelift_codegen::FinalizedRelocTarget::ExternalName(ExternalName::User(_)) => {
                    if reloc.addend != -4 {
                        return Err(format!("clif: unexpected reloc addend {}", reloc.addend));
                    }
                    call_reloc_offsets.push(reloc.offset as usize);
                }
                other => return Err(format!("clif: unsupported reloc target {other:?}")),
            }
        }
        Ok(CompiledPiece {
            code: compiled.code_buffer().to_vec(),
            call_reloc_offsets,
            stack_maps,
            maps_unmatched,
        })
    })
}

/// Raw signature: `fn(exec_ctx, [base, closure], arg × nparams) -> i64`.
/// Int-declared args arrive unboxed; everything else arrives as its boxed
/// VmValue bits. `exec_ctx` is only dereferenced by the heap-walking ops and
/// the slow helpers. Frame-aware functions (they allocate and/or take a
/// `this` receiver) carry two extra parameters: `base` (this frame's
/// register-0 index into `ctx.stack`, for flushing heap-typed registers to
/// their home slots at a safepoint and for reading the receiver from
/// `stack[base+0]`) and `closure` (this function's `VmClosure*`, needed by
/// shape-driven object construction).
fn raw_signature(nparams: usize, isa: &OwnedTargetIsa, frame_aware: bool) -> Signature {
    let mut sig = Signature::new(isa.default_call_conv());
    let extra = if frame_aware { 3 } else { 0 };
    for _ in 0..(1 + extra + nparams) {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

fn lower_raw(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    has_alloc: bool,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<CompiledPiece, String> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let nparams = proto.arity.saturating_sub(1);
    let nregs = proto.register_count as usize;
    let cc = isa.default_call_conv();
    let want_roots = debug.as_deref().is_some_and(|d| d.want_roots);
    // Frame-aware functions carry (base, closure) so they can flush heap refs
    // to ctx.stack home slots at a safepoint and read the `this` receiver
    // from stack[base+0]. A method/constructor needs it for the receiver even
    // when it never allocates.
    let frame_aware = proto.has_this || has_alloc || proto.upvalue_count > 0 || proto.is_generator || proto.is_async;

    // ---- scan: instruction starts, block starts (jump targets +
    // fall-through after conditional jumps) ----
    let mut block_starts: Vec<usize> = vec![0];
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        let op = OpCode::from_u8(code[ip] as u8).ok_or("clif: unknown opcode")?;
        let next = ip + info.len;
        match op {
            OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                block_starts.push(ip + 3 + off);
                if matches!(op, OpCode::JumpIfFalse | OpCode::JumpIfTrue) {
                    block_starts.push(next);
                }
            }
            OpCode::Loop => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                block_starts.push((ip + 3).saturating_sub(off));
            }
            _ => {}
        }
        ip = next;
    }
    block_starts.sort_unstable();
    block_starts.dedup();

    // ---- loop hoisting plan ----
    // Post-linearization (loop-aware RPO in ssa/emit) loops are CONTIGUOUS:
    // a `Loop` op at L targeting T delimits the region [T, L). For each
    // region, array receivers that are never redefined inside it get their
    // payload pointer resolved ONCE in the fall-through preheader into a
    // cache variable (0 = invalid — matching the template's loop_hoist
    // sentinel; no live allocation sits at address 0). Accesses test the
    // cache and skip the whole tag/generation/slot walk on hit. Sound for
    // the routed subset: nothing under a routed frame can run a GC, and an
    // append only mutates the payload's inner words, never the payload
    // pointer itself.
    // An ALLOCATING function may cache too, but only over a loop whose own
    // body allocates nothing. The pointer a cache holds is the address of
    // the receiver's heap slot, and any allocation can push that slot's Vec
    // (nursery or old gen) past its capacity and move it — a collection is
    // not the only way to invalidate one. An allocation-free region cannot
    // do that, and the collection that CAN happen at its back edge resets
    // every cache from the safepoint's taken arm
    // (`alloc::emit_backedge_safepoint`). That pair is the whole soundness
    // argument; `readonly` stays false here regardless, since the mid-end
    // must not hoist a resolve across the safepoint on its own.
    let mut regions: Vec<emit::Region> = Vec::new();
    let mut scan_ip = 0usize;
    while scan_ip < code.len() {
        let ip = scan_ip;
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if OpCode::from_u8(code[ip] as u8) == Some(OpCode::Loop) {
            let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
            let header = (ip + 3) - off;
            if header > 0 && (!has_alloc || !alloc::has_alloc(&code[header..ip], pool)?) {
                let mut receivers: Vec<usize> = Vec::new();
                let mut written: Vec<usize> = Vec::new();
                let mut redefined: Vec<usize> = Vec::new();
                let mut j = header;
                while j < ip {
                    let jinfo = decode(code, j, pool).ok_or("clif: undecodable opcode")?;
                    let jop = OpCode::from_u8(code[j] as u8).ok_or("clif: unknown opcode")?;
                    let dest = (code[j] >> 8) as usize;
                    match jop {
                        OpCode::ArrayGetIndex | OpCode::ArrayLength => {
                            receivers.push((code[j + 1] >> 8) as usize);
                            redefined.push(dest);
                        }
                        OpCode::ArraySetIndex => {
                            receivers.push(dest);
                            written.push(dest);
                        }
                        OpCode::CallSelf => redefined.push((code[j + 1] >> 8) as usize),
                        _ => {
                            if jinfo.def.is_some() {
                                redefined.push(dest);
                            }
                        }
                    }
                    j += jinfo.len;
                }
                receivers.sort_unstable();
                receivers.dedup();
                receivers.retain(|r| !redefined.contains(r));
                // A receiver the region only READS gets the stronger cache:
                // with no allocation in the region either, its element
                // pointer, length and repr are all loop-invariant.
                let read_only: Vec<usize> = receivers
                    .iter()
                    .copied()
                    .filter(|r| !written.contains(r))
                    .collect();
                if !receivers.is_empty() {
                    if super::trace() {
                        eprintln!(
                            "CLIF REGION {:?}: [{header}..{ip}] recv={receivers:?} ro={read_only:?}",
                            proto.name
                        );
                    }
                    regions.push((header, ip, receivers, read_only));
                }
            }
        }
        scan_ip += info.len;
    }

    // ---- build ----
    let mut func = Function::with_name_signature(
        UserFuncName::user(0, 0),
        raw_signature(nparams, isa, frame_aware),
    );
    let self_sig_ref = func.import_signature(raw_signature(nparams, isa, frame_aware));
    let self_name =
        func.declare_imported_user_function(cranelift_codegen::ir::UserExternalName::new(0, 0));
    let self_ref = func.import_function(cranelift_codegen::ir::ExtFuncData {
        name: cranelift_codegen::ir::ExternalName::user(self_name),
        signature: self_sig_ref,
        colocated: true,
    });

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

    // A float-typed register (`register_meta[r] == Float`) is an unboxed f64
    // in an `F64` Variable; every other register is a boxed/unboxed i64.
    let vars: Vec<Variable> = (0..nregs)
        .map(|r| {
            let ty = if meta_is_float(&proto.register_meta, r) {
                types::F64
            } else {
                types::I64
            };
            let v = b.declare_var(ty);
            // Marking has to happen before any use — Cranelift explicitly does
            // not retrofit pre-existing ones. Floats are never GC references.
            if want_roots && ty == types::I64 {
                b.declare_var_needs_stack_map(v);
            }
            v
        })
        .collect();
    // One payload-cache variable per (loop region, receiver register).
    // Zero-defined at entry like every var, and 0 means "not resolved":
    // the frontend's all-paths-defined rule and the sentinel share a def.
    let cache_vars: HashMap<(usize, usize), emit::RegionCache> = regions
        .iter()
        .flat_map(|(h, _, regs, ro)| regs.iter().map(move |r| ((*h, *r), ro.contains(r))))
        .map(|(k, read_only)| {
            let payload = b.declare_var(types::I64);
            let view = read_only.then(|| {
                [
                    b.declare_var(types::I64),
                    b.declare_var(types::I64),
                    b.declare_var(types::I64),
                ]
            });
            (k, emit::RegionCache { payload, view })
        })
        .collect();
    // Flat list for the safepoint, which invalidates all of them at once
    // and has no way to tell which region it sits in.
    let all_caches: Vec<Variable> = cache_vars
        .values()
        .flat_map(|c| {
            std::iter::once(c.payload).chain(c.view.into_iter().flatten())
        })
        .collect();

    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    // Every register defined up front: bytecode from the register allocator
    // never reads uninitialized slots, but the frontend requires a def on
    // every path; Cranelift's DCE removes the dead zeros.
    let zero = b.ins().iconst(types::I64, 0);
    let zero_f = b.ins().f64const(0.0);
    for (r, v) in vars.iter().enumerate() {
        // An F64 Variable must be zero-defined with an f64; DCE removes both.
        if meta_is_float(&proto.register_meta, r) {
            b.def_var(*v, zero_f);
        } else {
            b.def_var(*v, zero);
        }
    }
    for c in cache_vars.values() {
        b.def_var(c.payload, zero);
        for v in c.view.into_iter().flatten() {
            b.def_var(v, zero);
        }
    }
    let (exec_ctx, alloc_env) = if frame_aware {
        let closure = b.block_params(entry)[1];
        let base = b.block_params(entry)[2];
        let exec_ctx = b.block_params(entry)[3];
        (exec_ctx, Some((base, closure)))
    } else {
        let exec_ctx = b.block_params(entry)[0];
        (exec_ctx, None)
    };
    // Only a frame-aware function ever flushes, so only it needs the answer.
    let live = if alloc_env.is_some() {
        super::liveness::analyze(code, pool, nregs)
    } else {
        super::liveness::Liveness::everything()
    };
    let actx = alloc_env.map(|(base, closure)| AllocCtx {
        vars: vars.as_slice(),
        helpers,
        cc,
        exec_ctx,
        base,
        closure,
        nregs,
        register_meta: &proto.register_meta,
        live: &live,
        cur_ip: std::cell::Cell::new(0),
        safepoints: want_roots.then(|| std::cell::RefCell::new(Vec::new())),
    });
    let reg_offset = 1;
    for i in 0..nparams {
        let r = reg_offset + i;
        let param_idx = if frame_aware { 4 + i } else { 1 + i };
        let p = b.block_params(entry)[param_idx];
        if meta_is_float(&proto.register_meta, r) {
            let f = unbox_f64_coerce(&mut b, p);
            b.def_var(vars[r], f);
        } else if proto.param_kinds.get(i) == Some(&SlotKind::Int) && actx.is_some() {
            let un = wrap_i48(&mut b, p);
            b.def_var(vars[r], un);
        } else {
            b.def_var(vars[r], p);
        }
        if let Some(ref actx) = actx {
            let fb = alloc::frame_base_addr(&mut b, actx);
            b.ins().store(MemFlags::trusted(), p, fb, (r * 8) as i32);
        }
    }
    // A method/constructor receives `this` at register 0 — read it from its
    // ctx.stack home slot (stack[base+1]); the caller placed it there.
    if proto.has_this {
        if let Some(actx) = actx.as_ref() {
            let this = alloc::load_receiver(&mut b, actx);
            b.def_var(vars[0], this);
        }
    }
    let arr = arrays::ArrCtx {
        vars: vars.as_slice(),
        helpers,
        cc,
        exec_ctx,
        regions: &regions,
        cache_vars: &cache_vars,
        register_meta: &proto.register_meta,
        has_alloc,
    };
    let fld = fields::FldCtx {
        vars: vars.as_slice(),
        helpers,
        cc,
        exec_ctx,
        register_meta: &proto.register_meta,
    };
    let gbl = globals::GblCtx {
        vars: vars.as_slice(),
        helpers,
        exec_ctx,
        register_meta: &proto.register_meta,
    };
    let gen = generic::GenCtx {
        vars: vars.as_slice(),
        cc,
        exec_ctx,
        register_meta: &proto.register_meta,
    };

    let blocks: HashMap<usize, cranelift_codegen::ir::Block> = block_starts
        .iter()
        .map(|&s| (s, b.create_block()))
        .collect();
    if let Some(first) = blocks.get(&0) {
        b.ins().jump(*first, &[]);
    }

    let entries = kind_flow(
        code,
        pool,
        constants,
        &block_starts,
        nregs,
        &proto.param_kinds,
        &proto.register_meta,
        proto.has_this,
    )?;

    super::debug::capture_kinds(&mut debug, &entries, nregs);

    let mut state: Vec<K> = entries[&0].clone();
    let mut filled: Vec<usize> = Vec::new();
    let mut ip = 0usize;
    let mut terminated = true; // entry already jumped to block 0
    while ip < code.len() {
        if let Some(blk) = blocks.get(&ip) {
            if !terminated {
                // A fall-through edge is an edge like any other: the block's
                // entry kinds are the merge over ALL its predecessors, so the
                // registers must be converted into that representation before
                // jumping. Skipping this leaves, say, an unboxed loop counter
                // in a register the body then reads as boxed VmValue bits —
                // a raw small int reinterpreted as a denormal float.
                if let Some(e) = entries.get(&ip) {
                    box_for_target(&mut b, &proto.register_meta, &vars, &state, e);
                    state = e.clone();
                }
                // Falling through into a loop header: this block is the
                // preheader — resolve each planned receiver's payload into
                // its cache (sentinel 0 when the guard chain rejects).
                for (h, _, regs, _) in regions.iter().filter(|(h, _, _, _)| *h == ip) {
                    for &r in regs {
                        if state[r] != K::Boxed {
                            continue;
                        }
                        let obj = b.use_var(vars[r]);
                        let cache = cache_vars[&(*h, r)];
                        let invalid = b.create_block();
                        let done = b.create_block();
                        b.append_block_param(done, types::I64);
                        let payload = emit_array_payload(
                            &mut b,
                            exec_ctx,
                            obj,
                            &helpers.array_layout,
                            helpers.heap_field_offset,
                            invalid,
                            false,
                            true,
                        );
                        b.ins().jump(done, &[payload.into()]);
                        b.switch_to_block(invalid);
                        let z = b.ins().iconst(types::I64, 0);
                        b.ins().jump(done, &[z.into()]);
                        b.switch_to_block(done);
                        let resolved = b.block_params(done)[0];
                        b.def_var(cache.payload, resolved);
                        // Read-only receiver: hoist the three words behind the
                        // payload too. Loading them off an unresolved (0)
                        // payload would fault, so they are read on the
                        // resolved path and zeroed on the reject path — and
                        // `data == 0` is what the accesses test.
                        if let Some(view) = cache.view {
                            let lay = &helpers.array_layout;
                            let ok = b.ins().icmp_imm(IntCC::NotEqual, resolved, 0);
                            let load_blk = b.create_block();
                            let skip = b.create_block();
                            let merge = b.create_block();
                            for _ in 0..3 {
                                b.append_block_param(merge, types::I64);
                            }
                            b.ins().brif(ok, load_blk, &[], skip, &[]);

                            b.switch_to_block(load_blk);
                            let data = b.ins().load(
                                types::I64,
                                MemFlags::trusted(),
                                resolved,
                                (16 + lay.elems_ptr_off) as i32,
                            );
                            let len = b.ins().load(
                                types::I64,
                                MemFlags::trusted(),
                                resolved,
                                (16 + lay.elems_len_off) as i32,
                            );
                            let disc = super::emit::array_disc(&mut b, resolved, lay);
                            b.ins()
                                .jump(merge, &[data.into(), len.into(), disc.into()]);

                            b.switch_to_block(skip);
                            let z0 = b.ins().iconst(types::I64, 0);
                            b.ins().jump(merge, &[z0.into(), z0.into(), z0.into()]);

                            b.switch_to_block(merge);
                            for (i, v) in view.iter().enumerate() {
                                let p = b.block_params(merge)[i];
                                b.def_var(*v, p);
                            }
                        }
                    }
                }
                b.ins().jump(*blk, &[]);
            }
            // Both arms below set `terminated` for the block being entered.
            match entries.get(&ip) {
                Some(e) => {
                    b.switch_to_block(*blk);
                    filled.push(ip);
                    state = e.clone();
                    terminated = false;
                }
                None => {
                    // Block never reached by the dataflow: dead label
                    // (e.g. the emitter's LoadNull;Return tail). Skip its
                    // body; the block itself gets a trap filler below.
                    terminated = true;
                    let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
                    ip += info.len;
                    continue;
                }
            }
        } else if terminated {
            // unreachable filler between a terminator and the next label
            let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
            ip += info.len;
            continue;
        }

        let raw_op = code[ip];
        let first_reg = (raw_op >> 8) as usize;
        let op = OpCode::from_u8(raw_op as u8).ok_or("clif: unknown opcode")?;
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        let next_ip = ip + info.len;
        // Republish the point every emit arm below sizes its GC flush set at.
        if let Some(a) = actx.as_ref() {
            a.cur_ip.set(ip);
        }
        // Stamp the bytecode ip onto every instruction this opcode emits. It
        // is what later maps a stack map's PC back to an ip: the two sides are
        // keyed differently (ours by bytecode ip, Cranelift's by code offset)
        // and this is the only thing that joins them.
        if want_roots {
            b.set_srcloc(cranelift_codegen::ir::SourceLoc::new(ip as u32));
        }

        match op {
            OpCode::LoadIntZero => def_const_int(&mut b, &proto.register_meta, &vars, first_reg, 0),
            OpCode::LoadIntOne => def_const_int(&mut b, &proto.register_meta, &vars, first_reg, 1),
            OpCode::LoadIntMinusOne => def_const_int(&mut b, &proto.register_meta, &vars, first_reg, -1),
            OpCode::LoadTrue => def_const_bool(&mut b, &vars, first_reg, true),
            OpCode::LoadFalse => def_const_bool(&mut b, &vars, first_reg, false),
            OpCode::LoadInt => {
                let v = code[ip + 1] as i16 as i64;
                def_const_int(&mut b, &proto.register_meta, &vars, first_reg, v);
            }
            OpCode::LoadConst => {
                let idx = code[ip + 1] as usize;
                let c = *constants.get(idx).ok_or("clif: constant index")?;
                if meta_is_float(&proto.register_meta, first_reg) {
                    // A float-typed sink: load the constant as an unboxed f64
                    // (a float literal, or an int literal widened).
                    let f = if c.is_f64() {
                        b.ins().f64const(c.as_f64())
                    } else if c.is_int() {
                        b.ins().f64const(c.as_int() as f64)
                    } else {
                        return Err("clif: non-numeric const into float reg".into());
                    };
                    b.def_var(vars[first_reg], f);
                } else if c.is_int() {
                    def_const(&mut b, &vars, first_reg, c.as_int());
                } else if c.is_heap() && (c.as_heap_idx() & 0x8000_0000 == 0) {
                    // A nursery heap constant could be evacuated under a
                    // safepoint, invalidating an embedded index. Module
                    // constants are old-gen (bit 31 set) and stable; a
                    // nursery one is unexpected, so bail.
                    return Err("clif: nursery heap constant".into());
                } else {
                    // Stable value — an old-gen heap ref rooted by the module,
                    // or a non-heap immediate — embed its boxed bits directly.
                    def_const(&mut b, &vars, first_reg, c.0 as i64);
                }
            }
            OpCode::LoadNull => {
                // Real null VmValue (K::Boxed): call-staging slot (never read)
                // AND a genuine null operand, e.g. `x === null`.
                def_const(&mut b, &vars, first_reg, VmValue::null().0 as i64);
            }
            OpCode::Move => {
                let src = (code[ip + 1] >> 8) as usize;
                let src_is_float = meta_is_float(&proto.register_meta, src);
                let dest_is_float = meta_is_float(&proto.register_meta, first_reg);
                // The two registers can disagree on representation, and the
                // Variable's declared Cranelift type is fixed for the whole
                // function — so the copy CONVERTS rather than reinterprets.
                // Every non-float source reaches an f64 sink through its boxed
                // bits (`unbox_f64_coerce` widens a tagged int), never by
                // passing an I64 straight into an F64 Variable.
                let val_to_store = if dest_is_float && !src_is_float {
                    let v = box_or_pass(&mut b, &vars, &state, src);
                    let f = unbox_f64_coerce(&mut b, v);
                    b.def_var(vars[first_reg], f);
                    v
                } else if !dest_is_float && src_is_float {
                    let f = b.use_var(vars[src]);
                    let boxed = box_f64(&mut b, f);
                    b.def_var(vars[first_reg], boxed);
                    boxed
                } else {
                    let v = b.use_var(vars[src]);
                    b.def_var(vars[first_reg], v);
                    box_or_pass(&mut b, &vars, &state, src)
                };
                if let Some(ref actx) = actx {
                    let fb = alloc::frame_base_addr(&mut b, actx);
                    b.ins().store(MemFlags::trusted(), val_to_store, fb, (first_reg * 8) as i32);
                }
            }
            OpCode::AddInt | OpCode::SubInt | OpCode::MulInt => {
                let w1 = code[ip + 1];
                let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let s1 = use_int(&mut b, &vars, &state, r1)?;
                let s2 = use_int(&mut b, &vars, &state, r2)?;
                let r = match op {
                    OpCode::AddInt => b.ins().iadd(s1, s2),
                    OpCode::SubInt => b.ins().isub(s1, s2),
                    _ => b.ins().imul(s1, s2),
                };
                let w = wrap_i48(&mut b, r);
                b.def_var(vars[first_reg], w);
            }
            OpCode::Negate => {
                let src = (code[ip + 1] >> 8) as usize;
                if meta_is_float(&proto.register_meta, first_reg) {
                    // `use_f64` converts an `Int` operand and bails on
                    // anything else — unchanged, and the bail matters: it is
                    // what sends `(-a) + (-f)` shapes to the interpreter.
                    let f = use_f64(&mut b, &vars, &state, src)?;
                    let neg = b.ins().fneg(f);
                    b.def_var(vars[first_reg], neg);
                } else if state[src] == K::Int {
                    let i = b.use_var(vars[src]);
                    let neg = b.ins().ineg(i);
                    // Wrap like the interpreter's `make_int`, then BOX: the
                    // kind flow types this destination `Boxed` (see
                    // `apply_kinds`), and storing a raw payload into it left a
                    // register whose readers disagree about its representation.
                    let wrapped = wrap_i48(&mut b, neg);
                    let boxed = box_int(&mut b, wrapped);
                    b.def_var(vars[first_reg], boxed);
                } else {
                    // A boxed operand: an int, but equally a decimal or a
                    // bigint, whose negation ALLOCATES — which is exactly why
                    // `Negate` is in the `has_alloc` set. This used to take
                    // `use_int`, which ACCEPTS boxed registers, so `-(7.5d)`
                    // ran `ineg` over a heap-tagged VmValue's payload bits and
                    // produced a value that resolved to null.
                    let actx = actx.as_ref().ok_or("clif: Negate outside alloc fn")?;
                    let regs = alloc::live_boxed(actx, &state);
                    alloc::flush_boxed(&mut b, actx, &state, &regs);
                    let v = box_or_pass(&mut b, &vars, &state, src);
                    let res = call_helper(&mut b, cc, helpers.negate, &[exec_ctx, v]);
                    alloc::reload_boxed(&mut b, actx, &state, &regs);
                    alloc::def_result(&mut b, actx, first_reg, res);
                }
            }
            OpCode::AddImm | OpCode::SubImm => {
                let w1 = code[ip + 1];
                let src = (w1 >> 8) as usize;
                let imm = (w1 & 0xFF) as i8 as i64;
                let s = use_int(&mut b, &vars, &state, src)?;
                let r = if op == OpCode::AddImm {
                    b.ins().iadd_imm(s, imm)
                } else {
                    b.ins().iadd_imm(s, -imm)
                };
                let w = wrap_i48(&mut b, r);
                b.def_var(vars[first_reg], w);
            }
            OpCode::ModInt => {
                // Inline signed remainder. i48 operands can't hit the one
                // trapping case (i64::MIN % -1), so only a zero divisor
                // needs a guard: the cold branch calls the runtime helper,
                // which raises the proper VM error via longjmp.
                let w1 = code[ip + 1];
                let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let a = use_int(&mut b, &vars, &state, r1)?;
                let d = use_int(&mut b, &vars, &state, r2)?;
                let is_zero = b.ins().icmp_imm(IntCC::Equal, d, 0);
                let raise = b.create_block();
                let ok = b.create_block();
                let merge = b.create_block();
                b.append_block_param(merge, types::I64);
                b.ins().brif(is_zero, raise, &[], ok, &[]);
                b.switch_to_block(raise);
                let ba = box_int(&mut b, a);
                let bd = box_int(&mut b, d);
                let _ = call_helper(&mut b, cc, helpers.modulo, &[exec_ctx, ba, bd]);
                // helpers.modulo longjmps on a zero divisor and never
                // returns; the jump keeps the block well-formed.
                let z = b.ins().iconst(types::I64, 0);
                b.ins().jump(merge, &[z.into()]);
                b.switch_to_block(ok);
                let r = b.ins().srem(a, d);
                b.ins().jump(merge, &[r.into()]);
                b.switch_to_block(merge);
                let res = b.block_params(merge)[0];
                b.def_var(vars[first_reg], res);
            }
            OpCode::LtInt
            | OpCode::LteInt
            | OpCode::GtInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt => {
                let w1 = code[ip + 1];
                let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                if state[r1] == K::Int && state[r2] == K::Int {
                    let s1 = use_int(&mut b, &vars, &state, r1)?;
                    let s2 = use_int(&mut b, &vars, &state, r2)?;
                    let cc = match op {
                        OpCode::LtInt => IntCC::SignedLessThan,
                        OpCode::LteInt => IntCC::SignedLessThanOrEqual,
                        OpCode::GtInt => IntCC::SignedGreaterThan,
                        OpCode::GteInt => IntCC::SignedGreaterThanOrEqual,
                        OpCode::EqInt => IntCC::Equal,
                        OpCode::NeqInt => IntCC::NotEqual,
                        _ => unreachable!(),
                    };
                    let c = b.ins().icmp(cc, s1, s2);
                    let ext = b.ins().uextend(types::I64, c);
                    b.def_var(vars[first_reg], ext);
                } else {
                    let h_fn = match op {
                        OpCode::LtInt => helpers.lt,
                        OpCode::LteInt => helpers.lte,
                        OpCode::GtInt => helpers.gt,
                        OpCode::GteInt => helpers.gte,
                        OpCode::EqInt => helpers.eq,
                        OpCode::NeqInt => helpers.neq,
                        _ => unreachable!(),
                    };
                    generic::emit_compare(&mut b, &gen, &state, code, ip, h_fn);
                }
            }
            OpCode::Jump => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let target_ip = ip + 3 + off;
                let target = blocks[&target_ip];
                if let Some(target_state) = entries.get(&target_ip) {
                    box_for_target(&mut b, &proto.register_meta, &vars, &state, target_state);
                }
                b.ins().jump(target, &[]);
                terminated = true;
            }
            OpCode::Loop => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let target_ip = (ip + 3) - off;
                let target = blocks[&target_ip];
                // Allocating functions run a GC safepoint at the back-edge,
                // flushing live heap refs to their ctx.stack home slots so
                // the collector can root and rewrite them. (A frame-aware but
                // non-allocating function — e.g. a method — never fills the
                // nursery, so it skips this.)
                if has_alloc {
                    if let Some(actx) = actx.as_ref() {
                        alloc::emit_backedge_safepoint(&mut b, actx, &state, &all_caches);
                    }
                }
                if let Some(target_state) = entries.get(&target_ip) {
                    box_for_target(&mut b, &proto.register_meta, &vars, &state, target_state);
                }
                b.ins().jump(target, &[]);
                terminated = true;
            }
            OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                // brif tests non-zero, so an unboxed bool/int condition is used
                // directly; a boxed condition's truthiness is `!is_falsy` via
                // the logical_not helper.
                let cond = match state[first_reg] {
                    K::Bool | K::Int => b.use_var(vars[first_reg]),
                    k if is_boxed_kind(k) => {
                        let v = b.use_var(vars[first_reg]);
                        let falsy_boxed = call_helper(&mut b, cc, helpers.logical_not, &[exec_ctx, v]);
                        let falsy = unbox_bool(&mut b, falsy_boxed);
                        b.ins().bxor_imm(falsy, 1)
                    }
                    _ => {
                        if meta_is_int(&proto.register_meta, first_reg) {
                            b.use_var(vars[first_reg])
                        } else {
                            let v = box_or_pass(&mut b, &vars, &state, first_reg);
                            let falsy_boxed = call_helper(&mut b, cc, helpers.logical_not, &[exec_ctx, v]);
                            let falsy = unbox_bool(&mut b, falsy_boxed);
                            b.ins().bxor_imm(falsy, 1)
                        }
                    }
                };
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let target_ip = ip + 3 + off;
                let target = blocks[&target_ip];
                let fall = blocks[&next_ip];
                let target_trampoline = b.create_block();
                let fall_trampoline = b.create_block();
                if op == OpCode::JumpIfFalse {
                    b.ins().brif(cond, fall_trampoline, &[], target_trampoline, &[]);
                } else {
                    b.ins().brif(cond, target_trampoline, &[], fall_trampoline, &[]);
                }

                b.switch_to_block(target_trampoline);
                if let Some(target_state) = entries.get(&target_ip) {
                    box_for_target(&mut b, &proto.register_meta, &vars, &state, target_state);
                }
                b.ins().jump(target, &[]);

                b.switch_to_block(fall_trampoline);
                if let Some(fall_state) = entries.get(&next_ip) {
                    box_for_target(&mut b, &proto.register_meta, &vars, &state, fall_state);
                }
                b.ins().jump(fall, &[]);
                terminated = true;
            }
            OpCode::Return => {
                let src = (code[ip + 1] & 0xFF) as usize;
                let v = emit_return_value(&mut b, &vars, &state, proto.return_kind, src)?;
                b.ins().return_(&[v]);
                terminated = true;
            }
            OpCode::CallSelf => {
                let w1 = code[ip + 1];
                let w2 = code[ip + 2];
                let dest = (w1 >> 8) as usize;
                let arg_count = (w2 >> 8) as usize;
                let arg_start = (w2 & 0xFF) as usize;
                if arg_count != nparams + 1 {
                    return Err("clif: CallSelf arity mismatch".into());
                }
                let mut args = Vec::with_capacity(4 + nparams);
                if frame_aware {
                    let stack_ptr = b.block_params(entry)[0];
                    let closure_val = b.block_params(entry)[1];
                    let base_val = b.block_params(entry)[2];
                    args.push(stack_ptr);
                    args.push(closure_val);
                    args.push(base_val);
                }
                args.push(exec_ctx);
                for i in 0..nparams {
                    let r = arg_start + 1 + i;
                    let v = if proto.param_kinds.get(i) == Some(&SlotKind::Int) {
                        use_int(&mut b, &vars, &state, r)?
                    } else {
                        use_boxed(&mut b, &vars, &state, r)?
                    };
                    args.push(v);
                }
                let call = b.ins().call(self_ref, &args);
                let res = b.inst_results(call)[0];
                if meta_is_float(&proto.register_meta, dest) {
                    let f = unbox_f64_coerce(&mut b, res);
                    b.def_var(vars[dest], f);
                } else {
                    b.def_var(vars[dest], res);
                }
            }
            OpCode::LoadGlobalIdx => {
                globals::emit_load_global_idx(&mut b, &gbl, code, ip, first_reg);
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                globals::emit_store_global_idx(&mut b, &gbl, &state, code, ip)?;
            }
            OpCode::ArrayLength => {
                arrays::emit_array_length(&mut b, &arr, &state, code, ip, first_reg)?;
            }
            OpCode::ArrayGetIndex => {
                arrays::emit_array_get_index(&mut b, &arr, &state, code, ip, first_reg)?;
            }
            OpCode::ArraySetIndex => {
                arrays::emit_array_set_index(&mut b, &arr, &state, code, ip, first_reg)?;
            }
            OpCode::GetFixedField => {
                fields::emit_get_fixed_field(&mut b, &fld, &state, code, ip, first_reg)?;
            }
            OpCode::GetProperty => {
                let actx = actx.as_ref().ok_or("clif: GetProperty outside frame-aware fn")?;
                alloc::emit_get_property(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::SetProperty => {
                let actx = actx.as_ref().ok_or("clif: SetProperty outside frame-aware fn")?;
                alloc::emit_set_property(&mut b, actx, &state, code, ip);
            }
            OpCode::SetFixedField => {
                fields::emit_set_fixed_field(&mut b, &fld, &state, code, ip, first_reg)?;
            }
            OpCode::Call => {
                // `Call` is itself in the `has_alloc` set, so a function that
                // contains one is always frame-aware and always has `actx`.
                let actx = actx.as_ref().ok_or("clif: call in non-frame-aware fn")?;
                let callee_reg = (code[ip + 1] & 0xFF) as usize;
                let target = match state[callee_reg] {
                    K::Global(i) => linker.static_target(i as usize),
                    _ => None,
                };
                alloc::emit_call(&mut b, actx, &state, code, ip, target.as_ref())?;
            }
            OpCode::BuildArray => {
                let actx = actx.as_ref().ok_or("clif: BuildArray outside alloc fn")?;
                alloc::emit_build_array(&mut b, actx, &state, code, ip);
            }
            OpCode::ArrayPush => {
                let actx = actx.as_ref().ok_or("clif: ArrayPush outside alloc fn")?;
                alloc::emit_array_push(&mut b, actx, &state, code, ip);
            }
            OpCode::StrConcat => {
                let actx = actx.as_ref().ok_or("clif: StrConcat outside alloc fn")?;
                alloc::emit_str_concat(&mut b, actx, &state, code, ip);
            }
            OpCode::CallNativeOp => {
                let actx = actx.as_ref().ok_or("clif: CallNativeOp outside alloc fn")?;
                alloc::emit_call_native_op(&mut b, actx, &state, &proto.register_meta, code, pool, ip)?;
            }
            OpCode::CallMethod => {
                let actx = actx.as_ref().ok_or("clif: CallMethod outside alloc fn")?;
                methods::emit_call_method(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::InvokeVirtual => {
                let actx = actx.as_ref().ok_or("clif: InvokeVirtual outside alloc fn")?;
                methods::emit_invoke_virtual(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::MakeEnumVariant => {
                let actx = actx.as_ref().ok_or("clif: MakeEnumVariant outside alloc fn")?;
                alloc::emit_make_enum_variant(&mut b, actx, &state, code, ip);
            }
            OpCode::GetEnumTag => {
                let actx = actx.as_ref().ok_or("clif: GetEnumTag outside alloc fn")?;
                alloc::emit_get_enum_tag(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::BuildStr => {
                let actx = actx.as_ref().ok_or("clif: BuildStr outside alloc fn")?;
                alloc::emit_build_str(&mut b, actx, &state, code, ip);
            }
            OpCode::Intrinsic => {
                let actx = actx.as_ref().ok_or("clif: Intrinsic outside alloc fn")?;
                alloc::emit_intrinsic(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::ToString => {
                let actx = actx.as_ref().ok_or("clif: ToString outside alloc fn")?;
                alloc::emit_to_string(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::BuildObjectWithShape => {
                let actx = actx
                    .as_ref()
                    .ok_or("clif: BuildObjectWithShape outside alloc fn")?;
                let shape_idx = code[ip + 2] as usize;
                let count = proto
                    .resolved_shape(shape_idx)
                    .map(|s| s.property_names.len())
                    .ok_or("clif: unresolved object shape")?;
                alloc::emit_build_object_with_shape(&mut b, actx, &state, code, ip, count);
            }
            OpCode::LoadUpvalue => {
                let actx = actx.as_ref().ok_or("clif: LoadUpvalue outside frame-aware fn")?;
                alloc::emit_load_upvalue(&mut b, actx, code, ip);
            }
            OpCode::StoreUpvalue => {
                let actx = actx.as_ref().ok_or("clif: StoreUpvalue outside frame-aware fn")?;
                alloc::emit_store_upvalue(&mut b, actx, &state, code, ip);
            }
            OpCode::CloseUpvalue => {
                let actx = actx.as_ref().ok_or("clif: CloseUpvalue outside frame-aware fn")?;
                alloc::emit_close_upvalue(&mut b, actx, &state, code, ip);
            }
            OpCode::MakeClosure => {
                let actx = actx.as_ref().ok_or("clif: MakeClosure outside alloc fn")?;
                alloc::emit_make_closure(&mut b, actx, &state, code, ip);
            }
            OpCode::LoadStaticFn => {
                let actx = actx.as_ref().ok_or("clif: LoadStaticFn outside alloc fn")?;
                alloc::emit_load_static_fn(&mut b, actx, &state, code, ip);
            }
            OpCode::LoadModule => {
                let actx = actx.as_ref().ok_or("clif: LoadModule outside alloc fn")?;
                alloc::emit_load_module(&mut b, actx, &state, code, ip);
            }
            OpCode::LoadModuleSlot => {
                let actx = actx.as_ref().ok_or("clif: LoadModuleSlot outside alloc fn")?;
                alloc::emit_load_module_slot(&mut b, actx, &state, code, ip);
            }
            OpCode::StoreModuleSlot => {
                let actx = actx.as_ref().ok_or("clif: StoreModuleSlot outside alloc fn")?;
                alloc::emit_store_module_slot(&mut b, actx, &state, code, ip);
            }
            OpCode::MakeClass => {
                let actx = actx.as_ref().ok_or("clif: MakeClass outside alloc fn")?;
                alloc::emit_make_class(&mut b, actx, &state, code, ip);
            }
            OpCode::DeclareField
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::Inherit => {
                let actx = actx.as_ref().ok_or("clif: ClassMemberOp outside alloc fn")?;
                alloc::emit_class_member_op(&mut b, actx, &state, op, code, ip)?;
            }
            OpCode::GetSuper => {
                let actx = actx.as_ref().ok_or("clif: GetSuper outside alloc fn")?;
                alloc::emit_get_super(&mut b, actx, &state, code, ip);
            }
            OpCode::LoadGlobal => {
                let actx = actx.as_ref().ok_or("clif: LoadGlobal outside alloc fn")?;
                alloc::emit_load_global(&mut b, actx, &state, code, ip);
            }
            OpCode::StoreGlobal => {
                let actx = actx.as_ref().ok_or("clif: StoreGlobal outside alloc fn")?;
                alloc::emit_global_write(&mut b, actx, &state, helpers.store_global, code, ip);
            }
            OpCode::DefineGlobal => {
                let actx = actx.as_ref().ok_or("clif: DefineGlobal outside alloc fn")?;
                alloc::emit_global_write(&mut b, actx, &state, helpers.define_global, code, ip);
            }
            OpCode::GetIndex => {
                let actx = actx.as_ref().ok_or("clif: GetIndex outside alloc fn")?;
                alloc::emit_get_index(&mut b, actx, &state, code, ip);
            }
            OpCode::SetIndex => {
                let actx = actx.as_ref().ok_or("clif: SetIndex outside alloc fn")?;
                alloc::emit_set_index(&mut b, actx, &state, code, ip);
            }
            OpCode::Try => {
                let actx = actx.as_ref().ok_or("clif: Try outside alloc fn")?;
                alloc::emit_try_push(&mut b, actx, code, ip);
            }
            OpCode::PopTry => {
                let actx = actx.as_ref().ok_or("clif: PopTry outside alloc fn")?;
                alloc::emit_try_pop(&mut b, actx);
            }
            OpCode::Throw => {
                let actx = actx.as_ref().ok_or("clif: Throw outside alloc fn")?;
                alloc::emit_throw(&mut b, actx, &state, code, ip);
            }
            OpCode::Yield => {
                let actx = actx.as_ref().ok_or("clif: Yield outside alloc fn")?;
                alloc::emit_yield(&mut b, actx, &state, code, ip);
            }
            OpCode::Await => {
                let actx = actx.as_ref().ok_or("clif: Await outside alloc fn")?;
                alloc::emit_await(&mut b, actx, &state, code, ip);
            }
            OpCode::Spawn => {
                let actx = actx.as_ref().ok_or("clif: Spawn outside alloc fn")?;
                alloc::emit_spawn(&mut b, actx, &state, code, ip);
            }
            OpCode::BuildObject => {
                let actx = actx.as_ref().ok_or("clif: BuildObject outside alloc fn")?;
                alloc::emit_build_object(&mut b, actx, &state, code, ip);
            }
            OpCode::ObjectRest => {
                let actx = actx.as_ref().ok_or("clif: ObjectRest outside alloc fn")?;
                alloc::emit_object_rest(&mut b, actx, &state, code, ip);
            }
            OpCode::ObjectKeys => {
                let actx = actx.as_ref().ok_or("clif: ObjectKeys outside alloc fn")?;
                alloc::emit_object_keys(&mut b, actx, &state, code, ip);
            }
            OpCode::ObjectMerge => {
                let actx = actx.as_ref().ok_or("clif: ObjectMerge outside alloc fn")?;
                alloc::emit_object_merge(&mut b, actx, &state, code, ip);
            }
            OpCode::GetPropertyMaybe => {
                let actx = actx.as_ref().ok_or("clif: GetPropertyMaybe outside alloc fn")?;
                alloc::emit_get_property_maybe(&mut b, actx, &state, code, ip);
            }
            OpCode::BindMethod => {
                let actx = actx.as_ref().ok_or("clif: BindMethod outside alloc fn")?;
                alloc::emit_bind_method(&mut b, actx, &state, code, ip);
            }
            OpCode::AssertNotNull => {
                let actx = actx.as_ref().ok_or("clif: AssertNotNull outside alloc fn")?;
                alloc::emit_assert_not_null(&mut b, actx, &state, code, ip);
            }
            OpCode::ArrayExtend => {
                let actx = actx.as_ref().ok_or("clif: ArrayExtend outside alloc fn")?;
                alloc::emit_array_extend(&mut b, actx, &state, code, ip);
            }
            OpCode::WrapSpread => {
                let actx = actx.as_ref().ok_or("clif: WrapSpread outside alloc fn")?;
                alloc::emit_wrap_spread(&mut b, actx, &state, code, ip);
            }
            OpCode::CallSpread => {
                let actx = actx.as_ref().ok_or("clif: CallSpread outside alloc fn")?;
                alloc::emit_call_spread(&mut b, actx, &state, &proto.register_meta, code, ip);
            }
            OpCode::InvokeRuntimeStatic => {
                let actx = actx.as_ref().ok_or("clif: InvokeRuntimeStatic outside alloc fn")?;
                alloc::emit_invoke_runtime_static(&mut b, actx, &state, code, ip)?;
            }
            OpCode::Nop => {}
            // Typed float ops: native fadd/fsub/fmul/fdiv/fcmp when operands
            // are float (or int-coercible); Mod/Pow via the float-boxing
            // helper. A non-eligible case falls back to the generic helper.
            OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat => {
                if !floats::emit_float_op(
                    &mut b,
                    &vars,
                    &state,
                    &proto.register_meta,
                    code,
                    ip,
                    op,
                    cc,
                    exec_ctx,
                    helpers,
                )? && !generic::try_emit(&mut b, &gen, helpers, &state, op, code, ip)
                {
                    return Err(format!("clif: unsupported opcode {op:?}"));
                }
            }
            // Generic (helper-based) arithmetic / comparisons / unary ops.
            _ if generic::try_emit(&mut b, &gen, helpers, &state, op, code, ip) => {}
            _ => return Err(format!("clif: unsupported opcode {op:?}")),
        }
        apply_kinds(&mut state, code, pool, ip, op, constants, &proto.register_meta);
        ip = next_ip;
    }
    if !terminated {
        let z = b.ins().iconst(types::I64, 0);
        b.ins().return_(&[z]);
    }

    // Labels the dataflow never reached (dead code, e.g. the emitter's
    // LoadNull;Return tail) still own a Cranelift block; give each a
    // terminator so the verifier is satisfied.
    for (&s, blk) in blocks.iter() {
        if !filled.contains(&s) {
            b.switch_to_block(*blk);
            b.ins().trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
        }
    }

    b.seal_all_blocks();
    b.finalize();
    super::debug::capture_ir(&mut debug, &func);
    let piece = compile_piece(func, isa)?;
    if let Some(rec) = actx.as_ref().and_then(|a| a.safepoints.as_ref()) {
        super::debug::capture_roots(
            &mut debug,
            &rec.borrow(),
            &piece.stack_maps,
            piece.maps_unmatched,
            |ip| match OpCode::from_u8(code[ip] as u8) {
                Some(op) => format!("{op:?}"),
                None => "?".to_owned(),
            },
        );
    }
    Ok(piece)
}


/// Wrapper with the template `JitFn` ABI:
/// `(stack_ptr, closure, base, exec_ctx) -> boxed VmValue`.
fn build_wrapper(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    frame_aware: bool,
) -> Result<CompiledPiece, String> {
    let nparams = proto.arity.saturating_sub(1);
    let mut sig = Signature::new(isa.default_call_conv());
    for _ in 0..4 {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let mut func = Function::with_name_signature(UserFuncName::user(0, 1), sig);
    let raw_sig = func.import_signature(raw_signature(nparams, isa, frame_aware));
    let raw_name =
        func.declare_imported_user_function(cranelift_codegen::ir::UserExternalName::new(0, 0));
    let raw_ref = func.import_function(cranelift_codegen::ir::ExtFuncData {
        name: cranelift_codegen::ir::ExternalName::user(raw_name),
        signature: raw_sig,
        colocated: true,
    });

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    b.seal_block(block);

    let (stack_ptr, closure, base, exec_ctx) = {
        let p = b.block_params(block);
        (p[0], p[1], p[2], p[3])
    };

    // Protocol: every JIT prologue consumes the caller-prepush flag.
    let zero32 = b.ins().iconst(types::I64, 0);
    b.ins().store(
        MemFlags::trusted(),
        zero32,
        exec_ctx,
        helpers.frame_prepushed_offset as i32,
    );

    // Boxed args live at stack[base + 1 + i]. Int-declared params unbox
    // (the shl/sar pair is exactly the interpreter's typed read); anything
    // else passes through as boxed bits per the raw entry contract.
    let base_bytes = b.ins().imul_imm(base, 8);
    let arg_base = b.ins().iadd(stack_ptr, base_bytes);
    let mut args = Vec::with_capacity(4 + nparams);
    if frame_aware {
        args.push(stack_ptr);
        args.push(closure);
        args.push(base);
    }
    args.push(exec_ctx);
    for i in 0..nparams {
        let boxed = b
            .ins()
            .load(types::I64, MemFlags::trusted(), arg_base, ((1 + i) * 8) as i32);
        if proto.param_kinds.get(i) == Some(&SlotKind::Int) {
            let sh = b.ins().ishl_imm(boxed, 16);
            let un = b.ins().sshr_imm(sh, 16);
            args.push(un);
        } else {
            args.push(boxed);
        }
    }

    let call = b.ins().call(raw_ref, &args);
    let raw_res = b.inst_results(call)[0];

    // Only an int return comes back as an unboxed i48 payload to re-tag.
    // Every other admitted return (string/ref/dynamic, or a constructor's
    // null) is already boxed VmValue bits — pass through. Re-tagging a null
    // would forge a non-null value and defeat jit_construct_fast's null check.
    let result = retag_raw_return(&mut b, raw_res, proto.return_kind);
    b.ins().return_(&[result]);
    b.finalize();
    compile_piece(func, isa)
}
