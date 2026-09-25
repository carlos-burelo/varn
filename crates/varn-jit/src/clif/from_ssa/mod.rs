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
//! * this file — driver, eligibility, block/phi plumbing;
//! * [`cfg`] — the compiled CFG: reachable blocks, order, loops;
//! * [`store`] — where a value lives and how a result lands there;
//! * [`scalar`] — native scalar ops (arith/compare/bitwise/negate);
//! * [`arrays`] — inline element access on proven arrays;
//! * [`boxed`] — ops whose semantics live behind a boxed runtime helper;
//! * [`globals`] — module-relative global reads;
//! * [`call`] — self-recursion and cross-proto calls;
//! * [`numeric`] — `std:math` intrinsics and numeric conversions;
//! * [`pool`] — constant-pool literals;
//! * [`closures`] — closure creation, captured variables and upvalues;
//! * [`exceptions`] — `try` regions (resumed interpreted) and `throw`;
//! * [`views`] — array views cached between safepoints;
//! * [`induction`] — loop counters whose step cannot overflow;
//! * [`osr`] — resuming a running frame at a loop header;
//! * [`term`] — terminators and the branch/jump argument windows.

use cranelift_codegen::ir::{
    ExtFuncData, ExternalName, FuncRef, Function, InstBuilder, UserExternalName, UserFuncName,
    Value,
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

mod arrays;
mod boxed;
mod call;
mod cfg;
mod classops;
mod closures;
mod dynop;
mod exceptions;
mod globals;
mod heap;
mod heapvalue;
mod induction;
mod numeric;
mod osr;
mod pool;
mod props;
mod scalar;
mod store;
mod term;
mod views;

use store::{clif_ty, def_heap, is_heap, land, load_value, use_heap, Out};

/// Frame resources a frame-aware body needs for global access, calls and home
/// storage. `base` is the activation id (raw ABI param 2).
pub(super) struct FrameIo<'a> {
    pub exec_ctx: Value,
    pub closure: Value,
    pub base: Value,
    pub linker: &'a dyn ClifLinker,
    /// Register → (class, index): where each register's home is.
    pub layout: varn_types::register_meta::FrameLayout,
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
    /// Whether the host rounds in one instruction (`floor`/`ceil`, SSE4.1).
    pub has_round: bool,
    /// Each array receiver's view, valid until the next safepoint.
    pub views: views::Views,
    /// The `int` steps proven unable to overflow ([`induction`]).
    pub in_range_steps: std::collections::HashSet<u32>,
    /// In an OSR lowering, the scalars live into the loop header that the
    /// resumed body also redefines, each held in a Cranelift variable so the
    /// header merges the entry's value with the body's (see [`store`]).
    pub carried: std::collections::HashMap<u32, cranelift_frontend::Variable>,
}

