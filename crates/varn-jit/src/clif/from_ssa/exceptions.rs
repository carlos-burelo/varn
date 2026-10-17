use super::super::abi::Activation;
use super::super::native_abi::{NativeClass, NativeShape};
use super::{heap, home_store, Ctx, FrameIo};
use cranelift_codegen::ir::{types, Block, InstBuilder, MemFlagsData, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

fn frame<'a>(ctx: &'a Ctx<'_>) -> Result<&'a FrameIo, String> {
    ctx.frame
        .as_ref()
        .ok_or_else(|| "from_ssa: try/throw without a frame".into())
}

fn current_landing(ctx: &Ctx<'_>) -> Option<(usize, Block)> {
    let st = ctx.pure_try.borrow();
    let h = *st.stack.last()?;
    let blk = *st.clif_map.get(&h)?;
    Some((h, blk))
}

fn err_ret(ctx: &Ctx<'_>) -> Option<Block> {
    ctx.pure_try.borrow().err_ret
}

pub(super) fn set_block_stack(ctx: &Ctx<'_>, ssa_idx: usize) {
    let entry = ctx
        .pure_try
        .borrow()
        .entry_stacks
        .get(ssa_idx)
        .cloned()
        .unwrap_or_default();
    ctx.pure_try.borrow_mut().stack = entry;
}

pub(super) fn emit_try(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    catch_ip: u32,
    catch_value: u32,
    live: &[u32],
) -> Result<(), String> {
    let fr = frame(ctx)?;
    let _ = fr;
    for &v in live {
        let boxed = heap::boxed_value(b, ctx, values, v)?;
        home_store(b, ctx, ctx.ssa.reg(v), boxed)?;
    }
    let e = ctx
        .ssa
        .catch_at(catch_ip as usize)
        .ok_or_else(|| format!("from_ssa: no catch entry at ip {catch_ip}"))?;
    let h = e.block as usize;
    {
        let mut st = ctx.pure_try.borrow_mut();
        if let Some(prev) = st.landing_error.get(&h) {
            if *prev != catch_value {
                return Err("from_ssa: handler shared by two try values".into());
            }
        } else {
            st.landing_error.insert(h, catch_value);
        }
        st.stack.push(h);
    }
    Ok(())
}

pub(super) fn emit_pop_try(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let _ = b;
    let mut st = ctx.pure_try.borrow_mut();
    st.stack
        .pop()
        .ok_or_else(|| "from_ssa: poptry without try".to_owned())?;
    Ok(())
}

pub(super) fn emit_throw(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    value: u32,
) -> Result<(), String> {
    let boxed = heap::boxed_value(b, ctx, values, value)?;
    emit_throw_boxed(b, ctx, boxed)
}

pub(super) fn emit_throw_boxed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    boxed: Value,
) -> Result<(), String> {
    if let Some((_, landing)) = current_landing(ctx) {
        b.ins().jump(landing, &[boxed.into()]);
        Ok(())
    } else {
        let fr = frame(ctx)?;
        let ectx = fr.exec_ctx;
        b.ins().store(
            MemFlagsData::trusted(),
            boxed,
            ectx,
            ctx.helpers.jit_native_result_offset as i32,
        );
        let one = b.ins().iconst(types::I64, 1);
        b.ins().store(
            MemFlagsData::trusted(),
            one,
            ectx,
            ctx.helpers.jit_error_flag_offset as i32,
        );
        let err =
            err_ret(ctx).ok_or_else(|| "from_ssa: throw without handler nor err_ret".to_owned())?;
        b.ins().jump(err, &[]);
        Ok(())
    }
}

pub(super) fn emit_catch_param(values: &[Option<Value>], try_val: u32) -> Result<Value, String> {
    values
        .get(try_val as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("from_ssa: catch value {try_val} not defined at landing"))
}

pub(super) fn clear_error_flag(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let Some(fr) = ctx.frame.as_ref() else {
        return Ok(());
    };
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().store(
        MemFlagsData::trusted(),
        zero,
        fr.exec_ctx,
        ctx.helpers.jit_error_flag_offset as i32,
    );
    Ok(())
}

pub(super) fn check_error_branch(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let Some(fr) = ctx.frame.as_ref() else {
        return Ok(());
    };
    let ectx = fr.exec_ctx;
    let flag = b.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_error_flag_offset as i32,
    );
    let (landing, err) = {
        let st = ctx.pure_try.borrow();
        let landing = st.stack.last().and_then(|h| st.clif_map.get(h).copied());
        (landing, st.err_ret)
    };
    let cont = b.create_block();
    match (landing, err) {
        (Some(t), _) => {
            let err_blk = b.create_block();
            b.ins().brif(flag, err_blk, &[], cont, &[]);
            b.switch_to_block(err_blk);
            let boxed = b.ins().load(
                types::I128,
                MemFlagsData::trusted(),
                ectx,
                ctx.helpers.jit_native_result_offset as i32,
            );
            b.declare_value_needs_stack_map(boxed);
            let zero = b.ins().iconst(types::I64, 0);
            b.ins().store(
                MemFlagsData::trusted(),
                zero,
                ectx,
                ctx.helpers.jit_error_flag_offset as i32,
            );
            b.ins().jump(t, &[boxed.into()]);
            b.switch_to_block(cont);
        }
        (None, Some(e)) => {
            b.ins().brif(flag, e, &[], cont, &[]);
            b.switch_to_block(cont);
        }
        (None, None) => {}
    }
    Ok(())
}

pub(super) fn call_checked(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    helper: usize,
    args: &[Value],
) -> Result<(), String> {
    clear_error_flag(b, ctx)?;
    super::super::emit::call_helper_void(b, ctx.cc, helper, args);
    check_error_branch(b, ctx)
}

pub(super) fn call_checked_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    helper: usize,
    args: &[Value],
) -> Result<Value, String> {
    clear_error_flag(b, ctx)?;
    let v = super::super::emit::call_helper(b, ctx.cc, helper, args);
    check_error_branch(b, ctx)?;
    Ok(v)
}

pub(super) fn try_live_set(ssa: &varn_types::ssa::SsaProto) -> rustc_hash::FxHashSet<u32> {
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

pub(super) fn emit_pure_err_ret(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let st = ctx.pure_try.borrow();
    let Some(err_ret) = st.err_ret else {
        return Ok(());
    };
    drop(st);
    b.switch_to_block(err_ret);
    emit_placeholder_return(b, ctx);
    Ok(())
}

pub(super) fn emit_placeholder_return(b: &mut FunctionBuilder, ctx: &Ctx<'_>) {
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
}
