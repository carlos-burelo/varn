use cranelift_codegen::ir::{types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::SsaProto;

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
    let mut word = 4;
    for (k, &p) in blk.params.iter().enumerate() {
        let kind = ssa.value_ty(p);
        let pv = if is_call_entry {
            entry_param(b, ctx, shape, &params, &mut word, k, kind)?
        } else {
            params[k]
        };
        store::define(b, ctx, values, p, pv);
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
        if let varn_types::ssa::SsaOp::GetFixedField { object, slot, .. } = &inst.op {
            let k = (canon(*object), *slot);
            if let (Some(d), Some(&src)) = (inst.dest, fwd.get(&k))
                && ctx.ssa.value_ty(d) == ctx.ssa.value_ty(src)
            {
                let v = store::load_value(b, ctx, values, src)?;
                store::define(b, ctx, values, d, v);
                continue;
            }
        }
        if let (Some(d), varn_types::ssa::SsaOp::LoadGlobalIdx(slot)) = (inst.dest, &inst.op) {
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
        if let (varn_types::ssa::SsaOp::Call { .. }, Some(d)) = (&inst.op, inst.dest)
            && ctx.fused_awaits.contains_key(&d)
        {
            continue;
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
            varn_types::ssa::SsaOp::GetFixedField { object, slot, .. } => {
                if let Some(d) = inst.dest {
                    fwd.insert((canon(*object), *slot), d);
                }
            }
            varn_types::ssa::SsaOp::SetFixedField {
                object,
                value,
                slot,
                ..
            } => {
                fwd.insert((canon(*object), *slot), *value);
            }
            varn_types::ssa::SsaOp::StoreGlobalIdx { slot, .. } => {
                let g = 0x8000_0000u32 | slot;
                fwd.retain(|k, _| k.0 != g);
                ctx.views.invalidate_global(b, *slot);
                hoisted.retain(|(_, s), _| s != slot);
            }
            varn_types::ssa::SsaOp::LoadGlobalIdx(..) => {}
            varn_types::ssa::SsaOp::ConstInt(_)
            | varn_types::ssa::SsaOp::ConstFloat(_)
            | varn_types::ssa::SsaOp::ConstBool(_)
            | varn_types::ssa::SsaOp::ConstNull
            | varn_types::ssa::SsaOp::ConstStr(_)
            | varn_types::ssa::SsaOp::Binary { .. }
            | varn_types::ssa::SsaOp::Unary { .. }
            | varn_types::ssa::SsaOp::SelfCall { .. }
            | varn_types::ssa::SsaOp::Call { .. }
            | varn_types::ssa::SsaOp::AllocInstance { .. }
            | varn_types::ssa::SsaOp::ArrayGetIndex { .. }
            | varn_types::ssa::SsaOp::BytesGetIndex { .. }
            | varn_types::ssa::SsaOp::ArraySetIndex { .. }
            | varn_types::ssa::SsaOp::BytesSetIndex { .. }
            | varn_types::ssa::SsaOp::ConstChar(_)
            | varn_types::ssa::SsaOp::ConstBigInt(_)
            | varn_types::ssa::SsaOp::ConstDecimal(_)
            | varn_types::ssa::SsaOp::MakeEnumVariant { .. }
            | varn_types::ssa::SsaOp::IntrinsicCall { .. }
            | varn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
            | varn_types::ssa::SsaOp::Try { .. }
            | varn_types::ssa::SsaOp::PopTry
            | varn_types::ssa::SsaOp::CatchParam { .. }
            | varn_types::ssa::SsaOp::MakeClosure { .. }
            | varn_types::ssa::SsaOp::LoadCaptured { .. }
            | varn_types::ssa::SsaOp::StoreCaptured { .. }
            | varn_types::ssa::SsaOp::LoadUpvalue(_)
            | varn_types::ssa::SsaOp::StoreUpvalue { .. }
            | varn_types::ssa::SsaOp::CloseUpvalues { .. }
            | varn_types::ssa::SsaOp::Cast { .. }
            | varn_types::ssa::SsaOp::Convert { .. }
            | varn_types::ssa::SsaOp::IsNull { .. }
            | varn_types::ssa::SsaOp::Typeof { .. }
            | varn_types::ssa::SsaOp::ToString { .. }
            | varn_types::ssa::SsaOp::IsArray { .. }
            | varn_types::ssa::SsaOp::GetEnumTag { .. }
            | varn_types::ssa::SsaOp::ObjectKeys { .. }
            | varn_types::ssa::SsaOp::BuildStr { .. }
            | varn_types::ssa::SsaOp::BuildArray { .. }
            | varn_types::ssa::SsaOp::BuildMap { .. }
            | varn_types::ssa::SsaOp::BuildObject { .. }
            | varn_types::ssa::SsaOp::GetProperty { .. }
            | varn_types::ssa::SsaOp::SetProperty { .. }
            | varn_types::ssa::SsaOp::GetIndex { .. }
            | varn_types::ssa::SsaOp::SetIndex { .. }
            | varn_types::ssa::SsaOp::ArrayLength { .. }
            | varn_types::ssa::SsaOp::StrLength { .. }
            | varn_types::ssa::SsaOp::BytesLength { .. }
            | varn_types::ssa::SsaOp::MethodCall { .. }
            | varn_types::ssa::SsaOp::CallNativeOp { .. }
            | varn_types::ssa::SsaOp::ArrayPush { .. }
            | varn_types::ssa::SsaOp::This
            | varn_types::ssa::SsaOp::MakeClass { .. }
            | varn_types::ssa::SsaOp::DeclareLayout { .. }
            | varn_types::ssa::SsaOp::DefineMethod { .. }
            | varn_types::ssa::SsaOp::GetSuper { .. }
            | varn_types::ssa::SsaOp::LoadGlobal(_)
            | varn_types::ssa::SsaOp::StoreGlobal { .. }
            | varn_types::ssa::SsaOp::BuildTuple { .. }
            | varn_types::ssa::SsaOp::BuildArraySpread { .. }
            | varn_types::ssa::SsaOp::BuildObjectSpread { .. }
            | varn_types::ssa::SsaOp::ObjectMerge { .. }
            | varn_types::ssa::SsaOp::ObjectRest { .. }
            | varn_types::ssa::SsaOp::GetPropertyMaybe { .. }
            | varn_types::ssa::SsaOp::AssertNotNull { .. }
            | varn_types::ssa::SsaOp::BindMethod { .. }
            | varn_types::ssa::SsaOp::ArrayExtend { .. }
            | varn_types::ssa::SsaOp::WrapSpread { .. }
            | varn_types::ssa::SsaOp::Range { .. }
            | varn_types::ssa::SsaOp::GetSymbol { .. }
            | varn_types::ssa::SsaOp::IterCall { .. }
            | varn_types::ssa::SsaOp::SuperCall { .. }
            | varn_types::ssa::SsaOp::SuperMethodCall { .. }
            | varn_types::ssa::SsaOp::ExtensionCall { .. }
            | varn_types::ssa::SsaOp::CallSpread { .. }
            | varn_types::ssa::SsaOp::LoadModule { .. }
            | varn_types::ssa::SsaOp::ModuleSlot { .. }
            | varn_types::ssa::SsaOp::StoreModuleSlot { .. }
            | varn_types::ssa::SsaOp::Await { .. }
            | varn_types::ssa::SsaOp::Spawn { .. }
            | varn_types::ssa::SsaOp::Yield { .. }
            | varn_types::ssa::SsaOp::Dispose { .. } => {
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
            let class = shape.params[1 + k];
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
