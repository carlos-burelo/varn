//! Terminators of the SSA lowering: returns, jumps and branches, and the
//! parallel argument windows that fill each target block's phi params.

use cranelift_codegen::ir::{types, BlockArg, InstBuilder, MemFlags, TrapCode, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::SsaTerm;

use super::{block_of, heap, load_value, resolve_args, Ctx};

fn scalar_return(k: SlotKind) -> bool {
    matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool)
}

/// Write a non-scalar return to `jit_native_result` and return void — the
/// convention the wrapper reads for a `Dynamic`/heap return class.
fn store_boxed_return(b: &mut FunctionBuilder, ctx: &Ctx<'_>, boxed: Value) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: non-scalar return without a frame")?;
    b.ins().store(
        MemFlags::trusted(),
        boxed,
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    b.ins().return_(&[]);
    Ok(())
}

/// Emit `term`. `back_edge(target)` tells whether jumping to `target` closes
/// a loop: before such a jump a frame-aware body polls the collector, since
/// an allocating loop has no other point where one could run. Its heap
/// values live in their homes, which the collector sees and rewrites, and
/// its scalars cannot move, so nothing is flushed or reloaded around it.
pub(super) fn emit_term(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    blocks: &[Option<cranelift_codegen::ir::Block>],
    values: &[Option<Value>],
    term: &SsaTerm,
    back_edge: impl Fn(u32) -> bool,
) -> Result<(), String> {
    let loops = match term {
        SsaTerm::Jump { target, .. } => back_edge(*target),
        SsaTerm::Branch {
            then_blk, else_blk, ..
        } => back_edge(*then_blk) || back_edge(*else_blk),
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
            });
        }
    }
    match term {
        SsaTerm::Return(Some(v)) => {
            let ret = ctx.proto.return_kind;
            if scalar_return(ret) {
                // The value is converted from its class to the function's
                // return class: a boxed value (a `dynamic` sum returned as
                // `int`) is unboxed; a scalar must already be of that class.
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
            let c = load_value(b, ctx, values, *cond)?;
            let t: Vec<BlockArg> = resolve_args(b, ctx, values, then_args)?;
            let e: Vec<BlockArg> = resolve_args(b, ctx, values, else_args)?;
            let tb = block_of(blocks, *then_blk)?;
            let eb = block_of(blocks, *else_blk)?;
            b.ins().brif(c, tb, &t, eb, &e);
        }
        SsaTerm::Unreachable => {
            b.ins().trap(TrapCode::user(1).unwrap());
        }
        // Throw needs a convention this ABI does not carry; decline.
        SsaTerm::Throw(_) => return Err("from_ssa: unhandled terminator".into()),
    }
    Ok(())
}
