//! CLIF lowering from the portable typed SSA (`varn_types::ssa`).
//!
//! The primary lowering of the same SSA the interpreter runs: each
//! [`varn_types::ssa::SsaValue`] becomes one CLIF `Value` of its declared
//! physical type, and each typed `SsaOp` becomes the native instruction the
//! checker already proved — no re-derivation from operand types, no flow
//! lattice. It covers the whole portable family (scalars, aggregates, calls,
//! modules, suspension); anything still outside it returns `Err`, and the
//! function runs interpreted. The fallback is a missing optimization, never
//! a wrong result (Ley 10).
//!
//! Storage model (one rule, no re-derivation):
//! * **scalar** values (`Int`/`Float`/`Bool`) live in CLIF registers;
//! * **heap** values (`Str`/`Ref`/`Dyn`) live in their VM **home** slot, and
//!   every read loads from the home. Homes are the roots the GC knows about,
//!   so a heap value survives an allocation or a call by construction — this
//!   is the interpreter's own model, not a new one. A home's *address* is
//!   memoized per register and reused until something may push a frame (see
//!   [`store::home_addr`]); only the *value* is reloaded per read.
//!
//! Shapes admitted (see `docs/plans/2026-09-20-PLAN-PENDIENTE.md`):
//! * **leaf** — only scalars, no `this`/upvalues/alloc: raw `(exec_ctx, args…)
//!   -> scalar`;
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
//! * [`extra`] — full-family ops outside the scalar/heap core (spreads,
//!   names, super/extension, modules, suspension, disposal);
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

use super::abi::{raw_signature, Activation};
use super::lower::ClifLinker;
use super::native_abi::{NativeClass, NativeShape};
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
mod extra;
mod globals;
mod heap;
mod heapvalue;
mod induction;
mod numeric;
mod osr;
mod pinned;
mod pool;
mod props;
mod scalar;
mod stack_exit;
mod store;
mod term;
mod views;

use store::{clif_ty, drop_home_addrs, home_load, home_store, is_heap, land, load_value, Out};

/// Frame resources a frame-aware body needs for global access, calls and home
/// storage. `base` is the `FrameStore` activation id of a framed body; a
/// native body has none, and reaching for a home there is
/// [`NEEDS_ACTIVATION`].
pub(super) struct FrameIo<'a> {
    pub exec_ctx: Value,
    pub closure: Value,
    pub base: Option<Value>,
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
    /// Live `ExecCtx`, always real: entry param 0 in a leaf, param 3 in a
    /// frame-aware body (same value `frame.exec_ctx` carries there). Ningún
    /// camino lo recupera por thread-local.
    pub exec_ctx: Value,
    /// `Some` iff this body is frame-aware (heap/globals/calls).
    pub frame: Option<FrameIo<'a>>,
    pub activation: Activation,
    /// Register 0 — `this`, or a plain call's callee placeholder — read once
    /// at entry by any frame-aware body.
    pub this: Option<Value>,
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
    /// Memoized home-slot addresses by VM register (see
    /// [`store::home_addr`]). The driver clears it at each block and after
    /// any instruction that may push a frame; everything else leaves
    /// FrameStore vectors (hence every home address) in place.
    pub home_addrs: std::cell::RefCell<std::collections::HashMap<u32, Value>>,
    /// One reusable native-stack staging window (see [`call::ScratchWin`]).
    pub scratch: Option<call::ScratchWin>,
}

/// What a native lowering answers when the body reaches for a home: the body
/// needs a `FrameStore` activation, so the caller lowers it framed instead.
pub(super) const NEEDS_ACTIVATION: &str = "from_ssa: body needs a FrameStore activation";

pub(super) struct Lowered {
    pub piece: CompiledPiece,
    /// The body never touches the VM frame — scalars only, no calls, globals
    /// or throws — so a caller may enter it without pushing one.
    pub frameless: bool,
}

