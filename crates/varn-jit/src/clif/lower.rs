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

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, ExternalName, Function, InstBuilder, MemFlags, Signature,
    UserFuncName,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::{FunctionProto, VmValue};

use super::kinds::{apply_kinds, is_boxed_kind, kind_flow, K};
use crate::mem::JitBuffer;
use crate::JitHelpers;

use super::alloc::{self, AllocCtx};
use super::arrays;
use super::emit::{
    box_int, call_helper, code_has_array_ops, def_const, emit_array_payload, state_meta_int,
    use_boxed, use_int, wrap_i48, INT_TAG, MASK_48,
};
use super::fields;

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
/// holds, when that closure is clif-compiled with an Int-proven contract.
pub struct ClifTarget {
    /// Raw-function entry address.
    pub raw: usize,
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

pub fn try_compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> Result<ClifArtifact, String> {
    if proto.is_generator || proto.is_async || proto.has_rest {
        return Err("clif: unsupported function kind".into());
    }
    if proto.upvalue_count > 0 {
        return Err("clif: upvalues not supported".into());
    }
    let nparams = proto.arity.saturating_sub(1);
    // Int params enter unboxed; anything else passes through boxed
    // (`K::Boxed`). No declared kinds at all (pre-param_kinds caches)
    // means we cannot prove the entry contract.
    if proto.param_kinds.len() != nparams {
        return Err("clif: missing param kinds".into());
    }

    let has_alloc = alloc::has_alloc(&proto.chunk.code, &proto.chunk.constants)?;
    let frame_aware = proto.has_this || has_alloc;
    let raw = lower_raw(proto, constants, helpers, isa, linker, has_alloc)?;
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

/// `site` is the buffer offset of the rel32 field; `target` the buffer
/// offset the call must reach.
fn patch_rel32(buf: &mut [u8], site: usize, target: usize) {
    let disp = target as i64 - (site as i64 + 4);
    let disp = i32::try_from(disp).expect("clif: rel32 out of range");
    buf[site..site + 4].copy_from_slice(&disp.to_le_bytes());
}

struct CompiledPiece {
    code: Vec<u8>,
    /// Offsets of rel32 call displacements that must resolve to raw@0.
    call_reloc_offsets: Vec<usize>,
}

fn compile_piece(func: Function, isa: &OwnedTargetIsa) -> Result<CompiledPiece, String> {
    let mut ctx = Context::for_function(func);
    let compiled = ctx
        .compile(isa.as_ref(), &mut ControlPlane::default())
        .map_err(|e| format!("clif compile: {e:?}"))?;
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
    let extra = if frame_aware { 2 } else { 0 };
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
) -> Result<CompiledPiece, String> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let nparams = proto.arity.saturating_sub(1);
    let nregs = proto.register_count as usize;
    let cc = isa.default_call_conv();
    // Frame-aware functions carry (base, closure) so they can flush heap refs
    // to ctx.stack home slots at a safepoint and read the `this` receiver
    // from stack[base+0]. A method/constructor needs it for the receiver even
    // when it never allocates.
    let frame_aware = proto.has_this || has_alloc;

    // Cross-function static calls (the `Call` arm) are only sound when no
    // heap pointer is live across the call — a GC under the fallback could
    // move it. In the routed subset, the only boxed values are callee-fn
    // loads consumed immediately by their own call; the presence of ANY
    // array op (whose payload pointers are cached across loop bodies) or a
    // boxed parameter breaks that, so we forbid `Call` in those functions.
    let array_free = !code_has_array_ops(code, pool)?;
    let all_int_params = proto.param_kinds.iter().all(|k| *k == SlotKind::Int);
    // Cross-function static calls are not yet threaded through the
    // frame-aware safepoint discipline, so an allocating function forbids
    // them (they bail to the template) alongside the existing array/boxed
    // constraints.
    let calls_allowed = array_free && all_int_params && !has_alloc;

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
    // Loop payload caching is unsound in an allocating function: a
    // back-edge safepoint can grow the old-gen Vec or move the array, so
    // a cached data pointer would dangle. Such functions re-resolve every
    // access instead (see `readonly = !has_alloc` below).
    let mut regions: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    if !has_alloc {
        let mut ip = 0usize;
        while ip < code.len() {
            let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
            if OpCode::from_u8(code[ip] as u8) == Some(OpCode::Loop) {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let header = (ip + 3) - off;
                if header > 0 {
                    let mut receivers: Vec<usize> = Vec::new();
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
                            OpCode::ArraySetIndex => receivers.push(dest),
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
                    if !receivers.is_empty() {
                        regions.push((header, ip, receivers));
                    }
                }
            }
            ip += info.len;
        }
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

    let vars: Vec<Variable> = (0..nregs).map(|_| b.declare_var(types::I64)).collect();
    // One payload-cache variable per (loop region, receiver register).
    // Zero-defined at entry like every var, and 0 means "not resolved":
    // the frontend's all-paths-defined rule and the sentinel share a def.
    let cache_vars: HashMap<(usize, usize), Variable> = regions
        .iter()
        .flat_map(|(h, _, regs)| regs.iter().map(move |r| (*h, *r)))
        .map(|k| (k, b.declare_var(types::I64)))
        .collect();

    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    // Every register defined up front: bytecode from the register allocator
    // never reads uninitialized slots, but the frontend requires a def on
    // every path; Cranelift's DCE removes the dead zeros.
    let zero = b.ins().iconst(types::I64, 0);
    for v in &vars {
        b.def_var(*v, zero);
    }
    for v in cache_vars.values() {
        b.def_var(*v, zero);
    }
    let exec_ctx = b.block_params(entry)[0];
    // Frame-aware functions carry `base` and `closure` as the second and
    // third raw parameters; args follow them.
    let alloc_env = if frame_aware {
        Some((b.block_params(entry)[1], b.block_params(entry)[2]))
    } else {
        None
    };
    let arg_off = if frame_aware { 3 } else { 1 };
    for i in 0..nparams {
        let p = b.block_params(entry)[arg_off + i];
        b.def_var(vars[1 + i], p);
    }

    let actx = alloc_env.map(|(base, closure)| AllocCtx {
        vars: vars.as_slice(),
        helpers,
        cc,
        exec_ctx,
        base,
        closure,
        nregs,
    });
    // A method/constructor receives `this` at register 0 — read it from its
    // ctx.stack home slot (stack[base+0]); the caller placed it there.
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

    let mut state: Vec<K> = entries[&0].clone();
    let mut filled: Vec<usize> = Vec::new();
    let mut ip = 0usize;
    let mut terminated = true; // entry already jumped to block 0
    while ip < code.len() {
        if let Some(blk) = blocks.get(&ip) {
            if !terminated {
                // Falling through into a loop header: this block is the
                // preheader — resolve each planned receiver's payload into
                // its cache (sentinel 0 when the guard chain rejects).
                for (h, _, regs) in regions.iter().filter(|(h, _, _)| *h == ip) {
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
                        b.def_var(cache, resolved);
                    }
                }
                b.ins().jump(*blk, &[]);
            }
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

        match op {
            OpCode::LoadIntZero => def_const(&mut b, &vars, first_reg, 0),
            OpCode::LoadIntOne => def_const(&mut b, &vars, first_reg, 1),
            OpCode::LoadIntMinusOne => def_const(&mut b, &vars, first_reg, -1),
            OpCode::LoadInt => {
                let v = code[ip + 1] as i16 as i64;
                def_const(&mut b, &vars, first_reg, v);
            }
            OpCode::LoadConst => {
                let idx = code[ip + 1] as usize;
                let c = *constants.get(idx).ok_or("clif: constant index")?;
                if c.is_int() {
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
                // Call-staging callee slot; Poison in the kind lattice, so
                // any real read of it bails.
                def_const(&mut b, &vars, first_reg, 0);
            }
            OpCode::Move => {
                let src = (code[ip + 1] >> 8) as usize;
                // Any concrete kind copies by bits (ints, bools, and boxed
                // heap refs alike); only bottom/top/poison are unmovable.
                if matches!(state[src], K::Unset | K::Poison | K::Mixed) {
                    return Err("clif: move of untracked kind".into());
                }
                let v = b.use_var(vars[src]);
                b.def_var(vars[first_reg], v);
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
                let s1 = use_int(&mut b, &vars, &state, r1)?;
                let s2 = use_int(&mut b, &vars, &state, r2)?;
                let cc = match op {
                    OpCode::LtInt => IntCC::SignedLessThan,
                    OpCode::LteInt => IntCC::SignedLessThanOrEqual,
                    OpCode::GtInt => IntCC::SignedGreaterThan,
                    OpCode::GteInt => IntCC::SignedGreaterThanOrEqual,
                    OpCode::EqInt => IntCC::Equal,
                    _ => IntCC::NotEqual,
                };
                let c = b.ins().icmp(cc, s1, s2);
                let ext = b.ins().uextend(types::I64, c);
                b.def_var(vars[first_reg], ext);
            }
            OpCode::Jump => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let target = blocks[&(ip + 3 + off)];
                b.ins().jump(target, &[]);
                terminated = true;
            }
            OpCode::Loop => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let target = blocks[&((ip + 3) - off)];
                // Allocating functions run a GC safepoint at the back-edge,
                // flushing live heap refs to their ctx.stack home slots so
                // the collector can root and rewrite them. (A frame-aware but
                // non-allocating function — e.g. a method — never fills the
                // nursery, so it skips this.)
                if has_alloc {
                    if let Some(actx) = actx.as_ref() {
                        alloc::emit_backedge_safepoint(&mut b, actx, &state);
                    }
                }
                b.ins().jump(target, &[]);
                terminated = true;
            }
            OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                if state[first_reg] != K::Bool {
                    return Err("clif: branch on non-bool".into());
                }
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                let target = blocks[&(ip + 3 + off)];
                let fall = blocks[&next_ip];
                let cond = b.use_var(vars[first_reg]);
                if op == OpCode::JumpIfFalse {
                    b.ins().brif(cond, fall, &[], target, &[]);
                } else {
                    b.ins().brif(cond, target, &[], fall, &[]);
                }
                terminated = true;
            }
            OpCode::Return => {
                let src = (code[ip + 1] & 0xFF) as usize;
                // A reachable null return (a constructor's implicit
                // `return null`; the int-return tail's LoadNull;Return is dead
                // and trap-filled, never reaching here). Emit the null bits.
                let v = if state[src] == K::Poison {
                    b.ins().iconst(types::I64, VmValue::null().0 as i64)
                } else {
                    match proto.return_kind {
                    // Int return: the raw yields an unboxed i48 payload, which
                    // the wrapper (and the clif→clif fast call) re-tag. The
                    // source must be int by PROOF — kind Int, or boxed with a
                    // DECLARED int return (checker-enforced at typed call
                    // sites). An untyped identity arrow has neither and bails,
                    // so a Boxed return can't forge an int from a heap ref.
                    SlotKind::Int => match state[src] {
                        K::Int => b.use_var(vars[src]),
                        K::Boxed => use_int(&mut b, &vars, &state, src)?,
                        k => return Err(format!("clif: unproven int return ({k:?})")),
                    },
                    // Heap return (string/ref): the raw yields the boxed
                    // VmValue bits and the wrapper passes them through. Only
                    // reachable via the wrapper — the fast call path requires
                    // an int contract, so no clif→clif caller sees these bits
                    // as an unboxed int.
                    SlotKind::Str | SlotKind::Ref => {
                        if is_boxed_kind(state[src]) {
                            b.use_var(vars[src])
                        } else {
                            return Err(format!("clif: non-boxed heap return ({:?})", state[src]));
                        }
                    }
                    k => return Err(format!("clif: unsupported return kind {k:?}")),
                    }
                };
                b.ins().return_(&[v]);
                terminated = true;
            }
            OpCode::CallSelf => {
                // A frame-aware self-call would need its extra ABI params
                // (base/closure) threaded through, which the raw self-call
                // path does not do — bail so it never mis-calls.
                if frame_aware {
                    return Err("clif: CallSelf in frame-aware fn unsupported".into());
                }
                let w1 = code[ip + 1];
                let w2 = code[ip + 2];
                let dest = (w1 >> 8) as usize;
                // arg_count counts the staged callee slot too.
                let arg_count = (w2 >> 8) as usize;
                let arg_start = (w2 & 0xFF) as usize;
                if arg_count != nparams + 1 {
                    return Err("clif: CallSelf arity mismatch".into());
                }
                // Raw self-args must match the entry contract: unboxed for
                // Int-declared params, boxed bits otherwise.
                let mut args = Vec::with_capacity(1 + nparams);
                args.push(exec_ctx);
                for i in 0..nparams {
                    let r = arg_start + 1 + i;
                    let v = if proto.param_kinds[i] == SlotKind::Int {
                        use_int(&mut b, &vars, &state, r)?
                    } else {
                        use_boxed(&mut b, &vars, &state, r)?
                    };
                    args.push(v);
                }
                let call = b.ins().call(self_ref, &args);
                let res = b.inst_results(call)[0];
                b.def_var(vars[dest], res);
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[ip + 1] as usize;
                // The globals data pointer lives at `globals_offset + 8`
                // (the `values` Vec's ptr word inside GlobalStore — Rust
                // reorders the fields, hence the +8; the template loads it
                // at the same offset).
                let gbase = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    exec_ctx,
                    (helpers.globals_offset + 8) as i32,
                );
                let v =
                    b.ins()
                        .load(types::I64, MemFlags::trusted(), gbase, (idx * 8) as i32);
                if state_meta_int(&proto.register_meta, first_reg) {
                    let s = b.ins().ishl_imm(v, 16);
                    let un = b.ins().sshr_imm(s, 16);
                    b.def_var(vars[first_reg], un);
                } else {
                    b.def_var(vars[first_reg], v);
                }
            }
            OpCode::StoreGlobalIdx => {
                let src = (code[ip + 1] >> 8) as usize;
                let idx = code[ip + 2] as usize;
                // Globals are always in the GC root set — a plain boxed
                // store, no barrier. (DefineGlobalIdx is NOT admitted: it
                // can grow the globals vec and move its base.)
                let v = match state[src] {
                    K::Int => {
                        let raw = b.use_var(vars[src]);
                        box_int(&mut b, raw)
                    }
                    K::Boxed => b.use_var(vars[src]),
                    k => return Err(format!("clif: global store of {k:?}")),
                };
                let gbase = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    exec_ctx,
                    (helpers.globals_offset + 8) as i32,
                );
                b.ins()
                    .store(MemFlags::trusted(), v, gbase, (idx * 8) as i32);
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
            OpCode::SetFixedField => {
                fields::emit_set_fixed_field(&mut b, &fld, &state, code, ip, first_reg)?;
            }
            OpCode::Call if !calls_allowed => {
                // `new` / non-int-contract call in a frame-aware fn: the
                // fallback runs the callee (maybe a constructor) and can GC.
                let actx = actx.as_ref().ok_or("clif: call in non-frame-aware fn")?;
                alloc::emit_generic_call(&mut b, actx, &state, &proto.register_meta, code, ip)?;
            }
            OpCode::Call => {
                // Static cross-function call via a guarded monomorphic IC
                // (calls_allowed). w1 = pack(dest, callee_reg);
                // w2 = pack(total, call_base); total-1 real args at
                // call_base+1.. (total counts the staged null callee slot).
                let w1 = code[ip + 1];
                let w2 = code[ip + 2];
                let dest = (w1 >> 8) as usize;
                let callee_reg = (w1 & 0xFF) as usize;
                let total = (w2 >> 8) as usize;
                let call_base = (w2 & 0xFF) as usize;
                let argc = total.saturating_sub(1);

                // The callee must be a global-loaded function (guarded);
                // the linker must have a clif target with a matching int
                // contract; ≤4 args for the by-value fallback.
                let gidx = match state[callee_reg] {
                    K::Global(i) => i as usize,
                    _ => return Err("clif: non-global-fn callee".into()),
                };
                let target = linker
                    .static_target(gidx)
                    .ok_or("clif: no static target for callee")?;
                if target.param_kinds.len() != argc
                    || target.param_kinds.iter().any(|k| *k != SlotKind::Int)
                    || target.return_kind != SlotKind::Int
                    || argc > 4
                {
                    return Err("clif: callee contract unsupported".into());
                }
                for i in 0..argc {
                    let r = call_base + 1 + i;
                    if !matches!(state[r], K::Int | K::Boxed) {
                        return Err("clif: non-int call arg".into());
                    }
                }

                let callee = b.use_var(vars[callee_reg]);
                let expected = b.ins().iconst(types::I64, target.expected_bits as i64);
                let same = b.ins().icmp(IntCC::Equal, callee, expected);
                let fast = b.create_block();
                let slow = b.create_block();
                let merge = b.create_block();
                b.append_block_param(merge, types::I64); // unboxed int result
                b.ins().brif(same, fast, &[], slow, &[]);

                // fast: direct clif->clif raw call (unboxed int in and out).
                b.switch_to_block(fast);
                let mut raw_args = Vec::with_capacity(1 + argc);
                raw_args.push(exec_ctx);
                for i in 0..argc {
                    raw_args.push(use_int(&mut b, &vars, &state, call_base + 1 + i)?);
                }
                let raw_sig = {
                    let mut s = Signature::new(cc);
                    for _ in 0..=argc {
                        s.params.push(AbiParam::new(types::I64));
                    }
                    s.returns.push(AbiParam::new(types::I64));
                    b.import_signature(s)
                };
                let raw_ptr = b.ins().iconst(types::I64, target.raw as i64);
                let rc = b.ins().call_indirect(raw_sig, raw_ptr, &raw_args);
                let rfast = b.inst_results(rc)[0];
                b.ins().jump(merge, &[rfast.into()]);

                // slow: box args, dispatch through the interpreter/JIT via
                // the fallback helper, whose extern "C" signature is fixed at
                // (ctx, callee, argc, a0, a1, a2, a3) — pass exactly seven,
                // padding the unused arg slots with zero.
                b.switch_to_block(slow);
                let argc_v = b.ins().iconst(types::I64, argc as i64);
                let zero_arg = b.ins().iconst(types::I64, 0);
                let mut fb = vec![exec_ctx, callee, argc_v];
                for i in 0..4 {
                    if i < argc {
                        let a = use_int(&mut b, &vars, &state, call_base + 1 + i)?;
                        fb.push(box_int(&mut b, a));
                    } else {
                        fb.push(zero_arg);
                    }
                }
                let boxed = call_helper(&mut b, cc, helpers.clif_call_fallback, &fb);
                let s = b.ins().ishl_imm(boxed, 16);
                let rslow = b.ins().sshr_imm(s, 16);
                b.ins().jump(merge, &[rslow.into()]);

                b.switch_to_block(merge);
                let res = b.block_params(merge)[0];
                b.def_var(vars[dest], res);
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
                alloc::emit_call_native_op(&mut b, actx, &state, code, pool, ip)?;
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
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div => {
                let actx = actx.as_ref().ok_or("clif: generic binop outside alloc fn")?;
                let helper = match op {
                    OpCode::Add => helpers.add,
                    OpCode::Sub => helpers.sub,
                    OpCode::Mul => helpers.mul,
                    _ => helpers.div,
                };
                alloc::emit_binop(&mut b, actx, &state, code, ip, helper);
            }
            OpCode::Nop => {}
            _ => return Err(format!("clif: unsupported opcode {op:?}")),
        }
        apply_kinds(&mut state, code, ip, op, constants, &proto.register_meta);
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
    compile_piece(func, isa)
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
    let mut args = Vec::with_capacity(3 + nparams);
    args.push(exec_ctx);
    // Frame-aware raw functions take `base` (frame register-0 index) and
    // `closure` (this VmClosure*) next, so they can address ctx.stack home
    // slots at safepoints, read the receiver, and drive shape-based object
    // construction.
    if frame_aware {
        args.push(base);
        args.push(closure);
    }
    for i in 0..nparams {
        let boxed = b
            .ins()
            .load(types::I64, MemFlags::trusted(), arg_base, ((1 + i) * 8) as i32);
        if proto.param_kinds[i] == SlotKind::Int {
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
    // Every other admitted return (string/ref, or a constructor's null) is
    // already boxed VmValue bits — pass through. Re-tagging a null would
    // forge a non-null value and defeat jit_construct_fast's null check.
    let result = if proto.return_kind == SlotKind::Int {
        let masked = b.ins().band_imm(raw_res, MASK_48);
        b.ins().bor_imm(masked, INT_TAG)
    } else {
        raw_res
    };
    b.ins().return_(&[result]);
    b.finalize();
    compile_piece(func, isa)
}
