use super::super::abi::{raw_signature, Activation};
use super::super::lower::ClifLinker;
use super::super::native_abi::{NativeClass, NativeShape};
use super::super::piece::compile_piece;
use super::frame_policy::{may_push_frame, needs_frame};
use super::store::{clif_ty, drop_home_addrs, is_heap, land};
use super::{
    arrays, call, cfg, globals, heap, induction, osr, pinned, ranges, scalar, stack_exit, store,
    term, views, Ctx, FrameIo, Lowered, NEEDS_ACTIVATION,
};
use crate::JitHelpers;
use cranelift_codegen::ir::{
    types, Block, BlockArg, ExtFuncData, ExternalName, Function, InstBuilder, UserExternalName,
    UserFuncName, Value,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{OsrRequest, SsaEntryKind, SsaProto, SsaTerm};
use varn_types::{FunctionProto, VmValue};
pub(in crate::clif) fn try_lower(
    proto: &FunctionProto,
    ssa: &SsaProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    osr_req: Option<OsrRequest>,
    activation: Activation,
    mut debug: Option<&mut super::super::debug::ClifDebugSink>,
) -> Result<Lowered, String> {
    let osr = match osr_req {
        None => None,
        Some(req) => Some(
            ssa.secondary_at(req.ip)
                .ok_or_else(|| format!("from_ssa: no secondary entry at ip {}", req.ip))?,
        ),
    };
    if (osr.is_some() || proto.resumes_in_interpreter()) && activation == Activation::Native {
        return Err(NEEDS_ACTIVATION.into());
    }
    let nparams = proto.arity.saturating_sub(1);
    cfg::check_block_args(ssa)?;
    if ssa.blocks.is_empty()
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

    let entry = ssa.entry as usize;
    let call_entry = osr.is_none().then_some(entry);
    let start = osr.map_or(entry, |h| h.block as usize);
    let mut landing_live: std::collections::BTreeMap<usize, Vec<u32>> = Default::default();
    let mut landing_error: std::collections::BTreeMap<usize, u32> = Default::default();
    for e in &ssa.secondary_entries {
        if e.kind == SsaEntryKind::Catch {
            landing_live.insert(e.block as usize, e.live.clone());
        }
    }
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let varn_types::ssa::SsaOp::Try {
                catch_ip,
                catch_value,
                ..
            } = &inst.op
            {
                if let Some(e) = ssa.catch_at(*catch_ip as usize) {
                    let h = e.block as usize;
                    if let Some(prev) = landing_error.get(&h) {
                        if *prev != *catch_value {
                            return Err("from_ssa: handler shared by two try values".into());
                        }
                    } else {
                        landing_error.insert(h, *catch_value);
                    }
                }
            }
        }
    }
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
        if landing_live.contains_key(&i) {
            if !blk.params.is_empty() {
                return Err("from_ssa: catch block with params".into());
            }
            b.append_block_param(cb, types::I128);
        }
        blocks[i] = Some(cb);
    }

    let mut rpo = cfg::order(ssa, start);
    let preds = cfg::predecessors(ssa);
    let mut rpo_pos = vec![0usize; ssa.blocks.len()];
    let mut reached = vec![false; ssa.blocks.len()];
    for (pos, &blk) in rpo.iter().enumerate() {
        rpo_pos[blk] = pos;
        reached[blk] = true;
    }
    let full_rpo = if osr.is_some() {
        cfg::order(ssa, entry)
    } else {
        rpo.clone()
    };
    let mut full_reached = vec![false; ssa.blocks.len()];
    for &blk in &full_rpo {
        full_reached[blk] = true;
    }
    let full_entry_stacks = cfg::try_entry_stacks(ssa, &full_rpo, &full_reached)
        .ok_or_else(|| "from_ssa: inconsistent try stacks".to_owned())?;
    if osr.is_some() {
        let resume_blk = start;
        let enclosing = full_entry_stacks
            .get(resume_blk)
            .cloned()
            .unwrap_or_default();
        let mut stack: Vec<usize> = enclosing;
        while let Some(b) = stack.pop() {
            if b >= reached.len() || reached[b] {
                continue;
            }
            reached[b] = true;
            rpo_pos[b] = rpo.len();
            rpo.push(b);
            match &ssa.blocks[b].term {
                SsaTerm::Jump { target, .. } => {
                    let s = *target as usize;
                    if s < reached.len() && !reached[s] {
                        stack.push(s);
                    }
                }
                SsaTerm::Branch {
                    then_blk, else_blk, ..
                } => {
                    for s in [*then_blk as usize, *else_blk as usize] {
                        if s < reached.len() && !reached[s] {
                            stack.push(s);
                        }
                    }
                }
                SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => {}
            }
            for inst in &ssa.blocks[b].insts {
                if let varn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op {
                    if let Some(e) = ssa.catch_at(*catch_ip as usize) {
                        let h = e.block as usize;
                        if h < reached.len() && !reached[h] {
                            stack.push(h);
                        }
                    }
                }
            }
        }
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
            let homes = super::super::homes::Homes {
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
    let (no_overflow, version_plans) = if osr.is_some() {
        (rustc_hash::FxHashSet::default(), Vec::new())
    } else {
        ranges::analyze(ssa, &preds, &rpo_pos, &reached)
    };
    let entry_stacks = full_entry_stacks;
    let mut clif_map: std::collections::BTreeMap<usize, Block> = Default::default();
    for h in landing_live.keys() {
        if let Some(cb) = blocks[*h] {
            clif_map.insert(*h, cb);
        }
    }
    let err_ret = if frame_aware {
        Some(b.create_block())
    } else {
        None
    };
    let mut ctx = Ctx {
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
        has_round: super::super::floats::has_round_support(isa),
        views,
        in_range_steps: induction::in_range_steps(ssa, &preds, &rpo_pos, &reached),
        no_overflow,
        fast_bounds: rustc_hash::FxHashSet::default(),
        elide: std::cell::Cell::new(false),
        home_addrs: Default::default(),
        scratch,
        carried: match osr {
            Some(h) => osr::carried(&mut b, ssa, h, &reached),
            None => osr::versioned(&mut b, ssa, &version_plans, &preds, &rpo_pos, &reached),
        },
        try_live: try_live_set(ssa),
        pure_try: Default::default(),
    };
    {
        let mut st = ctx.pure_try.borrow_mut();
        st.entry_stacks = entry_stacks;
        st.landing_error = landing_error;
        st.clif_map = clif_map;
        st.err_ret = err_ret;
        st.stack = Vec::new();
    }

    if let Some(frame) = &ctx.frame {
        stack_exit::publish(&mut b, helpers, frame.exec_ctx);
    }
    if osr.is_none() {
        if let Some(frame) = &ctx.frame {
            let zero = b.ins().iconst(types::I64, 0);
            b.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                zero,
                frame.exec_ctx,
                helpers.jit_error_flag_offset as i32,
            );
        }
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
    if super::super::trace() && !pins.is_empty() {
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

    let mut global_src: std::collections::BTreeMap<u32, u32> = Default::default();
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let (Some(d), varn_types::ssa::SsaOp::LoadGlobalIdx(slot)) = (inst.dest, &inst.op) {
                global_src.insert(d, *slot);
            }
        }
    }

    let (loop_headers, loop_bodies) = ranges::loop_sets(ssa, &preds, &rpo_pos, &reached);
    let mut hoist_at: std::collections::BTreeMap<usize, Vec<(usize, u32)>> =
        std::collections::BTreeMap::new();
    for &h in &loop_headers {
        let body = &loop_bodies[&h];
        let outside: Vec<usize> = preds[h]
            .iter()
            .copied()
            .filter(|p| reached[*p] && !body.contains(p))
            .collect();
        if outside.len() != 1 || ctx.frame.is_none() {
            continue;
        }
        let mut stored: rustc_hash::FxHashSet<u32> = rustc_hash::FxHashSet::default();
        let mut loaded: Vec<u32> = Vec::new();
        let mut pure = true;
        for &b in body {
            if !reached[b] {
                continue;
            }
            for inst in &ssa.blocks[b].insts {
                match &inst.op {
                    varn_types::ssa::SsaOp::LoadGlobalIdx(slot) => {
                        if !loaded.contains(slot) {
                            loaded.push(*slot);
                        }
                    }
                    varn_types::ssa::SsaOp::StoreGlobalIdx { slot, .. } => {
                        stored.insert(*slot);
                    }
                    op => {
                        if !ranges::version_op_ok(ssa, op) {
                            pure = false;
                        }
                    }
                }
            }
        }
        if !pure {
            continue;
        }
        let slots: Vec<u32> = loaded.into_iter().filter(|s| !stored.contains(s)).collect();
        if !slots.is_empty() {
            hoist_at
                .entry(outside[0])
                .or_default()
                .extend(slots.into_iter().map(|s| (h, s)));
        }
    }
    let mut hoisted: std::collections::BTreeMap<(usize, u32), Value> =
        std::collections::BTreeMap::new();

    let plans = version_plans;
    let mut plan_by_pre: std::collections::BTreeMap<usize, usize> =
        std::collections::BTreeMap::new();
    for (idx, pl) in plans.iter().enumerate() {
        plan_by_pre.entry(pl.preheader).or_insert(idx);
        ctx.fast_bounds.extend(pl.elide.iter().copied());
    }
    if super::super::trace() && (!plans.is_empty() || !hoist_at.is_empty()) {
        eprintln!(
            "clif: versions {} hoists {:?} fn={}",
            plans.len(),
            hoist_at,
            proto.name.as_deref().unwrap_or("<module>")
        );
    }
    let mut vmaps: Vec<VersionClones> = Vec::new();
    for pl in &plans {
        let body = cfg::loop_body(&preds, &rpo_pos, &reached, pl.header);
        let mut fast = std::collections::BTreeMap::new();
        let mut slow = std::collections::BTreeMap::new();
        for s in &body {
            for (map, _) in [(&mut fast, true), (&mut slow, false)] {
                let cb = b.create_block();
                for &p in &ssa.blocks[*s].params {
                    let ty = clif_ty(ssa.value_ty(p)).ok_or("from_ssa: unsupported block param")?;
                    b.append_block_param(cb, ty);
                }
                map.insert(*s, cb);
            }
        }
        let mut fast_view = blocks.clone();
        let mut slow_view = blocks.clone();
        for (s, c) in &fast {
            fast_view[*s] = Some(*c);
        }
        for (s, c) in &slow {
            slow_view[*s] = Some(*c);
        }
        vmaps.push(VersionClones {
            fast,
            slow,
            fast_view,
            slow_view,
        });
    }
    let mut emitted = vec![false; ssa.blocks.len()];

    for (n, i) in rpo.iter().enumerate() {
        let i = *i;
        if emitted[i] {
            continue;
        }
        if let Some(&pi) = plan_by_pre.get(&i) {
            let cb = blocks[i].expect("block created");
            let is_call_entry = Some(i) == call_entry;
            lower_block(
                &mut b,
                &ctx,
                &mut values,
                &blocks,
                ssa,
                &preds,
                &rpo_pos,
                &shape,
                &global_src,
                &pins,
                &mut hoisted,
                &hoist_at,
                &loop_bodies,
                &landing_live,
                i,
                cb,
                is_call_entry,
                true,
                false,
                false,
                true,
                n,
            )?;
            emitted[i] = true;
            let gobjs: Vec<u32> = plans[pi].guards.iter().map(|(o, _)| *o).collect();
            if !gobjs.iter().all(|o| values[*o as usize].is_some()) {
                if super::super::trace() {
                    eprintln!("clif: versions skip {}", plans[pi].header);
                }
                let blk = &ssa.blocks[i];
                let polls = |target: u32| {
                    rpo_pos[target as usize] <= rpo_pos[i]
                        && cfg::loop_may_collect(ssa, &preds, target as usize, i)
                };
                term::emit_term(&mut b, &ctx, &blocks, &values, &blk.term, polls)?;
                continue;
            }
            arrays::prefill(&mut b, &ctx, &values, &gobjs)?;
            emit_version_guard(&mut b, &ctx, &mut values, ssa, &plans[pi], &vmaps[pi])?;
            let fast_order: Vec<(usize, Block)> = vmaps[pi]
                .fast
                .iter()
                .filter(|(s, _)| rpo.contains(s))
                .map(|(s, c)| (*s, *c))
                .collect();
            let mut fast_values = values.clone();
            for (ssa_b, fast_blk) in fast_order {
                lower_block(
                    &mut b,
                    &ctx,
                    &mut fast_values,
                    &vmaps[pi].fast_view,
                    ssa,
                    &preds,
                    &rpo_pos,
                    &shape,
                    &global_src,
                    &pins,
                    &mut hoisted,
                    &hoist_at,
                    &loop_bodies,
                    &landing_live,
                    ssa_b,
                    fast_blk,
                    false,
                    false,
                    true,
                    true,
                    false,
                    n,
                )?;
                emitted[ssa_b] = true;
            }
            let slow_order: Vec<(usize, Block)> = vmaps[pi]
                .slow
                .iter()
                .filter(|(s, _)| rpo.contains(s))
                .map(|(s, c)| (*s, *c))
                .collect();
            let mut slow_values = values.clone();
            for (ssa_b, slow_blk) in slow_order {
                lower_block(
                    &mut b,
                    &ctx,
                    &mut slow_values,
                    &vmaps[pi].slow_view,
                    ssa,
                    &preds,
                    &rpo_pos,
                    &shape,
                    &global_src,
                    &pins,
                    &mut hoisted,
                    &hoist_at,
                    &loop_bodies,
                    &landing_live,
                    ssa_b,
                    slow_blk,
                    false,
                    false,
                    false,
                    true,
                    false,
                    n,
                )?;
                emitted[ssa_b] = true;
            }
            continue;
        }
        let cb = blocks[i].expect("block created");
        let is_call_entry = Some(i) == call_entry;
        lower_block(
            &mut b,
            &ctx,
            &mut values,
            &blocks,
            ssa,
            &preds,
            &rpo_pos,
            &shape,
            &global_src,
            &pins,
            &mut hoisted,
            &hoist_at,
            &loop_bodies,
            &landing_live,
            i,
            cb,
            is_call_entry,
            true,
            false,
            false,
            false,
            n,
        )?;
        emitted[i] = true;
    }

    for (i, cb) in blocks.iter().enumerate() {
        if !reached[i] {
            b.switch_to_block(cb.expect("block created"));
            b.ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).expect("non-zero trap code"));
        }
    }

    if ctx.frame.is_some() {
        emit_pure_err_ret(&mut b, &ctx)?;
    }

    b.seal_all_blocks();
    b.finalize(isa.frontend_config());
    super::super::debug::capture_ir(&mut debug, &func);
    super::super::debug::capture_kinds_ssa(&mut debug, ssa);
    Ok(Lowered {
        piece: compile_piece(func, isa)?,
        frameless,
    })
}