/// Attempt the SSA lowering in `activation`'s ABI. `Err` leaves the function
/// to the interpreter, except [`NEEDS_ACTIVATION`] from a native attempt,
/// which the caller answers with a framed one.
///
/// `osr_ip` selects the entry. `None` is a call: arguments in, execution from
/// the entry block. `Some(ip)` is an on-stack-replacement entry into a running
/// interpreted frame at the loop header starting at bytecode offset `ip`: no
/// arguments, the header's parameters and live values read from their homes,
/// and only the blocks reachable from the header compiled.
#[allow(clippy::too_many_arguments)]
pub(super) fn try_lower(
    proto: &FunctionProto,
    ssa: &SsaProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    osr_ip: Option<usize>,
    activation: Activation,
    mut debug: Option<&mut super::debug::ClifDebugSink>,
) -> Result<Lowered, String> {
    let osr = match osr_ip {
        None => None,
        Some(ip) => Some(
            ssa.loop_header_at(ip)
                .ok_or_else(|| format!("from_ssa: no loop header at ip {ip}"))?,
        ),
    };
    if osr.is_some() && activation == Activation::Native {
        return Err(NEEDS_ACTIVATION.into());
    }
    let nparams = proto.arity.saturating_sub(1);
    cfg::check_block_args(ssa)?;
    if proto.is_generator
        || proto.is_async
        || ssa.blocks.is_empty()
        || ssa.entry as usize >= ssa.blocks.len()
        || ssa.blocks[ssa.entry as usize].params.len() != nparams
        || proto.param_kinds.len() != nparams
    {
        return Err("from_ssa: not a leaf-compatible proto".into());
    }
    let scalar = |k: SlotKind| matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool);
    let frameless = !(ssa.values.iter().any(|v| is_heap(v.ty))
        || !proto.param_kinds.iter().all(|k| scalar(*k))
        || !scalar(proto.return_kind)
        || proto.has_this
        || ssa.has_this
        || proto.upvalue_count > 0
        || ssa.blocks.iter().any(|blk| {
            blk.insts.iter().any(|i| needs_frame(ssa, &i.op))
                || matches!(blk.term, SsaTerm::Throw(_))
        }));
    let frame_aware = activation == Activation::Framed || !frameless;

    let cc = isa.default_call_conv();
    let abi_params = if osr.is_some() { 0 } else { nparams };
    let shape = NativeShape::of_proto(proto);
    let signature = match activation {
        Activation::Native => shape.signature(),
        Activation::Framed => raw_signature(proto, abi_params, cc),
    };
    let mut func = Function::with_name_signature(UserFuncName::user(0, 0), signature.clone());
    let self_sig = func.import_signature(signature);
    let self_name = func.declare_imported_user_function(UserExternalName::new(0, 0));
    let self_ref = func.import_function(ExtFuncData {
        name: ExternalName::user(self_name),
        signature: self_sig,
        colocated: true,
        patchable: false,
    });
    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

    // One CLIF block per SSA block. For a call the SSA entry block *is* the
    // function entry and receives the raw function parameters directly; an
    // OSR entry is a block of its own that jumps to the loop header.
    let entry = ssa.entry as usize;
    let call_entry = osr.is_none().then_some(entry);
    let start = osr.map_or(entry, |h| h.block as usize);
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

    let entry_params = b.block_params(entry_blk).to_vec();
    let expected = match activation {
        Activation::Native => 2 + shape.params.iter().map(|c| c.words()).sum::<usize>(),
        Activation::Framed => 4 + abi_params,
    };
    if entry_params.len() != expected {
        return Err("from_ssa: entry param count mismatch".into());
    }
    let (exec_ctx, closure, base) = match activation {
        Activation::Native => (entry_params[0], entry_params[1], None),
        Activation::Framed => (entry_params[3], entry_params[1], Some(entry_params[2])),
    };
    let frame = frame_aware.then(|| FrameIo {
        exec_ctx,
        closure,
        base,
        linker,
        layout: varn_types::register_meta::FrameLayout::for_proto(proto),
    });
    let this = match (&frame, activation) {
        (None, _) => None,
        (Some(_), Activation::Native) => {
            let v = b.ins().iconcat(entry_params[2], entry_params[3]);
            b.declare_value_needs_stack_map(v);
            Some(v)
        }
        (Some(f), Activation::Framed) => {
            let homes = super::homes::Homes {
                exec_ctx,
                base: base.expect("a framed body has an activation"),
                layout: &f.layout,
                offsets: &helpers.frame_layout,
            };
            let v = homes.load(&mut b, 0);
            b.declare_value_needs_stack_map(v);
            Some(v)
        }
    };
    let scratch = call::ScratchWin::create(&mut b, call::scratch_max(ssa));
    let ctx = Ctx {
        cc,
        helpers,
        ssa,
        proto,
        constants,
        self_ref,
        exec_ctx,
        frame,
        activation,
        this,
        has_round: super::floats::has_round_support(isa),
        views,
        in_range_steps: induction::in_range_steps(ssa, &preds, &rpo_pos, &reached),
        home_addrs: Default::default(),
        scratch,
        carried: match osr {
            Some(h) => osr::carried(&mut b, ssa, h, &reached),
            None => Default::default(),
        },
    };

    if let Some(frame) = &ctx.frame {
        stack_exit::publish(&mut b, helpers, frame.exec_ctx);
    }
    let mut values: Vec<Option<Value>> = vec![None; ssa.values.len()];
    if let Some(h) = osr {
        let header = blocks[h.block as usize].expect("block created");
        osr::emit_entry(&mut b, &ctx, &mut values, h, header)?;
    }
    let pins = match osr {
        Some(_) => std::collections::BTreeMap::new(),
        None => pinned::compute(ssa, &preds, &rpo_pos, &reached),
    };
    if super::trace() && !pins.is_empty() {
        let mut flat: Vec<String> = Vec::new();
        for (pre, objs) in &pins {
            flat.push(format!("{pre}:{objs:?}"));
        }
        eprintln!(
            "clif: pins {} [{}]",
            proto.name.as_deref().unwrap_or("<module>"),
            flat.join(" ")
        );
    }

    for (n, i) in rpo.iter().enumerate() {
        let i = *i;
        let blk = &ssa.blocks[i];
        let cb = blocks[i].expect("block created");
        // `Views::declare` above already emitted the initial clear into the
        // entry block, so on the first iteration the builder is both current
        // and partial there: re-switching to it trips Cranelift's
        // "fill your block before switching" (debug-only, but the release
        // build silently keeps the stray instructions). A self-switch is a
        // no-op anyway, so it is skipped; every later switch leaves a block
        // its terminator just filled.
        if !(n == 0 && Some(i) == call_entry) {
            b.switch_to_block(cb);
        }
        // Home addresses memoize per block only: reusing an address across
        // blocks needs a dominance proof the map does not carry (a use
        // reachable around the defining block reads garbage). Within one
        // block every definition textually precedes its uses.
        drop_home_addrs(&ctx);

        let params: Vec<Value> = b.block_params(cb).to_vec();
        let is_call_entry = Some(i) == call_entry;
        let mut word = 4;
        for (k, &p) in blk.params.iter().enumerate() {
            let kind = ssa.value_ty(p);
            let pv = if is_call_entry {
                entry_param(&mut b, &ctx, &shape, &params, &mut word, k, kind)?
            } else {
                params[k]
            };
            store::define(&mut b, &ctx, &mut values, p, pv);
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
            if may_push_frame(&ctx, &inst.op) {
                drop_home_addrs(&ctx);
            }
        }
        // A back edge polls the collector only when its loop can allocate.
        let polls = |target: u32| {
            rpo_pos[target as usize] <= rpo_pos[i]
                && cfg::loop_may_collect(ssa, &preds, target as usize, i)
        };
        if let Some(objects) = pins.get(&i) {
            arrays::prefill(&mut b, &ctx, &values, objects)?;
        }
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
    b.finalize(isa.frontend_config());
    super::debug::capture_ir(&mut debug, &func);
    super::debug::capture_kinds_ssa(&mut debug, ssa);
    Ok(Lowered {
        piece: compile_piece(func, isa)?,
        frameless,
    })
}

