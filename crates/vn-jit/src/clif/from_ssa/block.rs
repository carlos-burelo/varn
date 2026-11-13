use cranelift_codegen::ir::{types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use vn_types::register_meta::SlotKind;
use vn_types::ssa::SsaProto;

use super::super::abi::Activation;
use super::super::native_abi::{NativeClass, NativeShape};
use super::frame_policy::may_push_frame;
use super::{arrays, cfg, exceptions, globals, heap, scalar, setup, store, term, Ctx};

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_block(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    view: &[Option<Block>],
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    shape: &NativeShape,
    global_src: &std::collections::BTreeMap<u32, u32>,
    pins: &std::collections::BTreeMap<usize, Vec<u32>>,
    hoisted: &mut std::collections::BTreeMap<(usize, u32), Value>,
    hoist_at: &std::collections::BTreeMap<usize, Vec<(usize, u32)>>,
    loop_bodies: &rustc_hash::FxHashMap<usize, rustc_hash::FxHashSet<usize>>,
    landing_live: &setup::LandingLive,
    ssa_idx: usize,
    clif_blk: Block,
    is_call_entry: bool,
    do_prefill: bool,
    elide_flag: bool,
    is_clone: bool,
    skip_term: bool,
    rpo_n: usize,
    keep_defs: bool,
) -> Result<(), String> {
    let blk = &ssa.blocks[ssa_idx];
    exceptions::set_block_stack(ctx, ssa_idx);
    ctx.emitting_block.set(ssa_idx);
    let saved = if landing_live.contains_key(&ssa_idx) {
        Some(values.to_vec())
    } else {
        None
    };
    if !(rpo_n == 0 && is_call_entry) {
        b.switch_to_block(clif_blk);
    }

    if let Some(live) = landing_live.get(&ssa_idx)
        && ctx.frame.is_some()
    {
        for &v in live {
            let x = store::load_home_value(b, ctx, ctx.ssa.reg(v), ctx.ssa.value_ty(v))?;
            store::define(b, ctx, values, v, x);
        }
        if let Some(frame) = &ctx.frame {
            let zero = b.ins().iconst(types::I64, 0);
            b.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                zero,
                frame.exec_ctx,
                ctx.helpers.jit_error_flag_offset as i32,
            );
        }
    }

    store::drop_home_addrs(ctx);

    let params: Vec<Value> = b.block_params(clif_blk).to_vec();
    let mut word = ctx.arg_word_base;
    for (k, &p) in blk.params.iter().enumerate() {
        let kind = ssa.value_ty(p);
        let pv = if is_call_entry {
            entry_param(b, ctx, shape, &params, &mut word, k, kind)?
        } else {
            params[k]
        };
        store::define(b, ctx, values, p, pv);
        ctx.views.forget(b, p);
    }
    if landing_live.contains_key(&ssa_idx) {
        let err_val = ctx
            .pure_try
            .borrow()
            .landing_error
            .get(&ssa_idx)
            .copied()
            .ok_or_else(|| format!("from_ssa: landing {ssa_idx} without error value"))?;
        let err_param = params
            .get(blk.params.len())
            .copied()
            .ok_or_else(|| format!("from_ssa: landing {ssa_idx} without error param"))?;
        b.declare_value_needs_stack_map(err_param);
        store::define(b, ctx, values, err_val, err_param);
    }

    let old_elide = ctx.elide.replace(elide_flag);
    let mut fwd: std::collections::BTreeMap<(u32, u16), u32> = Default::default();
    let canon = |o: u32| -> u32 { global_src.get(&o).map(|s| 0x8000_0000u32 | s).unwrap_or(o) };
    for inst in &blk.insts {
        if let vn_types::ssa::SsaOp::GetFixedField { object, slot, .. } = &inst.op {
            let k = (canon(*object), *slot);
            if let (Some(d), Some(&src)) = (inst.dest, fwd.get(&k))
                && ctx.ssa.value_ty(d) == ctx.ssa.value_ty(src)
            {
                let v = store::load_value(b, ctx, values, src)?;
                ctx.views.forget(b, d);
                store::define(b, ctx, values, d, v);
                continue;
            }
        }
        if let (Some(d), vn_types::ssa::SsaOp::LoadGlobalIdx(slot)) = (inst.dest, &inst.op) {
            let mut best: Option<(Value, usize)> = None;
            for ((h, s), v) in hoisted.iter() {
                if *s != *slot {
                    continue;
                }
                let Some(bd) = loop_bodies.get(h) else {
                    continue;
                };
                if !bd.contains(&ssa_idx) {
                    continue;
                }
                match &best {
                    Some((_, bs)) if *bs <= bd.len() => {}
                    _ => best = Some((*v, bd.len())),
                }
            }
            if let Some((v, _)) = best {
                store::land(b, ctx, values, Some(d), store::Out::Boxed(v))?;
                continue;
            }
        }
        if let (vn_types::ssa::SsaOp::Call { .. }, Some(d)) = (&inst.op, inst.dest)
            && ctx.fused_awaits.contains_key(&d)
        {
            continue;
        }
        if let Some(d) = inst.dest {
            ctx.views.forget(b, d);
        }
        ctx.site_ip.set(inst.ip);
        b.set_srcloc(cranelift_codegen::ir::SourceLoc::new(inst.ip));
        if super::frame_policy::may_push_frame(ctx, &inst.op) {
            super::exceptions::note_site(b, ctx);
        }
        match scalar::emit_inst(b, ctx, values, &inst.op, inst.dest)? {
            Some(out) => store::land(b, ctx, values, inst.dest, out)?,
            None if inst.dest.is_some() => {
                return Err(format!("from_ssa: {:?} defines no value", inst.op));
            }
            None => {}
        }
        if let Some(d) = inst.dest
            && ctx.try_live.contains(&d)
        {
            debug_assert!(ctx.frame.is_some());
            let boxed = heap::boxed_value(b, ctx, values, d)?;
            store::home_store(b, ctx, ctx.ssa.reg(d), boxed)?;
        }
        match &inst.op {
            vn_types::ssa::SsaOp::GetFixedField { object, slot, .. } => {
                if let Some(d) = inst.dest {
                    fwd.insert((canon(*object), *slot), d);
                }
            }
            vn_types::ssa::SsaOp::SetFixedField {
                object,
                value,
                slot,
                ..
            } => {
                fwd.insert((canon(*object), *slot), *value);
            }
            vn_types::ssa::SsaOp::StoreGlobalIdx { slot, .. } => {
                let g = 0x8000_0000u32 | slot;
                fwd.retain(|k, _| k.0 != g);
                ctx.views.invalidate_global(b, *slot);
                hoisted.retain(|(_, s), _| s != slot);
            }
            vn_types::ssa::SsaOp::LoadGlobalIdx(..) => {}
            vn_types::ssa::SsaOp::ConstInt(_)
            | vn_types::ssa::SsaOp::ConstFloat(_)
            | vn_types::ssa::SsaOp::ConstBool(_)
            | vn_types::ssa::SsaOp::ConstNull
            | vn_types::ssa::SsaOp::ConstStr(_)
            | vn_types::ssa::SsaOp::Binary { .. }
            | vn_types::ssa::SsaOp::Unary { .. }
            | vn_types::ssa::SsaOp::SelfCall { .. }
            | vn_types::ssa::SsaOp::Call { .. }
            | vn_types::ssa::SsaOp::AllocInstance { .. }
            | vn_types::ssa::SsaOp::ArrayGetIndex { .. }
            | vn_types::ssa::SsaOp::BytesGetIndex { .. }
            | vn_types::ssa::SsaOp::ArraySetIndex { .. }
            | vn_types::ssa::SsaOp::BytesSetIndex { .. }
            | vn_types::ssa::SsaOp::ConstChar(_)
            | vn_types::ssa::SsaOp::ConstBigInt(_)
            | vn_types::ssa::SsaOp::ConstDecimal(_)
            | vn_types::ssa::SsaOp::MakeEnumVariant { .. }
            | vn_types::ssa::SsaOp::IntrinsicCall { .. }
            | vn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
            | vn_types::ssa::SsaOp::Try { .. }
            | vn_types::ssa::SsaOp::PopTry
            | vn_types::ssa::SsaOp::CatchParam { .. }
            | vn_types::ssa::SsaOp::MakeClosure { .. }
            | vn_types::ssa::SsaOp::LoadCaptured { .. }
            | vn_types::ssa::SsaOp::StoreCaptured { .. }
            | vn_types::ssa::SsaOp::LoadUpvalue(_)
            | vn_types::ssa::SsaOp::StoreUpvalue { .. }
            | vn_types::ssa::SsaOp::CloseUpvalues { .. }
            | vn_types::ssa::SsaOp::Cast { .. }
            | vn_types::ssa::SsaOp::Convert { .. }
            | vn_types::ssa::SsaOp::IsNull { .. }
            | vn_types::ssa::SsaOp::Typeof { .. }
            | vn_types::ssa::SsaOp::ToString { .. }
            | vn_types::ssa::SsaOp::IsArray { .. }
            | vn_types::ssa::SsaOp::GetEnumTag { .. }
            | vn_types::ssa::SsaOp::ObjectKeys { .. }
            | vn_types::ssa::SsaOp::BuildStr { .. }
            | vn_types::ssa::SsaOp::BuildArray { .. }
            | vn_types::ssa::SsaOp::BuildMap { .. }
            | vn_types::ssa::SsaOp::BuildObject { .. }
            | vn_types::ssa::SsaOp::GetProperty { .. }
            | vn_types::ssa::SsaOp::SetProperty { .. }
            | vn_types::ssa::SsaOp::GetIndex { .. }
            | vn_types::ssa::SsaOp::SetIndex { .. }
            | vn_types::ssa::SsaOp::ArrayLength { .. }
            | vn_types::ssa::SsaOp::StrLength { .. }
            | vn_types::ssa::SsaOp::BytesLength { .. }
            | vn_types::ssa::SsaOp::MethodCall { .. }
            | vn_types::ssa::SsaOp::CallNativeOp { .. }
            | vn_types::ssa::SsaOp::ArrayPush { .. }
            | vn_types::ssa::SsaOp::This
            | vn_types::ssa::SsaOp::MakeClass { .. }
            | vn_types::ssa::SsaOp::DeclareLayout { .. }
            | vn_types::ssa::SsaOp::DefineMethod { .. }
            | vn_types::ssa::SsaOp::GetSuper { .. }
            | vn_types::ssa::SsaOp::LoadGlobal(_)
            | vn_types::ssa::SsaOp::StoreGlobal { .. }
            | vn_types::ssa::SsaOp::BuildTuple { .. }
            | vn_types::ssa::SsaOp::BuildArraySpread { .. }
            | vn_types::ssa::SsaOp::BuildObjectSpread { .. }
            | vn_types::ssa::SsaOp::ObjectMerge { .. }
            | vn_types::ssa::SsaOp::ObjectRest { .. }
            | vn_types::ssa::SsaOp::GetPropertyMaybe { .. }
            | vn_types::ssa::SsaOp::AssertNotNull { .. }
            | vn_types::ssa::SsaOp::BindMethod { .. }
            | vn_types::ssa::SsaOp::ArrayExtend { .. }
            | vn_types::ssa::SsaOp::WrapSpread { .. }
            | vn_types::ssa::SsaOp::Range { .. }
            | vn_types::ssa::SsaOp::SuperCall { .. }
            | vn_types::ssa::SsaOp::SuperMethodCall { .. }
            | vn_types::ssa::SsaOp::ExtensionCall { .. }
            | vn_types::ssa::SsaOp::CallSpread { .. }
            | vn_types::ssa::SsaOp::LoadModule { .. }
            | vn_types::ssa::SsaOp::ModuleSlot { .. }
            | vn_types::ssa::SsaOp::StoreModuleSlot { .. }
            | vn_types::ssa::SsaOp::Await { .. }
            | vn_types::ssa::SsaOp::Spawn { .. }
            | vn_types::ssa::SsaOp::Yield { .. }
            | vn_types::ssa::SsaOp::Dispose { .. } => {
                if !super::view_invalidation::keeps_views(ssa, &inst.op) {
                    fwd.clear();
                    ctx.views.clear_instances(b);
                    hoisted.clear();
                }
            }
        }
        if !super::view_invalidation::keeps_views(ssa, &inst.op) {
            ctx.views.clear(b);
        }
        if may_push_frame(ctx, &inst.op) {
            store::drop_home_addrs(ctx);
        }
    }
    ctx.elide.set(old_elide);

    if !is_clone
        && let Some(hh) = hoist_at.get(&ssa_idx)
        && ctx.frame.is_some()
    {
        for (h, slot) in hh {
            if hoisted.contains_key(&(*h, *slot)) {
                continue;
            }
            let v = globals::emit_load(b, ctx, *slot, globals::Region::Module)?;
            hoisted.insert((*h, *slot), v);
        }
    }

    let polls = |target: u32| {
        rpo_pos[target as usize] <= rpo_pos[ssa_idx]
            && cfg::loop_may_collect(ssa, preds, target as usize, ssa_idx)
    };
    if do_prefill && let Some(objects) = pins.get(&ssa_idx) {
        arrays::prefill(b, ctx, values, objects)?;
    }
    if !skip_term {
        term::emit_term(b, ctx, view, values, &blk.term, polls)?;
    }
    if !keep_defs && let Some(saved) = saved {
        for (slot, cur) in values.iter_mut().enumerate() {
            if saved[slot].is_some() {
                *cur = saved[slot];
            }
        }
    }
    Ok(())
}

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
            let class = crate::clif::native_abi::NativeClass::of(shape.params[1 + k]);
            let at = *word;
            *word += class.words();
            match class {
                NativeClass::Boxed => {
                    let boxed = b.ins().iconcat(params[at], params[at + 1]);
                    if store::is_heap(kind) {
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
