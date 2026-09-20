//! CLIF lowering from the portable typed SSA (`varn_types::ssa`).
//!
//! The second, sibling lowering of the same SSA the interpreter runs: each
//! [`varn_types::ssa::SsaValue`] becomes one CLIF `Value` of its declared
//! physical type, and each typed `SsaOp` becomes the native instruction the
//! checker already proved — no re-derivation from operand types, no flow
//! lattice. It is intentionally a *strict subset*: anything outside the
//! admitted families returns `Err`, and [`super::lower::try_compile`] falls
//! back to the bytecode lowering for that function. The fallback is a missing
//! optimization, never a wrong result (Ley 10).
//!
//! Storage model (one rule, no re-derivation):
//! * **scalar** values (`Int`/`Float`/`Bool`) live in CLIF registers;
//! * **heap** values (`Str`/`Ref`/`Dyn`) live in their VM **home** slot, and
//!   every read loads from the home. Homes are the roots the GC knows about,
//!   so a heap value survives an allocation or a call by construction — this
//!   is the interpreter's own model, not a new one. The cost is home traffic;
//!   it is correct first, and a later pass may hoist.
//!
//! Shapes admitted (see `docs/plans/2026-09-20-PLAN-PENDIENTE.md`):
//! * **leaf** — only scalars, no `this`/upvalues/alloc: raw `(args…) -> scalar`;
//! * **frame-aware** — the body has heap values, module globals or calls, so it
//!   needs `(stack, closure, base, exec_ctx, args…)`.
//!
//! Module split by domain (each file owns one invariant):
//! * this file — driver, eligibility, block/phi plumbing, value storage;
//! * [`scalar`] — native scalar ops (arith/compare/bitwise/negate);
//! * [`boxed`] — ops whose semantics live behind a boxed runtime helper;
//! * [`globals`] — module-relative global reads;
//! * [`call`] — self-recursion and cross-proto calls;
//! * [`term`] — terminators and the branch/jump argument windows.

use cranelift_codegen::ir::{
    types, ExtFuncData, ExternalName, FuncRef, Function, InstBuilder, MemFlags, StackSlotData,
    StackSlotKind, UserExternalName, UserFuncName, Value,
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaOp, SsaProto, SsaTerm};
use varn_types::{FunctionProto, VmValue};

use super::abi::raw_signature;
use super::lower::ClifLinker;
use super::piece::{compile_piece, CompiledPiece};
use crate::JitHelpers;

mod boxed;
mod call;
mod classops;
mod globals;
mod heap;
mod heapvalue;
mod props;
mod scalar;
mod term;

/// Frame resources a frame-aware body needs for global access, calls and home
/// storage. `base` is the activation id (raw ABI param 2).
pub(super) struct FrameIo<'a> {
    pub exec_ctx: Value,
    pub closure: Value,
    pub base: Value,
    pub linker: &'a dyn ClifLinker,
}

/// Everything an instruction emission needs that is not the builder or the
/// value map, so each domain module takes one context instead of a drifting
/// argument list.
pub(super) struct Ctx<'a> {
    pub cc: CallConv,
    pub helpers: &'a JitHelpers,
    pub ssa: &'a SsaProto,
    pub proto: &'a FunctionProto,
    /// Resolved constant pool of the proto, indexed like the bytecode's.
    pub constants: &'a [VmValue],
    pub self_ref: FuncRef,
    /// `Some` iff this body is frame-aware (heap/globals/calls).
    pub frame: Option<FrameIo<'a>>,
    /// When true (frame-aware), the homes are authoritative for EVERY register,
    /// scalar included — the interpreter's model. This is what lets closures,
    /// `Try` resume and OSR read a scalar out of its home. A leaf body keeps
    /// scalars in CLIF registers only.
    pub homes_all: bool,
}

/// Whether an SSA value is stored in a home rather than a CLIF register.
pub(super) fn is_heap(kind: SlotKind) -> bool {
    matches!(kind, SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic)
}

pub(super) fn get(values: &[Option<Value>], v: u32) -> Result<Value, String> {
    values
        .get(v as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("from_ssa: value {v} used before definition"))
}

/// Read a value: scalars from the CLIF map, heap values from their home. In a
/// frame-aware body every value lives in its home.
pub(super) fn load_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    let kind = ctx.ssa.value_ty(v);
    if ctx.homes_all || is_heap(kind) {
        load_home_value(b, ctx, ctx.ssa.reg(v), kind)
    } else {
        get(values, v)
    }
}

