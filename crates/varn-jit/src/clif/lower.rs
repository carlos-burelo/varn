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

use crate::mem::JitBuffer;
use crate::JitHelpers;

const INT_TAG: i64 = 0x7FFC_0000_0000_0000u64 as i64;
const MASK_48: i64 = 0x0000_FFFF_FFFF_FFFFu64 as i64;
const HEAP_MASK: i64 =
    (varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG)
        as i64;
const HEAP_EXPECT: i64 =
    (varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR)
        as i64;

/// Compiled artifact: `entry` (the wrapper, `JitFn` ABI) points into
/// `buffer`.
pub struct ClifArtifact {
    pub buffer: JitBuffer,
    pub entry: *const u8,
}

pub fn try_compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
) -> Result<ClifArtifact, String> {
    if proto.is_generator || proto.is_async || proto.has_this || proto.has_rest {
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

    let raw = lower_raw(proto, constants, helpers, isa)?;
    let wrapper = build_wrapper(proto, helpers, isa)?;

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
    let entry = unsafe { buf.as_ptr().add(wrapper_off) };
    Ok(ClifArtifact { buffer: buf, entry })
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

/// Raw signature: `fn(exec_ctx, arg × nparams) -> i64`. Int-declared args
/// arrive unboxed; everything else arrives as its boxed VmValue bits.
/// `exec_ctx` is only dereferenced by the heap-walking ops and the slow
/// helpers.
fn raw_signature(nparams: usize, isa: &OwnedTargetIsa) -> Signature {
    let mut sig = Signature::new(isa.default_call_conv());
    for _ in 0..=nparams {
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
) -> Result<CompiledPiece, String> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let nparams = proto.arity.saturating_sub(1);
    let nregs = proto.register_count as usize;
    let cc = isa.default_call_conv();

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
    let mut regions: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    {
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
        raw_signature(nparams, isa),
    );
    let self_sig_ref = func.import_signature(raw_signature(nparams, isa));
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
    for i in 0..nparams {
        let p = b.block_params(entry)[1 + i];
        b.def_var(vars[1 + i], p);
    }

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
                if !c.is_int() {
                    return Err("clif: non-int constant".into());
                }
                def_const(&mut b, &vars, first_reg, c.as_int());
            }
            OpCode::LoadNull => {
                // Call-staging callee slot; Poison in the kind lattice, so
                // any real read of it bails.
                def_const(&mut b, &vars, first_reg, 0);
            }
            OpCode::Move => {
                let src = (code[ip + 1] >> 8) as usize;
                if !matches!(state[src], K::Int | K::Bool) {
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
                // The wrapper re-tags the raw result as an int, so the
                // source must be int by PROOF: either the kind lattice
                // shows Int, or the value is boxed and the DECLARED return
                // type is int (checker-enforced at every typed call site).
                // An untyped identity arrow has neither and bails — a
                // coerced arbitrary Boxed return would forge an int from a
                // heap reference.
                let v = match state[src] {
                    K::Int => b.use_var(vars[src]),
                    K::Boxed if proto.return_kind == SlotKind::Int => {
                        use_int(&mut b, &vars, &state, src)?
                    }
                    k => return Err(format!("clif: unproven return kind ({k:?})")),
                };
                b.ins().return_(&[v]);
                terminated = true;
            }
            OpCode::CallSelf => {
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
                let gbase = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    exec_ctx,
                    helpers.globals_offset as i32,
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
                    helpers.globals_offset as i32,
                );
                b.ins()
                    .store(MemFlags::trusted(), v, gbase, (idx * 8) as i32);
            }
            OpCode::ArrayLength => {
                let src = (code[ip + 1] >> 8) as usize;
                let obj = use_boxed(&mut b, &vars, &state, src)?;
                let slow = b.create_block();
                let merge = b.create_block();
                b.append_block_param(merge, types::I64); // unboxed len
                let cache = find_cache(&regions, &cache_vars, ip, src);
                let payload = cached_payload(
                    &mut b,
                    exec_ctx,
                    obj,
                    &helpers.array_layout,
                    helpers.heap_field_offset,
                    slow,
                    cache,
                );
                let len = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    payload,
                    (16 + helpers.array_layout.elems_len_off) as i32,
                );
                b.ins().jump(merge, &[len.into()]);
                // slow: generic helper returns a boxed int; unbox.
                b.switch_to_block(slow);
                let boxed = call_helper(&mut b, cc, helpers.array_length, &[exec_ctx, obj]);
                let s = b.ins().ishl_imm(boxed, 16);
                let un = b.ins().sshr_imm(s, 16);
                b.ins().jump(merge, &[un.into()]);
                b.switch_to_block(merge);
                let res = b.block_params(merge)[0];
                b.def_var(vars[first_reg], res);
            }
            OpCode::ArrayGetIndex => {
                let w1 = code[ip + 1];
                let obj_r = (w1 >> 8) as usize;
                let key_r = (w1 & 0xFF) as usize;
                let obj = use_boxed(&mut b, &vars, &state, obj_r)?;
                let key = use_int(&mut b, &vars, &state, key_r)?;
                let slow = b.create_block();
                let merge = b.create_block();
                b.append_block_param(merge, types::I64); // boxed element
                let cache = find_cache(&regions, &cache_vars, ip, obj_r);
                let payload = cached_payload(
                    &mut b,
                    exec_ctx,
                    obj,
                    &helpers.array_layout,
                    helpers.heap_field_offset,
                    slow,
                    cache,
                );
                let lay = &helpers.array_layout;
                let len = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    payload,
                    (16 + lay.elems_len_off) as i32,
                );
                // Unsigned compare also rejects negative keys.
                let oob = b
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, key, len);
                let inb = b.create_block();
                b.ins().brif(oob, slow, &[], inb, &[]);
                b.switch_to_block(inb);
                let data = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    payload,
                    (16 + lay.elems_ptr_off) as i32,
                );
                let off = b.ins().ishl_imm(key, 3);
                let addr = b.ins().iadd(data, off);
                let elem = b.ins().load(types::I64, MemFlags::trusted(), addr, 0);
                b.ins().jump(merge, &[elem.into()]);
                // slow: same generic helper as the template (returns null
                // out of bounds; allocates nothing).
                b.switch_to_block(slow);
                let boxed_key = box_int(&mut b, key);
                let r = call_helper(
                    &mut b,
                    cc,
                    helpers.jit_array_get_fast,
                    &[exec_ctx, obj, boxed_key],
                );
                b.ins().jump(merge, &[r.into()]);
                b.switch_to_block(merge);
                let res = b.block_params(merge)[0];
                if state_meta_int(&proto.register_meta, first_reg) {
                    let s = b.ins().ishl_imm(res, 16);
                    let un = b.ins().sshr_imm(s, 16);
                    b.def_var(vars[first_reg], un);
                } else {
                    b.def_var(vars[first_reg], res);
                }
            }
            OpCode::ArraySetIndex => {
                let w1 = code[ip + 1];
                let idx_r = (w1 >> 8) as usize;
                let val_r = (w1 & 0xFF) as usize;
                let obj_r = first_reg;
                let obj = use_boxed(&mut b, &vars, &state, obj_r)?;
                let key = use_int(&mut b, &vars, &state, idx_r)?;
                // Only int stores are admitted: a boxed int child is never
                // a heap reference, so no generation write barrier is
                // needed for ANY parent generation.
                if state[val_r] != K::Int {
                    return Err("clif: non-int array store".into());
                }
                let raw_val = b.use_var(vars[val_r]);
                let val = box_int(&mut b, raw_val);
                let slow = b.create_block();
                let merge = b.create_block();
                let cache = find_cache(&regions, &cache_vars, ip, obj_r);
                let payload = cached_payload(
                    &mut b,
                    exec_ctx,
                    obj,
                    &helpers.array_layout,
                    helpers.heap_field_offset,
                    slow,
                    cache,
                );
                let lay = &helpers.array_layout;
                let len = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    payload,
                    (16 + lay.elems_len_off) as i32,
                );
                let oob = b
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, key, len);
                let inb = b.create_block();
                b.ins().brif(oob, slow, &[], inb, &[]);
                b.switch_to_block(inb);
                let data = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    payload,
                    (16 + lay.elems_ptr_off) as i32,
                );
                let off = b.ins().ishl_imm(key, 3);
                let addr = b.ins().iadd(data, off);
                b.ins().store(MemFlags::trusted(), val, addr, 0);
                b.ins().jump(merge, &[]);
                // slow: append/OOB/non-array semantics live in the helper
                // (grows the Rust vec — no VM-heap allocation, no GC).
                b.switch_to_block(slow);
                let boxed_key = box_int(&mut b, key);
                let _ = call_helper(
                    &mut b,
                    cc,
                    helpers.jit_array_set_fast,
                    &[exec_ctx, obj, boxed_key, val],
                );
                b.ins().jump(merge, &[]);
                b.switch_to_block(merge);
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

