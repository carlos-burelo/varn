use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, BlockArg, InstBuilder, TrapCode, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::SsaTerm;

use super::{heap, load_value, Ctx};

fn scalar_return(k: SlotKind) -> bool {
    matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool)
}

fn store_boxed_return(b: &mut FunctionBuilder, ctx: &Ctx<'_>, boxed: Value) -> Result<(), String> {
    match ctx.activation {
        super::Activation::Native => {
            let (tag, payload) = b.ins().isplit(boxed);
            b.ins().return_(&[tag, payload]);
        }
        super::Activation::Framed => {
            b.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                boxed,
                ctx.exec_ctx,
                ctx.helpers.jit_native_result_offset as i32,
            );
            b.ins().return_(&[]);
        }
    }
    Ok(())
}

fn truthy(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    let x = load_value(b, ctx, values, v)?;
    Ok(match ctx.ssa.value_ty(v) {
        SlotKind::Bool => x,
        SlotKind::Int => b.ins().icmp_imm_u(IntCC::NotEqual, x, 0),
        SlotKind::Float => {
            let zero = b.ins().f64const(0.0);
            b.ins().fcmp(FloatCC::OrderedNotEqual, x, zero)
        }
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => {
            let (tag, payload) = b.ins().isplit(x);
            super::super::emit::call_helper(b, ctx.cc, ctx.helpers.truthy, &[tag, payload])
        }
    })
}

pub(super) fn emit_term(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    blocks: &[Option<cranelift_codegen::ir::Block>],
    values: &[Option<Value>],
    term: &SsaTerm,
    polls: impl Fn(u32) -> bool,
) -> Result<(), String> {
    let loops = match term {
        SsaTerm::Jump { target, .. } => polls(*target),
        SsaTerm::Branch {
            then_blk, else_blk, ..
        } => polls(*then_blk) || polls(*else_blk),
        SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => false,
    };
    if loops {
        if let Some(frame) = &ctx.frame {
            let exec_ctx = frame.exec_ctx;
            super::super::alloc::emit_gc_poll(b, ctx.helpers, exec_ctx, |b| {
                super::super::emit::call_helper_void(
                    b,
                    ctx.cc,
                    ctx.helpers.gc_safepoint,
                    &[exec_ctx],
                );

                ctx.views.clear(b);
            });
        }
    }
    match term {
        SsaTerm::Return(Some(v)) => {
            let ret = ctx.proto.return_kind;
            if scalar_return(ret) {
                let kind = ctx.ssa.value_ty(*v);
                let x = if super::is_heap(kind) {
                    let boxed = load_value(b, ctx, values, *v)?;
                    heap::unbox_dest(b, ret, boxed)?
                } else if kind == ret {
                    load_value(b, ctx, values, *v)?
                } else {
                    return Err(format!("from_ssa: {kind:?} value returned as {ret:?}"));
                };
                b.ins().return_(&[x]);
            } else {
                let boxed = heap::boxed_value(b, ctx, values, *v)?;
                store_boxed_return(b, ctx, boxed)?;
            }
        }
        SsaTerm::Return(None) => {
            if scalar_return(ctx.proto.return_kind) {
                return Err("from_ssa: value-less return of a scalar".into());
            }
            let tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
            let payload = b.ins().iconst(types::I64, 0);
            let null = b.ins().iconcat(tag, payload);
            store_boxed_return(b, ctx, null)?;
        }
        SsaTerm::Jump { target, args } => {
            let a: Vec<BlockArg> = resolve_args(b, ctx, values, args)?;
            let t = block_of(blocks, *target)?;
            b.ins().jump(t, &a);
        }
        SsaTerm::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => {
            let c = truthy(b, ctx, values, *cond)?;
            let t: Vec<BlockArg> = resolve_args(b, ctx, values, then_args)?;
            let e: Vec<BlockArg> = resolve_args(b, ctx, values, else_args)?;
            let tb = block_of(blocks, *then_blk)?;
            let eb = block_of(blocks, *else_blk)?;
            b.ins().brif(c, tb, &t, eb, &e);
        }
        SsaTerm::Unreachable => {
            b.ins().trap(TrapCode::user(1).unwrap());
        }
        SsaTerm::Throw(v) => super::exceptions::emit_throw(b, ctx, values, *v)?,
    }
    Ok(())
}

fn resolve_args(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    args: &[u32],
) -> Result<Vec<cranelift_codegen::ir::BlockArg>, String> {
    args.iter()
        .map(|v| load_value(b, ctx, values, *v).map(cranelift_codegen::ir::BlockArg::from))
        .collect()
}

fn block_of(
    blocks: &[Option<cranelift_codegen::ir::Block>],
    id: u32,
) -> Result<cranelift_codegen::ir::Block, String> {
    blocks
        .get(id as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("from_ssa: block {id} not created"))
}
