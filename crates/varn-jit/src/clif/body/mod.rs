//! The body lowering entry point: bytecode → CLIF.

pub(crate) mod op_dispatch;

use cranelift_codegen::ir::{types, Function, InstBuilder, MemFlags, UserFuncName};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::{FunctionProto, VmValue};

use super::abi::raw_signature;
use super::alloc::{self, AllocCtx};
use super::arrays;
use super::debug::ClifDebugSink;
use super::emit::{box_for_target, box_or_pass, call_helper, meta_is_float, meta_is_int, unbox_bool, unbox_f64_coerce, wrap_i48};
use super::fields;
use super::floats;
use super::generic;
use super::globals;
use super::kinds::{apply_kinds, is_boxed_kind, kind_flow, K};
use super::lower::ClifLinker;
use super::piece::{compile_piece, CompiledPiece};
use super::{preheader, scan, vars};
use crate::JitHelpers;

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_raw(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    has_alloc: bool,
    osr_ip: Option<usize>,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<CompiledPiece, String> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let osr = osr_ip.is_some();
    let nparams = proto.arity.saturating_sub(1);
    let sig_nparams = if osr { 0 } else { nparams };
    let nregs = proto.register_count as usize;
    let cc = isa.default_call_conv();
    let has_round = floats::has_round_support(isa);
    let want_roots = debug.as_deref().is_some_and(|d| d.want_roots);

    let mirror_home = proto.has_this
        || has_alloc
        || proto.upvalue_count > 0
        || proto.is_generator
        || proto.is_async;
    let frame_aware = osr || mirror_home;

    let block_starts = scan::block_starts(code, pool)?;
    let regions = scan::loop_regions(proto, code, pool, has_alloc)?;

    let mut func = Function::with_name_signature(
        UserFuncName::user(0, 0),
        raw_signature(sig_nparams, isa, frame_aware),
    );
    let self_sig_ref = func.import_signature(raw_signature(sig_nparams, isa, frame_aware));
    let self_name =
        func.declare_imported_user_function(cranelift_codegen::ir::UserExternalName::new(0, 0));
    let self_ref = func.import_function(cranelift_codegen::ir::ExtFuncData {
        name: cranelift_codegen::ir::ExternalName::user(self_name),
        signature: self_sig_ref,
        colocated: true,
    });

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

    let vars::VarFile {
        vars,
        cache_vars,
        all_caches,
    } = vars::declare(
        &mut b,
        nregs,
        &proto.register_meta,
        &regions,
        code,
        pool,
        want_roots,
    );

    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);

    let zero = b.ins().iconst(types::I64, 0);
    let zero_f = b.ins().f64const(0.0);
    for (r, v) in vars.iter().enumerate() {
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

    let live = if alloc_env.is_some() {
        super::liveness::analyze(code, pool, nregs)
    } else {
        super::liveness::Liveness::everything()
    };

    let narrow_roots = if alloc_env.is_some() {
        !alloc::has_try(code, pool)?
    } else {
        false
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
        narrow_roots,
        cur_ip: std::cell::Cell::new(0),
        safepoints: want_roots.then(|| std::cell::RefCell::new(Vec::new())),
    });

    let reg_offset = 1;
    for i in 0..sig_nparams {
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

    if proto.has_this && !osr {
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

    match osr_ip {
        Some(target_ip) => {
            let target = *blocks
                .get(&target_ip)
                .ok_or("osr: target ip is not a block start")?;
            let target_state = entries
                .get(&target_ip)
                .ok_or("osr: target ip has no kind state")?;
            let actx = actx.as_ref().ok_or("osr: entry is not frame-aware")?;
            super::osr::emit_osr_entry(&mut b, actx, target_state, target);
        }
        None => {
            if let Some(first) = blocks.get(&0) {
                b.ins().jump(*first, &[]);
            }
        }
    }

    let mut state: Vec<K> = entries[&0].clone();
    let mut filled: Vec<usize> = Vec::new();
    let mut ip = 0usize;
    let mut terminated = true;

    while ip < code.len() {
        if let Some(blk) = blocks.get(&ip) {
            if !terminated {
                if let Some(e) = entries.get(&ip) {
                    box_for_target(&mut b, &proto.register_meta, &vars, &state, e);
                    state = e.clone();
                }
                preheader::emit_region_caches(
                    &mut b,
                    helpers,
                    exec_ctx,
                    &vars,
                    &cache_vars,
                    &regions,
                    &state,
                    ip,
                );
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
                    terminated = true;
                    let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
                    ip += info.len;
                    continue;
                }
            }
        } else if terminated {
            let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
            ip += info.len;
            continue;
        }

        let raw_op = code[ip];
        let first_reg = (raw_op >> 8) as usize;
        let op = OpCode::from_u8(raw_op as u8).ok_or("clif: unknown opcode")?;
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        let next_ip = ip + info.len;

        if let Some(a) = actx.as_ref() {
            a.cur_ip.set(ip);
        }
        if want_roots {
            b.set_srcloc(cranelift_codegen::ir::SourceLoc::new(ip as u32));
        }

        match op {
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
                if has_alloc {
                    if let Some(actx) = actx.as_ref() {
                        // Every cache Variable, payloads AND views: a collection
                        // here can move what a view points into, and the
                        // safepoint resetting all of them is what lets
                        // `scan::loop_regions` treat the region as cacheable.
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
                let cond = match state[first_reg] {
                    K::Bool | K::Int => b.use_var(vars[first_reg]),
                    k if is_boxed_kind(k) => {
                        let v = b.use_var(vars[first_reg]);
                        let falsy_boxed =
                            call_helper(&mut b, cc, helpers.logical_not, &[exec_ctx, v]);
                        let falsy = unbox_bool(&mut b, falsy_boxed);
                        b.ins().bxor_imm(falsy, 1)
                    }
                    _ => {
                        if meta_is_int(&proto.register_meta, first_reg) {
                            b.use_var(vars[first_reg])
                        } else {
                            let v = box_or_pass(&mut b, &vars, &state, first_reg);
                            let falsy_boxed =
                                call_helper(&mut b, cc, helpers.logical_not, &[exec_ctx, v]);
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
                    b.ins()
                        .brif(cond, fall_trampoline, &[], target_trampoline, &[]);
                } else {
                    b.ins()
                        .brif(cond, target_trampoline, &[], fall_trampoline, &[]);
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
            _ => {
                let term = op_dispatch::dispatch_opcode(
                    &mut b,
                    op,
                    code,
                    pool,
                    ip,
                    first_reg,
                    proto,
                    constants,
                    &vars,
                    &mut state,
                    actx.as_ref(),
                    exec_ctx,
                    helpers,
                    cc,
                    has_alloc,
                    has_round,
                    linker,
                    &arr,
                    &fld,
                    &gbl,
                    &gen,
                    entry,
                    self_ref,
                    frame_aware,
                    osr,
                    nparams,
                )?;
                if term {
                    terminated = true;
                }
            }
        }

        // Advance the kind lattice past the op just emitted. Without this the
        // whole block is lowered against its ENTRY state, so every register
        // defined mid-block is read with a stale kind — an `Int` param handed
        // to a call as raw i48 where the callee reads a boxed value, and back.
        apply_kinds(
            &mut state,
            code,
            pool,
            ip,
            op,
            constants,
            &proto.register_meta,
        );

        ip = next_ip;
    }

    if !terminated {
        let z = b.ins().iconst(types::I64, 0);
        b.ins().return_(&[z]);
    }

    for (&s, blk) in blocks.iter() {
        if !filled.contains(&s) {
            b.switch_to_block(*blk);
            b.ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
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