/// Per-program-point value kind of a VM register. Flow-SENSITIVE: the
/// register allocator reuses one register for a comparison result here and
/// an integer there, so kinds are propagated over the bytecode CFG
/// (worklist, merge at joins) and reads validate against the state at the
/// use point. `Poison` marks the call-staging callee slot (`LoadNull`),
/// which the raw path stages but never reads; `Unset` is bottom, `Mixed`
/// top.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum K {
    Unset,
    Int,
    Bool,
    /// A NaN-boxed VmValue carried as raw bits (heap refs, non-int params,
    /// untyped loads). Coerces to Int at use via the i48 sign-extend — the
    /// same read the interpreter's typed ops perform.
    Boxed,
    Poison,
    Mixed,
}

fn merge(cur: K, k: K) -> K {
    match (cur, k) {
        (K::Unset, x) | (x, K::Unset) => x,
        (a, b) if a == b => a,
        _ => K::Mixed,
    }
}

/// Apply one instruction's effect on the kind state. Shared by the dataflow
/// pass and the lowering walk so the two can never disagree.
fn apply_kinds(
    state: &mut [K],
    code: &[u16],
    ip: usize,
    op: OpCode,
    constants: &[VmValue],
    meta: &[varn_types::register_meta::RegisterMeta],
) {
    let dest = (code[ip] >> 8) as usize;
    let meta_int = |r: usize| meta.get(r).map_or(false, |m| m.kind == SlotKind::Int);
    match op {
        OpCode::LoadIntZero
        | OpCode::LoadIntOne
        | OpCode::LoadIntMinusOne
        | OpCode::LoadInt
        | OpCode::AddInt
        | OpCode::SubInt
        | OpCode::MulInt
        | OpCode::AddImm
        | OpCode::SubImm
        | OpCode::ArrayLength => state[dest] = K::Int,
        OpCode::CallSelf => {
            let dest = (code[ip + 1] >> 8) as usize;
            state[dest] = K::Int;
        }
        OpCode::LoadConst => {
            let idx = code[ip + 1] as usize;
            state[dest] = match constants.get(idx) {
                Some(c) if c.is_int() => K::Int,
                _ => K::Mixed,
            };
        }
        OpCode::LtInt
        | OpCode::LteInt
        | OpCode::GtInt
        | OpCode::GteInt
        | OpCode::EqInt
        | OpCode::NeqInt => state[dest] = K::Bool,
        OpCode::LoadNull => state[dest] = K::Poison,
        OpCode::Move => {
            let src = (code[ip + 1] >> 8) as usize;
            state[dest] = state[src];
        }
        // Loads from the heap/globals produce boxed values; when the
        // checker-derived register meta proves the slot Int, the lowering
        // unboxes at the load and the var carries Int.
        OpCode::ArrayGetIndex | OpCode::LoadGlobalIdx => {
            state[dest] = if meta_int(dest) { K::Int } else { K::Boxed };
        }
        _ => {}
    }
}

