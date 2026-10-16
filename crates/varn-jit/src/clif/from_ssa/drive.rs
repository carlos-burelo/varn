use super::super::abi::{raw_signature, Activation};
use super::super::native_abi::NativeShape;
use super::super::piece::compile_piece;
use super::{
    block, call, exceptions, hoist, induction, osr, pinned, ranges, setup, stack_exit, versions,
    Ctx, Lowered,
};
use crate::JitHelpers;
use cranelift_codegen::ir::{
    types, Block, ExtFuncData, ExternalName, Function, InstBuilder, UserExternalName, UserFuncName,
    Value,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use varn_types::ssa::{OsrRequest, SsaProto};
use varn_types::{FunctionProto, VmValue};
fn dump_ssa_trace(ssa: &SsaProto) {
    let name = &*ssa.name;
    eprintln!(
        "clif: ssa {name} entry={} nvalues={}",
        ssa.entry,
        ssa.values.len()
    );
    for (i, blk) in ssa.blocks.iter().enumerate() {
        eprintln!(
            "clif: ssa {name} b{i} params={:?} term={:?}",
            blk.params, blk.term
        );
        for inst in &blk.insts {
            let op = format!("{:?}", inst.op);
            let cut = op
                .char_indices()
                .nth(160)
                .map(|(j, _)| &op[..j])
                .unwrap_or(&op);
            eprintln!("clif: ssa {name} b{i} v{:?} = {cut}", inst.dest);
        }
    }
}

pub(in crate::clif) fn try_lower(
    proto: &FunctionProto,
    ssa: &SsaProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    osr_req: Option<OsrRequest>,
    activation: Activation,
    mut debug: Option<&mut super::super::debug::ClifDebugSink>,
) -> Result<Lowered, String> {
    if super::super::trace() {
        dump_ssa_trace(ssa);
    }
    let (osr, flags) = setup::prepare(proto, ssa, osr_req, activation)?;
    let entry = ssa.entry as usize;
    let call_entry = osr.is_none().then_some(entry);
    let start = osr.map_or(entry, |h| h.block as usize);

    let (landing_live, landing_error) = setup::landing_maps(ssa)?;

    let cc = isa.default_call_conv();
    let shape = NativeShape::of_proto(proto);
    let signature = match activation {
        Activation::Native => shape.signature(),
        Activation::Framed => raw_signature(proto, flags.abi_params, cc),
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

    let (blocks, entry_blk) = setup::create_blocks(&mut b, ssa, call_entry, &landing_live)?;

    let mut reach = setup::reachability(ssa, start, osr.is_some())?;
    if osr.is_some() {
        osr::expand_resume(
            ssa,
            &reach.full_entry_stacks,
            start,
            &mut reach.rpo,
            &mut reach.rpo_pos,
            &mut reach.reached,
        );
    }

    b.switch_to_block(entry_blk);
    let est = setup::enter(
        &mut b,
        ssa,
        proto,
        helpers,
        activation,
        flags.frame_aware,
        flags.abi_params,
        &shape,
        entry_blk,
    )?;
    let scratch = call::ScratchWin::create(&mut b, call::scratch_max(ssa));
    let (no_overflow, version_plans) = if osr.is_some() {
        (rustc_hash::FxHashSet::default(), Vec::new())
    } else {
        ranges::analyze(ssa, &reach.preds, &reach.rpo_pos, &reach.reached)
    };
    let entry_stacks = reach.full_entry_stacks;
    let mut clif_map: std::collections::BTreeMap<usize, Block> = Default::default();
    for h in landing_live.keys() {
        if let Some(cb) = blocks[*h] {
            clif_map.insert(*h, cb);
        }
    }
    let err_ret = if flags.frame_aware {
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
        exec_ctx: est.exec_ctx,
        frame: est.frame,
        activation,
        this: est.this,
        has_round: super::super::floats::has_round_support(isa),
        views: est.views,
        in_range_steps: induction::in_range_steps(
            ssa,
            &reach.preds,
            &reach.rpo_pos,
            &reach.reached,
        ),
        no_overflow,
        fast_bounds: rustc_hash::FxHashSet::default(),
        elide: std::cell::Cell::new(false),
        home_addrs: Default::default(),
        natives: Default::default(),
        scratch,
        carried: match osr {
            Some(h) => osr::carried(&mut b, ssa, h, &reach.reached),
            None => osr::versioned(
                &mut b,
                ssa,
                &version_plans,
                &reach.preds,
                &reach.rpo_pos,
                &reach.reached,
            ),
        },
        try_live: exceptions::try_live_set(ssa),
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
    if osr.is_none()
        && let Some(frame) = &ctx.frame
    {
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            zero,
            frame.exec_ctx,
            helpers.jit_error_flag_offset as i32,
        );
    }
    let mut values: Vec<Option<Value>> = vec![None; ssa.values.len()];
    if let Some(h) = osr {
        let header = blocks[h.block as usize].expect("block created");
        osr::emit_entry(&mut b, &ctx, &mut values, h, header)?;
    }
    let pins = match osr {
        Some(_) => std::collections::BTreeMap::new(),
        None => pinned::compute(ssa, &reach.preds, &reach.rpo_pos, &reach.reached),
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

    let global_src = hoist::global_sources(ssa);

    let (loop_headers, loop_bodies) =
        ranges::loop_sets(ssa, &reach.preds, &reach.rpo_pos, &reach.reached);
    let hoist_at = hoist::plan_hoists(
        ssa,
        &reach.preds,
        &reach.reached,
        &loop_headers,
        &loop_bodies,
        ctx.frame.is_some(),
    );
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
    let vmaps = versions::build_clones(
        &mut b,
        ssa,
        &plans,
        &reach.preds,
        &reach.rpo_pos,
        &reach.reached,
        &blocks,
    )?;
    let mut emitted = vec![false; ssa.blocks.len()];

    for (n, i) in reach.rpo.iter().enumerate() {
        let i = *i;
        if emitted[i] {
            continue;
        }
        if let Some(&pi) = plan_by_pre.get(&i) {
            versions::lower_versioned(
                &mut b,
                &ctx,
                &mut values,
                &blocks,
                &mut emitted,
                ssa,
                &reach.preds,
                &reach.rpo,
                &reach.rpo_pos,
                &shape,
                &global_src,
                &pins,
                &mut hoisted,
                &hoist_at,
                &loop_bodies,
                &landing_live,
                &plans,
                &vmaps,
                pi,
                i,
                n,
                entry,
            )?;
            continue;
        }
        let cb = blocks[i].expect("block created");
        let is_call_entry = Some(i) == call_entry;
        block::lower_block(
            &mut b,
            &ctx,
            &mut values,
            &blocks,
            ssa,
            &reach.preds,
            &reach.rpo_pos,
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
            false,
        )
        .map_err(|e| format!("{e} [bloque {i}]"))?;
        emitted[i] = true;
    }

    for (i, cb) in blocks.iter().enumerate() {
        if !reach.reached[i] {
            b.switch_to_block(cb.expect("block created"));
            b.ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).expect("non-zero trap code"));
        }
    }

    if ctx.frame.is_some() {
        exceptions::emit_pure_err_ret(&mut b, &ctx)?;
    }

    b.seal_all_blocks();
    b.finalize(isa.frontend_config());
    super::super::debug::capture_ir(&mut debug, &func);
    super::super::debug::capture_kinds_ssa(&mut debug, ssa);
    let frameless = flags.frameless;
    Ok(Lowered {
        piece: compile_piece(func, isa)?,
        natives: ctx.natives.take(),
        frameless,
    })
}
