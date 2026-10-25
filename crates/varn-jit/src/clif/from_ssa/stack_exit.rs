use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlagsData, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::context::Ctx;
use crate::JitHelpers;

pub(super) fn guard_headroom(b: &mut FunctionBuilder, ctx: &Ctx<'_>) {
    let helpers = ctx.helpers;
    let m = MemFlagsData::trusted();
    let sp = b.ins().get_stack_pointer(types::I64);
    let floor = b.ins().load(
        types::I64,
        m,
        ctx.exec_ctx,
        helpers.jit_stack_floor_offset as i32,
    );
    let exhausted = b.ins().icmp(IntCC::UnsignedLessThan, sp, floor);
    let raise = b.create_block();
    let cont = b.create_block();
    b.set_cold_block(raise);
    b.ins().brif(exhausted, raise, &[], cont, &[]);

    b.switch_to_block(raise);
    let zero = b.ins().iconst(types::I64, 0);
    let err_off = helpers.jit_error_flag_offset as i32;
    b.ins().store(m, zero, ctx.exec_ctx, err_off);
    call_helper_void(
        b,
        ctx.cc,
        helpers.stack_overflow,
        &[ctx.exec_ctx, zero, zero, zero, zero],
    );
    let (landing, err_ret) = {
        let st = ctx.pure_try.borrow();
        let landing = st.stack.last().and_then(|h| st.clif_map.get(h).copied());
        (landing, st.err_ret)
    };
    match (landing, err_ret) {
        (Some(handler), _) => {
            let thrown = b.ins().load(
                types::I128,
                m,
                ctx.exec_ctx,
                helpers.jit_native_result_offset as i32,
            );
            b.declare_value_needs_stack_map(thrown);
            b.ins().store(m, zero, ctx.exec_ctx, err_off);
            b.ins().jump(handler, &[thrown.into()]);
        }
        (None, Some(error_return)) => {
            b.ins().jump(error_return, &[]);
        }
        (None, None) => super::exceptions::emit_placeholder_return(b, ctx),
    }

    b.switch_to_block(cont);
}

pub(crate) fn publish(b: &mut FunctionBuilder, helpers: &JitHelpers, exec_ctx: Value) {
    let sp = b.ins().get_stack_pointer(types::I64);
    let fp = b.ins().get_frame_pointer(types::I64);
    let off = helpers.jit_exit_offset as i32;
    b.ins().store(MemFlagsData::trusted(), sp, exec_ctx, off);
    b.ins()
        .store(MemFlagsData::trusted(), fp, exec_ctx, off + 8);
}