/// Kind state at every block entry, to a fixpoint over the bytecode CFG.
fn kind_flow(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    constants: &[VmValue],
    block_starts: &[usize],
    nregs: usize,
    param_kinds: &[SlotKind],
    meta: &[varn_types::register_meta::RegisterMeta],
) -> Result<HashMap<usize, Vec<K>>, String> {
    let mut entry0 = vec![K::Unset; nregs];
    for (i, pk) in param_kinds.iter().enumerate() {
        if 1 + i < nregs {
            entry0[1 + i] = if *pk == SlotKind::Int { K::Int } else { K::Boxed };
        }
    }
    let mut entries: HashMap<usize, Vec<K>> = HashMap::new();
    entries.insert(0, entry0);
    let mut work: Vec<usize> = vec![0];

    while let Some(start) = work.pop() {
        let mut state = entries[&start].clone();
        let mut ip = start;
        loop {
            if ip >= code.len() {
                break;
            }
            if ip != start && block_starts.contains(&ip) {
                // fall-through edge into the next block
                propagate(&mut entries, &mut work, ip, &state);
                break;
            }
            let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
            let op = OpCode::from_u8(code[ip] as u8).ok_or("clif: unknown opcode")?;
            match op {
                OpCode::Jump => {
                    let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                    propagate(&mut entries, &mut work, ip + 3 + off, &state);
                    break;
                }
                OpCode::Loop => {
                    let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                    propagate(&mut entries, &mut work, (ip + 3) - off, &state);
                    break;
                }
                OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                    let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                    propagate(&mut entries, &mut work, ip + 3 + off, &state);
                    propagate(&mut entries, &mut work, ip + 3, &state);
                    break;
                }
                OpCode::Return => break,
                _ => {
                    apply_kinds(&mut state, code, ip, op, constants, meta);
                }
            }
            ip += info.len;
        }
    }
    Ok(entries)
}