/// Read `reg`'s home and unbox it into `kind`'s native representation.
pub(super) fn load_home_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    kind: SlotKind,
) -> Result<Value, String> {
    let boxed = use_heap(b, ctx, reg)?;
    heap::unbox_dest(b, kind, boxed)
}

/// Write a native scalar value to its home (boxing it), for a frame-aware body.
pub(super) fn store_home_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    value: u32,
    native: Value,
) -> Result<(), String> {
    let kind = ctx.ssa.value_ty(value);
    let boxed = match kind {
        SlotKind::Int => super::emit::box_int(b, native),
        SlotKind::Float => super::emit::box_f64(b, native),
        SlotKind::Bool => super::emit::box_bool(b, native),
        _ => native,
    };
    def_heap(b, ctx, ctx.ssa.reg(value), boxed)
}

/// Write a boxed heap value to `reg`'s home (the GC root).
pub(super) fn def_heap(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    boxed: Value,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: heap value without a frame")?;
    let (tag, payload) = b.ins().isplit(boxed);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    super::emit::call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.home_store,
        &[frame.exec_ctx, frame.base, reg_v, tag, payload],
    );
    Ok(())
}

/// Read a boxed heap value back from `reg`'s home.
pub(super) fn use_heap(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: heap value without a frame")?;
    let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 4));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    super::emit::call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.home_load,
        &[frame.exec_ctx, frame.base, reg_v, addr],
    );
    Ok(b.ins().load(types::I128, MemFlags::trusted(), addr, 0))
}

/// CLIF type of an SSA value. `Bool` is a raw `I64` 0/1; `Ref`/`Dyn`/`Str` is a
/// 16-byte `VmValue` (`I128` = tag+payload).
pub(super) fn clif_ty(kind: SlotKind) -> Option<cranelift_codegen::ir::Type> {
    match kind {
        SlotKind::Int | SlotKind::Bool => Some(types::I64),
        SlotKind::Float => Some(types::F64),
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => Some(types::I128),
    }
}