/// Attempt the SSA lowering. `Err` is the fallback signal, not a failure: the
/// caller re-lowers from bytecode. On success returns the raw piece and whether
/// it takes the frame-aware ABI.
///
/// `osr_ip` selects the entry. `None` is a call: arguments in, execution from
/// the entry block. `Some(ip)` is an on-stack-replacement entry into a running
/// interpreted frame at the loop header starting at bytecode offset `ip`: no
/// arguments, the header's parameters and live values read from their homes,
/// and only the blocks reachable from the header compiled.
pub(super) fn try_lower(
    proto: &FunctionProto,
    ssa: &SsaProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    osr_ip: Option<usize>,
) -> Result<(CompiledPiece, bool), String> {
    let osr = match osr_ip {
        None => None,
        Some(ip) => Some(
            ssa.loop_header_at(ip)
                .ok_or_else(|| format!("from_ssa: no loop header at ip {ip}"))?,
        ),
    };
    let nparams = proto.arity.saturating_sub(1);
    if proto.is_generator
        || proto.is_async
        || ssa.blocks.is_empty()
        || ssa.entry as usize >= ssa.blocks.len()
        || ssa.blocks[ssa.entry as usize].params.len() != nparams
    {
        return Err("from_ssa: not a leaf-compatible proto".into());
    }
    let scalar = |k: SlotKind| matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool);
    // A non-scalar parameter makes the body frame-aware: the raw ABI passes it
    // as a bare payload, but the wrapper (the only way into a frame-aware
    // body — its `clif_raw` stays 0, so no call site links it directly) has
    // already left every argument in its home, register `1 + i`. The RETURN
    // may be any class: a non-scalar return is written boxed to
    // `jit_native_result` and the raw returns void, which is what the wrapper
    // reads (see `build_wrapper`).
    let heap_params = !proto.param_kinds.iter().all(|k| scalar(*k));

    let has_heap = ssa.values.iter().any(|v| is_heap(v.ty));
    let scalar_return = scalar(proto.return_kind);
    // An OSR entry resumes a frame that exists: it reads the frame's homes.
    let frame_aware = osr.is_some()
        || has_heap
        || heap_params
        || !scalar_return
        || proto.has_this
        || ssa.has_this
        || proto.upvalue_count > 0
        || ssa.blocks.iter().any(|blk| {
            blk.insts.iter().any(|i| needs_frame(ssa, &i.op))
                || matches!(blk.term, SsaTerm::Throw(_))
        });

    let cc = isa.default_call_conv();
    let abi_params = if osr.is_some() { 0 } else { nparams };
    let mut func = Function::with_name_signature(
        UserFuncName::user(0, 0),
        raw_signature(proto, abi_params, isa, frame_aware),
    );
    // A self call in an OSR body goes through the runtime (it is frame-aware),
    // never to this entry.
    let self_sig = func.import_signature(raw_signature(proto, abi_params, isa, frame_aware));
    let self_name = func.declare_imported_user_function(UserExternalName::new(0, 0));
    let self_ref = func.import_function(ExtFuncData {
        name: ExternalName::user(self_name),
        signature: self_sig,
        colocated: true,
    });
    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

    // One CLIF block per SSA block. For a call the SSA entry block *is* the
    // function entry and receives the raw function parameters directly; an
    // OSR entry is a block of its own that jumps to the loop header.
    let entry = ssa.entry as usize;
    let call_entry = osr.is_none().then_some(entry);
    let start = osr.map_or(entry, |h| h.block as usize);
    let preamble = if frame_aware { 4 } else { 0 };
    let mut blocks: Vec<Option<cranelift_codegen::ir::Block>> = vec![None; ssa.blocks.len()];
    let entry_blk = b.create_block();
    b.append_block_params_for_function_params(entry_blk);
    if let Some(e) = call_entry {
        blocks[e] = Some(entry_blk);
    }
    for (i, blk) in ssa.blocks.iter().enumerate() {
        if Some(i) == call_entry {
            continue;
        }
        let cb = b.create_block();
        for &p in &blk.params {
            let ty = clif_ty(ssa.value_ty(p)).ok_or("from_ssa: unsupported block param")?;
            b.append_block_param(cb, ty);
        }
        blocks[i] = Some(cb);
    }

    // The compiled CFG: blocks in reverse postorder from the entry, their
    // predecessors, and which blocks compiled code reaches at all. A jump to
    // a block at or before its own position in reverse postorder is a loop
    // back edge.
    let rpo = cfg::order(ssa, start);
    let preds = cfg::predecessors(ssa);
    let mut rpo_pos = vec![0usize; ssa.blocks.len()];
    let mut reached = vec![false; ssa.blocks.len()];
    for (pos, &blk) in rpo.iter().enumerate() {
        rpo_pos[blk] = pos;
        reached[blk] = true;
    }

    b.switch_to_block(entry_blk);
    let views = views::Views::declare(&mut b, ssa);

    // The frame-aware raw ABI prepends `stack, closure, base, exec_ctx`.
    let frame = if frame_aware {
        let params = b.block_params(entry_blk);
        let abi_count = if osr.is_some() {
            0
        } else {
            ssa.blocks[entry].params.len()
        };
        if params.len() != preamble + abi_count {
            return Err("from_ssa: entry param count mismatch".into());
        }
        Some(FrameIo {
            closure: params[1],
            base: params[2],
            exec_ctx: params[3],
            linker,
            layout: varn_types::register_meta::FrameLayout::for_proto(proto),
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
        has_round: super::floats::has_round_support(isa),
        views,
        in_range_steps: induction::in_range_steps(ssa, &preds, &rpo_pos, &reached),
        carried: match osr {
            Some(h) => osr::carried(&mut b, ssa, h, &reached),
            None => Default::default(),
        },
    };

    let mut values: Vec<Option<Value>> = vec![None; ssa.values.len()];
    if let Some(h) = osr {
        let header = blocks[h.block as usize].expect("block created");
        osr::emit_entry(&mut b, &ctx, &mut values, h, header)?;
    }

    for i in rpo {
        let blk = &ssa.blocks[i];
        let cb = blocks[i].expect("block created");
        b.switch_to_block(cb);

        let params: Vec<Value> = b.block_params(cb).to_vec();
        let is_call_entry = Some(i) == call_entry;
        let base = if is_call_entry { preamble } else { 0 };
        if is_call_entry && params.len() != base + blk.params.len() {
            return Err("from_ssa: entry param count mismatch".into());
        }
        for (k, &p) in blk.params.iter().enumerate() {
            let pv = params[base + k];
            let kind = ssa.value_ty(p);
            // An entry parameter arrives as the ABI classifies it: a scalar
            // `param_kinds` entry as its native value, anything else only in
            // the home of register `1 + k` (see `heap_params`).
            let pv = if is_call_entry && !scalar(proto.param_kinds[k]) {
                let arg_reg = 1 + k as u32;
                if is_heap(kind) {
                    if ssa.reg(p) != arg_reg {
                        let boxed = use_heap(&mut b, &ctx, arg_reg)?;
                        def_heap(&mut b, &ctx, ssa.reg(p), boxed)?;
                    }
                    continue;
                }
                let boxed = use_heap(&mut b, &ctx, arg_reg)?;
                heap::unbox_dest(&mut b, kind, boxed)?
            } else if is_call_entry && proto.param_kinds[k] != kind {
                return Err(format!(
                    "from_ssa: parameter {k} is {kind:?} in the SSA but {:?} in the ABI",
                    proto.param_kinds[k]
                ));
            } else {
                pv
            };
            if is_heap(kind) {
                def_heap(&mut b, &ctx, ssa.reg(p), pv)?;
            } else {
                store::define_scalar(&mut b, &ctx, &mut values, p, pv);
            }
        }

        for inst in &blk.insts {
            match scalar::emit_inst(&mut b, &ctx, &mut values, &inst.op, inst.dest)? {
                Some(out) => land(&mut b, &ctx, &mut values, inst.dest, out)?,
                None if inst.dest.is_some() => {
                    return Err(format!("from_ssa: {:?} defines no value", inst.op));
                }
                None => {}
            }
            if !views::keeps_views(ssa, &inst.op) {
                ctx.views.clear(&mut b);
            }
        }
        // A back edge polls the collector only when its loop can allocate.
        let polls = |target: u32| {
            rpo_pos[target as usize] <= rpo_pos[i]
                && cfg::loop_may_collect(ssa, &preds, target as usize, i)
        };
        term::emit_term(&mut b, &ctx, &blocks, &values, &blk.term, polls)?;
    }

    // Nothing compiled jumps to the rest; each still needs a body.
    for (i, cb) in blocks.iter().enumerate() {
        if !reached[i] {
            b.switch_to_block(cb.expect("block created"));
            b.ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).expect("non-zero trap code"));
        }
    }

    b.seal_all_blocks();
    b.finalize();
    Ok((compile_piece(func, isa)?, frame_aware))
}