fn propagate(
    entries: &mut HashMap<usize, Vec<K>>,
    work: &mut Vec<usize>,
    target: usize,
    state: &[K],
) {
    match entries.get_mut(&target) {
        Some(cur) => {
            let mut changed = false;
            for (c, s) in cur.iter_mut().zip(state) {
                let m = merge(*c, *s);
                if m != *c {
                    *c = m;
                    changed = true;
                }
            }
            if changed {
                work.push(target);
            }
        }
        None => {
            entries.insert(target, state.to_vec());
            work.push(target);
        }
    }
}

fn def_const(b: &mut FunctionBuilder, vars: &[Variable], reg: usize, v: i64) {
    let c = b.ins().iconst(types::I64, v);
    b.def_var(vars[reg], c);
}

/// Read a register as an unboxed int. `Int` vars are already raw; `Boxed`
/// vars coerce via the i48 sign-extend — bit-identical to the
/// interpreter's typed-op read of the payload.
fn use_int(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    match state[r] {
        K::Int => Ok(b.use_var(vars[r])),
        K::Boxed => {
            let v = b.use_var(vars[r]);
            let s = b.ins().ishl_imm(v, 16);
            Ok(b.ins().sshr_imm(s, 16))
        }
        k => Err(format!("clif: int use of {k:?} register")),
    }
}

/// Read a register as boxed VmValue bits (heap receivers).
fn use_boxed(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    match state[r] {
        K::Boxed => Ok(b.use_var(vars[r])),
        k => Err(format!("clif: boxed use of {k:?} register")),
    }
}

/// Re-tag an unboxed int as a VmValue.
fn box_int(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let m = b.ins().band_imm(v, MASK_48);
    b.ins().bor_imm(m, INT_TAG)
}

fn state_meta_int(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).map_or(false, |m| m.kind == SlotKind::Int)
}

/// Innermost loop region containing `ip` with a hoisted cache for `r`.
fn find_cache(
    regions: &[(usize, usize, Vec<usize>)],
    cache_vars: &HashMap<(usize, usize), Variable>,
    ip: usize,
    r: usize,
) -> Option<Variable> {
    regions
        .iter()
        .filter(|(h, e, regs)| *h <= ip && ip < *e && regs.contains(&r))
        .min_by_key(|(h, e, _)| e - h)
        .map(|(h, _, _)| cache_vars[&(*h, r)])
}