/// SSA entry parameter `k` (register `1 + k`) as the ABI delivers it. A native
/// body reads it in its class's form starting at entry word `*word`; a framed
/// one takes a scalar-declared argument from its word and anything else from
/// its home.
fn entry_param(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    shape: &NativeShape,
    params: &[Value],
    word: &mut usize,
    k: usize,
    kind: SlotKind,
) -> Result<Value, String> {
    let declared = ctx.proto.param_kinds[k];
    let mismatch =
        || format!("from_ssa: parameter {k} is {kind:?} in the SSA but {declared:?} in the ABI");
    match ctx.activation {
        Activation::Native => {
            let class = shape.params[1 + k];
            let at = *word;
            *word += class.words();
            match class {
                NativeClass::Boxed => {
                    let boxed = b.ins().iconcat(params[at], params[at + 1]);
                    if is_heap(kind) {
                        Ok(boxed)
                    } else {
                        heap::unbox_dest(b, kind, boxed)
                    }
                }
                NativeClass::Word | NativeClass::Float if declared == kind => Ok(params[at]),
                NativeClass::Word | NativeClass::Float => Err(mismatch()),
            }
        }
        Activation::Framed => {
            if !matches!(declared, SlotKind::Int | SlotKind::Float | SlotKind::Bool) {
                store::load_home_value(b, ctx, 1 + k as u32, kind)
            } else if declared != kind {
                Err(mismatch())
            } else {
                Ok(params[4 + k])
            }
        }
    }
}