/// Whether `op` reaches the activation, the running closure or the runtime
/// even when every value it touches is a scalar.
fn needs_frame(ssa: &SsaProto, op: &SsaOp) -> bool {
    matches!(
        op,
        SsaOp::Call { .. }
            | SsaOp::CallNativeOp { .. }
            | SsaOp::MethodCall { .. }
            | SsaOp::LoadGlobalIdx(_)
            | SsaOp::LoadNativeGlobalIdx(_)
            | SsaOp::StoreGlobalIdx { .. }
            | SsaOp::MakeClosure { .. }
            | SsaOp::LoadCaptured { .. }
            | SsaOp::StoreCaptured { .. }
            | SsaOp::LoadUpvalue(_)
            | SsaOp::StoreUpvalue { .. }
            | SsaOp::CloseUpvalues { .. }
            | SsaOp::Try { .. }
            | SsaOp::PopTry
            | SsaOp::CatchParam { .. }
            | SsaOp::MakeEnumVariant { .. }
            | SsaOp::IntrinsicCall { .. }
            | SsaOp::ArrayGetIndex { .. }
            | SsaOp::ArraySetIndex { .. }
    ) || matches!(op, SsaOp::Convert { operand, conv }
        if !numeric::is_inline_convert(ssa, *operand, *conv))
}