/// Attempt the SSA lowering. `Err` is the fallback signal, not a failure: the
/// caller re-lowers from bytecode. On success returns the raw piece and whether
/// it takes the frame-aware ABI.
pub(super) fn try_lower(
    proto: &FunctionProto,
    ssa: &SsaProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> Result<(CompiledPiece, bool), String> {
    let nparams = proto.arity.saturating_sub(1);
    if proto.upvalue_count > 0
        || proto.is_generator
        || proto.is_async
        || ssa.blocks.is_empty()
        || ssa.entry as usize >= ssa.blocks.len()
        || ssa.blocks[ssa.entry as usize].params.len() != nparams
    {
        return Err("from_ssa: not a leaf-compatible proto".into());
    }
    let scalar = |k: SlotKind| matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool);
    // Params must be scalar (the raw ABI passes non-float params as a bare
    // payload, so heap params are read from homes — not modelled yet). The
    // RETURN may be any class: a non-scalar return is written boxed to
    // `jit_native_result` and the raw returns void, which is what the wrapper
    // reads (see `build_wrapper`).
    if !proto.param_kinds.iter().all(|k| scalar(*k)) {
        if std::env::var_os("VARN_CLIF_TRACE").is_some() {
            eprintln!(
                "from_ssa sig {:?}: params={:?} ret={:?} has_this={}",
                proto.name, proto.param_kinds, proto.return_kind, proto.has_this
            );
        }
        return Err("from_ssa: non-scalar parameter".into());
    }

    let has_heap = ssa.values.iter().any(|v| is_heap(v.ty));
    let scalar_return = scalar(proto.return_kind);
    let frame_aware = has_heap
        || !scalar_return
        || proto.has_this
        || ssa.has_this
        || ssa.blocks.iter().any(|blk| {
            blk.insts
                .iter()
                .any(|i| matches!(i.op, SsaOp::Call { .. } | SsaOp::LoadGlobalIdx(_)))
        });

    let cc = isa.default_call_conv();
    let mut func = Function::with_name_signature(
        UserFuncName::user(0, 0),
        raw_signature(proto, nparams, isa, frame_aware),
    );
    let self_sig = func.import_signature(raw_signature(proto, nparams, isa, frame_aware));
    let self_name = func.declare_imported_user_function(UserExternalName::new(0, 0));
    let self_ref = func.import_function(ExtFuncData {
        name: ExternalName::user(self_name),
        signature: self_sig,
        colocated: true,
    });
    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

    // One CLIF block per SSA block; the SSA entry block *is* the function entry
    // and receives the raw function parameters directly.
    let entry = ssa.entry as usize;
    let preamble = if frame_aware { 4 } else { 0 };
    let mut blocks: Vec<Option<cranelift_codegen::ir::Block>> = vec![None; ssa.blocks.len()];
    let entry_blk = b.create_block();
    b.append_block_params_for_function_params(entry_blk);
    blocks[entry] = Some(entry_blk);
    for (i, blk) in ssa.blocks.iter().enumerate() {
        if i == entry {
            continue;
        }
        let cb = b.create_block();
        for &p in &blk.params {
            let ty = clif_ty(ssa.value_ty(p)).ok_or("from_ssa: unsupported block param")?;
            b.append_block_param(cb, ty);
        }
        blocks[i] = Some(cb);
    }

    // The frame-aware raw ABI prepends `stack, closure, base, exec_ctx`.
    let frame = if frame_aware {
        let params = b.block_params(entry_blk);
        if params.len() != preamble + ssa.blocks[entry].params.len() {
            return Err("from_ssa: entry param count mismatch".into());
        }
        Some(FrameIo {
            closure: params[1],
            base: params[2],
            exec_ctx: params[3],
            linker,
        })
    } else {
        None
    };
    let ctx = Ctx {
        cc,
        helpers,
        ssa,
        proto,
        constants,
        self_ref,
        frame,
        homes_all: frame_aware,
    };

    let mut values: Vec<Option<Value>> = vec![None; ssa.values.len()];

    for i in order(ssa) {
        let blk = &ssa.blocks[i];
        let cb = blocks[i].expect("block created");
        b.switch_to_block(cb);

        let params: Vec<Value> = b.block_params(cb).to_vec();
        let base = if i == entry { preamble } else { 0 };
        if i == entry && params.len() != base + blk.params.len() {
            return Err("from_ssa: entry param count mismatch".into());
        }
        for (k, &p) in blk.params.iter().enumerate() {
            let pv = params[base + k];
            let kind = ssa.value_ty(p);
            if ctx.homes_all {
                // Homes are authoritative: land every phi (scalar or heap).
                if is_heap(kind) {
                    def_heap(&mut b, &ctx, ssa.reg(p), pv)?;
                } else {
                    store_home_value(&mut b, &ctx, p, pv)?;
                }
            } else if is_heap(kind) {
                def_heap(&mut b, &ctx, ssa.reg(p), pv)?;
            } else {
                values[p as usize] = Some(pv);
            }
        }

        for inst in &blk.insts {
            if let Some(v) = scalar::emit_inst(&mut b, &ctx, &mut values, &inst.op, inst.dest)? {
                if let Some(d) = inst.dest {
                    let kind = ssa.value_ty(d);
                    if ctx.homes_all && !is_heap(kind) {
                        store_home_value(&mut b, &ctx, d, v)?;
                    } else {
                        values[d as usize] = Some(v);
                    }
                }
            }
        }
        term::emit_term(&mut b, &ctx, &blocks, &values, &blk.term)?;
    }

    b.seal_all_blocks();
    b.finalize();
    Ok((compile_piece(func, isa)?, frame_aware))
}

/// Reverse postorder from the entry so every value is defined before its uses,
/// with unreachable blocks appended in index order so all are still filled.
fn order(ssa: &SsaProto) -> Vec<usize> {
    let n = ssa.blocks.len();
    let entry = ssa.entry as usize;
    let mut visited = vec![false; n];
    let mut post = Vec::with_capacity(n);
    let mut stack: Vec<(usize, u8)> = vec![(entry, 0)];
    visited[entry] = true;
    while let Some(&(b, stage)) = stack.last() {
        stack.last_mut().unwrap().1 += 1;
        let succs: Vec<usize> = match &ssa.blocks[b].term {
            SsaTerm::Jump { target, .. } => vec![*target as usize],
            SsaTerm::Branch {
                then_blk, else_blk, ..
            } => vec![*else_blk as usize, *then_blk as usize],
            _ => Vec::new(),
        };
        match succs.get(stage as usize) {
            Some(&s) => {
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
    let mut out: Vec<usize> = post.into_iter().rev().collect();
    for (i, seen) in visited.iter().enumerate() {
        if !seen {
            out.push(i);
        }
    }
    out
}

pub(super) fn resolve_args(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    args: &[u32],
) -> Result<Vec<cranelift_codegen::ir::BlockArg>, String> {
    args.iter()
        .map(|v| load_value(b, ctx, values, *v).map(cranelift_codegen::ir::BlockArg::from))
        .collect()
}

pub(super) fn block_of(
    blocks: &[Option<cranelift_codegen::ir::Block>],
    id: u32,
) -> Result<cranelift_codegen::ir::Block, String> {
    blocks
        .get(id as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("from_ssa: block {id} not created"))
}