/// Whether `op` can push a VM frame (a real call, a module load, suspension
/// or user code behind a dynamic access): after it, memoized home addresses
/// are stale. Pure heap allocation does NOT push — the FrameStore vectors
/// only reallocate on frame push — so builders and readers stay cached
/// across it. Conservative on doubt: `CallNativeOp` stays `true` because a
/// higher-order native could call back, and every dynamic access may run
/// user code.
fn may_push_frame(ctx: &Ctx<'_>, op: &SsaOp) -> bool {
    match op {
        SsaOp::Call { .. }
        | SsaOp::New { .. }
        | SsaOp::CallNativeOp { .. }
        | SsaOp::MethodCall { .. }
        | SsaOp::IterCall { .. }
        | SsaOp::SuperCall { .. }
        | SsaOp::SuperMethodCall { .. }
        | SsaOp::ExtensionCall { .. }
        | SsaOp::CallSpread { .. }
        | SsaOp::Dispose { .. }
        | SsaOp::GetProperty { .. }
        | SsaOp::SetProperty { .. }
        | SsaOp::GetIndex { .. }
        | SsaOp::SetIndex { .. }
        | SsaOp::GetPropertyMaybe { .. }
        | SsaOp::GetSymbol { .. }
        | SsaOp::BuildObjectSpread { .. }
        | SsaOp::Spawn { .. }
        | SsaOp::LoadModule { .. }
        | SsaOp::Await { .. }
        | SsaOp::Yield { .. } => true,
        SsaOp::SelfCall { .. } => ctx.frame.is_some(),
        _ => false,
    }
}

/// Whether `op` reaches the activation, the running closure or the runtime
/// even when every value it touches is a scalar.
fn needs_frame(ssa: &SsaProto, op: &SsaOp) -> bool {
    matches!(
        op,
        SsaOp::Call { .. }
            | SsaOp::New { .. }
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
            | SsaOp::LoadGlobal(_)
            | SsaOp::StoreGlobal { .. }
            | SsaOp::BuildTuple { .. }
            | SsaOp::BuildArraySpread { .. }
            | SsaOp::BuildObjectSpread { .. }
            | SsaOp::ObjectMerge { .. }
            | SsaOp::ObjectRest { .. }
            | SsaOp::GetPropertyMaybe { .. }
            | SsaOp::AssertNotNull { .. }
            | SsaOp::BindMethod { .. }
            | SsaOp::ArrayExtend { .. }
            | SsaOp::WrapSpread { .. }
            | SsaOp::Range { .. }
            | SsaOp::GetSymbol { .. }
            | SsaOp::IterCall { .. }
            | SsaOp::SuperCall { .. }
            | SsaOp::SuperMethodCall { .. }
            | SsaOp::ExtensionCall { .. }
            | SsaOp::CallSpread { .. }
            | SsaOp::LoadModule { .. }
            | SsaOp::ModuleSlot { .. }
            | SsaOp::StoreModuleSlot { .. }
            | SsaOp::Await { .. }
            | SsaOp::Spawn { .. }
            | SsaOp::Yield { .. }
            | SsaOp::Dispose { .. }
    ) || matches!(op, SsaOp::Convert { operand, conv }
        if !numeric::is_inline_convert(ssa, *operand, *conv))
}