fn emit_pure_err_ret(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let st = ctx.pure_try.borrow();
    let Some(err_ret) = st.err_ret else {
        return Ok(());
    };
    drop(st);
    b.switch_to_block(err_ret);
    match ctx.activation {
        Activation::Framed => match ctx.proto.return_kind {
            SlotKind::Int | SlotKind::Bool => {
                let z = b.ins().iconst(types::I64, 0);
                b.ins().return_(&[z]);
            }
            SlotKind::Float => {
                let z = b.ins().f64const(0.0);
                b.ins().return_(&[z]);
            }
            SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => {
                b.ins().return_(&[]);
            }
        },
        Activation::Native => {
            let shape = NativeShape::of_proto(ctx.proto);
            match shape.ret {
                NativeClass::Word => {
                    let z = b.ins().iconst(types::I64, 0);
                    b.ins().return_(&[z]);
                }
                NativeClass::Float => {
                    let z = b.ins().f64const(0.0);
                    b.ins().return_(&[z]);
                }
                NativeClass::Boxed => {
                    let tag = b
                        .ins()
                        .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
                    let pay = b.ins().iconst(types::I64, 0);
                    b.ins().return_(&[tag, pay]);
                }
            }
        }
    }
    Ok(())
}

fn emit_version_guard(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ssa: &SsaProto,
    plan: &ranges::VersionPlan,
    vm: &VersionClones,
) -> Result<(), String> {
    use cranelift_codegen::ir::condcodes::IntCC;
    let mut ok = b.ins().iconst(types::I64, 1);
    for (object, need) in &plan.guards {
        let view = ctx
            .views
            .of(*object)
            .ok_or("from_ssa: version guard without array view")?;
        let len_v = b.use_var(view.len);
        let need_v = b.ins().iconst(types::I64, *need as i64);
        let c = b
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, len_v, need_v);
        let c = b.ins().uextend(types::I64, c);
        ok = b.ins().band(ok, c);
    }
    let args: Vec<BlockArg> = match &ssa.blocks[plan.preheader].term {
        SsaTerm::Jump { args, .. } => args
            .iter()
            .map(|v| store::load_value(b, ctx, values, *v).map(BlockArg::from))
            .collect::<Result<_, _>>()?,
        _ => return Err("from_ssa: version preheader without jump".into()),
    };
    let fh = vm.fast.get(&plan.header).copied().expect("fast header");
    let sh = vm.slow.get(&plan.header).copied().expect("slow header");
    b.ins().brif(ok, fh, &args, sh, &args);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_block(
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
    landing_live: &std::collections::BTreeMap<usize, Vec<u32>>,
    ssa_idx: usize,
    clif_blk: Block,
    is_call_entry: bool,
    do_prefill: bool,
    elide_flag: bool,
    is_clone: bool,
    skip_term: bool,
    rpo_n: usize,
) -> Result<(), String> {
    let blk = &ssa.blocks[ssa_idx];
    super::exceptions::set_block_stack(ctx, ssa_idx);
    let saved = if landing_live.contains_key(&ssa_idx) {
        Some(values.to_vec())
    } else {
        None
    };
    if !(rpo_n == 0 && is_call_entry) {
        b.switch_to_block(clif_blk);
    }

    if let Some(live) = landing_live.get(&ssa_idx) {
        if ctx.frame.is_some() {
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
    }

    drop_home_addrs(ctx);

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
            if let (Some(d), Some(&src)) = (inst.dest, fwd.get(&k)) {
                if ctx.ssa.value_ty(d) == ctx.ssa.value_ty(src) {
                    let v = store::load_value(b, ctx, values, src)?;
                    store::define(b, ctx, values, d, v);
                    continue;
                }
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
                land(b, ctx, values, Some(d), store::Out::Boxed(v))?;
                continue;
            }
        }
        match scalar::emit_inst(b, ctx, values, &inst.op, inst.dest)? {
            Some(out) => land(b, ctx, values, inst.dest, out)?,
            None if inst.dest.is_some() => {
                return Err(format!("from_ssa: {:?} defines no value", inst.op));
            }
            None => {}
        }
        if let Some(d) = inst.dest {
            if ctx.try_live.contains(&d) {
                debug_assert!(ctx.frame.is_some());
                let boxed = heap::boxed_value(b, ctx, values, d)?;
                store::home_store(b, ctx, ctx.ssa.reg(d), boxed)?;
            }
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
            _ => {
                if !views::keeps_views(ssa, &inst.op) {
                    fwd.clear();
                    ctx.views.clear_instances(b);
                    hoisted.clear();
                }
            }
        }
        if !views::keeps_views(ssa, &inst.op) {
            ctx.views.clear(b);
        }
        if may_push_frame(ctx, &inst.op) {
            drop_home_addrs(ctx);
        }
    }
    ctx.elide.set(old_elide);

    if !is_clone {
        if let Some(hh) = hoist_at.get(&ssa_idx) {
            if ctx.frame.is_some() {
                for (h, slot) in hh {
                    if hoisted.contains_key(&(*h, *slot)) {
                        continue;
                    }
                    let v = globals::emit_load(b, ctx, *slot, globals::Region::Module)?;
                    hoisted.insert((*h, *slot), v);
                }
            }
        }
    }

    let polls = |target: u32| {
        rpo_pos[target as usize] <= rpo_pos[ssa_idx]
            && cfg::loop_may_collect(ssa, preds, target as usize, ssa_idx)
    };
    if do_prefill {
        if let Some(objects) = pins.get(&ssa_idx) {
            arrays::prefill(b, ctx, values, objects)?;
        }
    }
    if !skip_term {
        term::emit_term(b, ctx, view, values, &blk.term, polls)?;
    }
    if let Some(saved) = saved {
        values.copy_from_slice(&saved);
    }
    Ok(())
}

struct VersionClones {
    fast: std::collections::BTreeMap<usize, Block>,
    slow: std::collections::BTreeMap<usize, Block>,
    fast_view: Vec<Option<Block>>,
    slow_view: Vec<Option<Block>>,
}

fn try_live_set(ssa: &SsaProto) -> rustc_hash::FxHashSet<u32> {
    let mut set = rustc_hash::FxHashSet::default();
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let varn_types::ssa::SsaOp::Try { live, .. } = &inst.op {
                set.extend(live.iter().copied());
            }
        }
    }
    set
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