/// Indirect call to a template-JIT runtime helper
/// (`extern "C" fn(exec_ctx, VmValue…) -> VmValue`). The admitted helpers
/// never allocate on the VM heap (no GC can run under a clif frame) and
/// raise VM errors by longjmp'ing to the outer setjmp, exactly like the
/// template's slow paths.
fn call_helper(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) -> cranelift_codegen::ir::Value {
    let mut sig = Signature::new(cc);
    for _ in 0..args.len() {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    let sig_ref = b.import_signature(sig);
    let ptr = b.ins().iconst(types::I64, helper as i64);
    let call = b.ins().call_indirect(sig_ref, ptr, args);
    b.inst_results(call)[0]
}

/// Payload via the loop cache when one exists for this access: cache != 0
/// short-circuits the whole guard walk (one test + branch, perfectly
/// predicted after iteration one); cache == 0 — or no cache — takes the
/// full resolve.
#[allow(clippy::too_many_arguments)]
fn cached_payload(
    b: &mut FunctionBuilder,
    exec_ctx: cranelift_codegen::ir::Value,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    heap_off: usize,
    slow: cranelift_codegen::ir::Block,
    cache: Option<Variable>,
) -> cranelift_codegen::ir::Value {
    match cache {
        Some(cv) => {
            let c = b.use_var(cv);
            let full = b.create_block();
            let ready = b.create_block();
            b.append_block_param(ready, types::I64);
            b.ins().brif(c, ready, &[c.into()], full, &[]);
            b.switch_to_block(full);
            let p = emit_array_payload(b, exec_ctx, obj, lay, heap_off, slow, false);
            b.ins().jump(ready, &[p.into()]);
            b.switch_to_block(ready);
            b.block_params(ready)[0]
        }
        None => emit_array_payload(b, exec_ctx, obj, lay, heap_off, slow, false),
    }
}

/// Resolve a boxed receiver down to its array payload pointer (the three
/// `Vec<VmValue>` words live at payload+16). Mirrors the template's
/// `emit_resolve_array_payload`: heap-tag check, generation select on bit
/// 31 of the index, slot tag check. Any rejection branches to `slow`; on
/// return the builder is positioned in a fresh block where the payload is
/// valid.
fn emit_array_payload(
    b: &mut FunctionBuilder,
    exec_ctx: cranelift_codegen::ir::Value,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    heap_off: usize,
    slow: cranelift_codegen::ir::Block,
    nursery_only: bool,
) -> cranelift_codegen::ir::Value {
    // The chain down to the payload pointer is `readonly` FOR THE DURATION
    // OF ONE ROUTED ACTIVATION: heap indices only get rebound by a GC
    // move or slot reuse, and no op in the routed subset (including the
    // admitted slow helpers) can allocate on the VM heap, so no collection
    // can run underneath us. Marking them readonly lets Cranelift's
    // mid-end hoist the whole resolve out of loops. The element Vec's
    // data/len words are NOT readonly — an out-of-bounds store's append
    // path reallocates them.
    let ro = MemFlags::trusted().with_readonly().with_can_move();
    let tag = b.ins().band_imm(obj, HEAP_MASK);
    let is_heap = b.ins().icmp_imm(IntCC::Equal, tag, HEAP_EXPECT);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    let raw = b.ins().band_imm(obj, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, ro, exec_ctx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);

    let (base, idx) = if nursery_only {
        let cont = b.create_block();
        b.ins().brif(old_bit, slow, &[], cont, &[]);
        b.switch_to_block(cont);
        let base = b.ins().load(
            types::I64,
            ro,
            rc,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        (base, raw)
    } else {
        let base_old = b.ins().load(
            types::I64,
            ro,
            rc,
            (lay.slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let base_nur = b.ins().load(
            types::I64,
            ro,
            rc,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let idx_old = b.ins().band_imm(raw, 0x7FFF_FFFF);
        let base = b.ins().select(old_bit, base_old, base_nur);
        let idx = b.ins().select(old_bit, idx_old, raw);
        (base, idx)
    };

    let byte_off = b.ins().imul_imm(idx, lay.slot_size as i64);
    let slot = b.ins().iadd(base, byte_off);
    let tagb = b.ins().uload8(types::I64, ro, slot, 0);
    let is_arr = b.ins().icmp_imm(IntCC::Equal, tagb, lay.array_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_arr, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    b.ins().load(types::I64, ro, slot, lay.payload_off as i32)
}

/// `(v << 16) >> 16` — the canonical i48 wrap (varn_core::numeric), which
/// is also exactly the unboxing of an int-tagged VmValue payload.
fn wrap_i48(b: &mut FunctionBuilder, v: cranelift_codegen::ir::Value) -> cranelift_codegen::ir::Value {
    let s = b.ins().ishl_imm(v, 16);
    b.ins().sshr_imm(s, 16)
}

/// Wrapper with the template `JitFn` ABI:
/// `(stack_ptr, closure, base, exec_ctx) -> boxed VmValue`.
fn build_wrapper(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
) -> Result<CompiledPiece, String> {
    let nparams = proto.arity.saturating_sub(1);
    let mut sig = Signature::new(isa.default_call_conv());
    for _ in 0..4 {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let mut func = Function::with_name_signature(UserFuncName::user(0, 1), sig);
    let raw_sig = func.import_signature(raw_signature(nparams, isa));
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

    let (stack_ptr, base, exec_ctx) = {
        let p = b.block_params(block);
        (p[0], p[2], p[3])
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
    let mut args = Vec::with_capacity(1 + nparams);
    args.push(exec_ctx);
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

    let masked = b.ins().band_imm(raw_res, MASK_48);
    let boxed = b.ins().bor_imm(masked, INT_TAG);
    b.ins().return_(&[boxed]);
    b.finalize();
    compile_piece(func, isa)
}
