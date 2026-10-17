use std::collections::BTreeMap;

use cranelift_codegen::ir::{types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{OsrRequest, SsaEntryKind, SsaProto, SsaSecondaryEntry};
use varn_types::FunctionProto;

use super::super::abi::Activation;
use super::super::homes::Homes;
use super::super::native_abi::NativeShape;
use super::frame_policy::needs_frame;
use super::store::{clif_ty, is_heap};
use super::views::Views;
use super::FrameIo;
use crate::JitHelpers;

pub(super) struct EntryFlags {
    pub frameless: bool,
    pub frame_aware: bool,
    pub abi_params: usize,
}

pub(super) fn prepare<'s>(
    proto: &FunctionProto,
    ssa: &'s SsaProto,
    osr_req: Option<OsrRequest>,
    activation: Activation,
) -> Result<(Option<&'s SsaSecondaryEntry>, EntryFlags), String> {
    let osr = match osr_req {
        None => None,
        Some(req) => Some(
            ssa.secondary_at(req.ip)
                .ok_or_else(|| format!("from_ssa: no secondary entry at ip {}", req.ip))?,
        ),
    };
    if osr.is_some() && activation == Activation::Native {
        return Err(super::NEEDS_ACTIVATION.into());
    }
    let nparams = proto.arity.saturating_sub(1);
    super::cfg::check_block_args(ssa)?;
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
                || matches!(blk.term, varn_types::ssa::SsaTerm::Throw(_))
        }));
    let frame_aware = activation == Activation::Framed || !frameless;
    let abi_params = if osr.is_some() { 0 } else { nparams };
    Ok((
        osr,
        EntryFlags {
            frameless,
            frame_aware,
            abi_params,
        },
    ))
}

pub(super) type LandingLive = BTreeMap<usize, Vec<u32>>;
pub(super) type LandingError = BTreeMap<usize, u32>;

pub(super) fn landing_maps(ssa: &SsaProto) -> Result<(LandingLive, LandingError), String> {
    let mut landing_live: LandingLive = Default::default();
    let mut landing_error: LandingError = Default::default();
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
                && let Some(e) = ssa.catch_at(*catch_ip as usize)
            {
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
    Ok((landing_live, landing_error))
}

pub(super) fn create_blocks(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    call_entry: Option<usize>,
    landing_live: &LandingLive,
) -> Result<(Vec<Option<Block>>, Block), String> {
    let mut blocks: Vec<Option<Block>> = vec![None; ssa.blocks.len()];
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
    Ok((blocks, entry_blk))
}

pub(super) struct Reach {
    pub rpo: Vec<usize>,
    pub rpo_pos: Vec<usize>,
    pub reached: Vec<bool>,
    pub preds: Vec<Vec<usize>>,
    pub full_entry_stacks: Vec<Vec<usize>>,
}

pub(super) fn reachability(ssa: &SsaProto, start: usize, with_full: bool) -> Result<Reach, String> {
    let rpo = super::cfg::order(ssa, start);
    let preds = super::cfg::predecessors(ssa);
    let mut rpo_pos = vec![0usize; ssa.blocks.len()];
    let mut reached = vec![false; ssa.blocks.len()];
    for (pos, &blk) in rpo.iter().enumerate() {
        rpo_pos[blk] = pos;
        reached[blk] = true;
    }
    let full_rpo = if with_full {
        super::cfg::order(ssa, ssa.entry as usize)
    } else {
        rpo.clone()
    };
    let mut full_reached = vec![false; ssa.blocks.len()];
    for &blk in &full_rpo {
        full_reached[blk] = true;
    }
    let full_entry_stacks = super::cfg::try_entry_stacks(ssa, &full_rpo, &full_reached)
        .ok_or_else(|| "from_ssa: inconsistent try stacks".to_owned())?;
    Ok(Reach {
        rpo,
        rpo_pos,
        reached,
        preds,
        full_entry_stacks,
    })
}

pub(super) struct EntryState {
    pub views: Views,
    pub exec_ctx: Value,
    pub frame: Option<FrameIo>,
    pub this: Option<Value>,
}

pub(super) fn enter(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    proto: &FunctionProto,
    helpers: &JitHelpers,
    activation: Activation,
    frame_aware: bool,
    abi_params: usize,
    shape: &NativeShape,
    entry_blk: Block,
) -> Result<EntryState, String> {
    let views = Views::declare(b, ssa);

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
            let homes = Homes {
                exec_ctx,
                base: base.expect("a framed body has an activation"),
                layout: &f.layout,
                offsets: &helpers.frame_layout,
            };
            let v = homes.load(b, 0);
            b.declare_value_needs_stack_map(v);
            Some(v)
        }
    };
    Ok(EntryState {
        views,
        exec_ctx,
        frame,
        this,
    })
}
