use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlagsData, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::context::{Ctx, SelfAbi};
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
    guard_limit(b, ctx, exhausted, helpers.stack_overflow);
}

pub(super) fn enter_frameless(b: &mut FunctionBuilder, ctx: &Ctx<'_>) {
    let lay = &ctx.helpers.call_layout;
    let m = MemFlagsData::trusted();
    let frames = b
        .ins()
        .load(types::I64, m, ctx.exec_ctx, lay.frames_len_off as i32);
    let frameless = b
        .ins()
        .load(types::I64, m, ctx.exec_ctx, lay.frameless_depth_off as i32);
    let depth = b.ins().iadd(frames, frameless);
    let exhausted = b.ins().icmp_imm_u(
        IntCC::UnsignedGreaterThanOrEqual,
        depth,
        lay.max_call_depth as i64,
    );
    guard_limit(b, ctx, exhausted, ctx.helpers.call_depth_overflow);
    let next = b.ins().iadd_imm_u(frameless, 1);
    b.ins()
        .store(m, next, ctx.exec_ctx, lay.frameless_depth_off as i32);
}

pub(super) fn leave_frameless(b: &mut FunctionBuilder, ctx: &Ctx<'_>) {
    let off = ctx.helpers.call_layout.frameless_depth_off as i32;
    let m = MemFlagsData::trusted();
    let depth = b.ins().load(types::I64, m, ctx.exec_ctx, off);
    let next = b.ins().iadd_imm_s(depth, -1);
    b.ins().store(m, next, ctx.exec_ctx, off);
}

fn guard_limit(b: &mut FunctionBuilder, ctx: &Ctx<'_>, exhausted: Value, helper: usize) {
    let raise = b.create_block();
    let cont = b.create_block();
    b.set_cold_block(raise);
    b.ins().brif(exhausted, raise, &[], cont, &[]);

    b.switch_to_block(raise);
    raise_overflow(b, ctx, helper);

    b.switch_to_block(cont);
}

fn raise_overflow(b: &mut FunctionBuilder, ctx: &Ctx<'_>, helper: usize) {
    let helpers = ctx.helpers;
    let m = MemFlagsData::trusted();
    super::exceptions::note_site(b, ctx);
    let zero = b.ins().iconst(types::I64, 0);
    let err_off = helpers.jit_error_flag_offset as i32;
    b.ins().store(m, zero, ctx.exec_ctx, err_off);
    call_helper_void(b, ctx.cc, helper, &[ctx.exec_ctx, zero, zero, zero, zero]);
    let (landing, err_ret) = ctx.exits(b);
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
}

pub(super) fn guard_self_call(b: &mut FunctionBuilder, ctx: &Ctx<'_>, abi: &SelfAbi) -> Value {
    let helpers = ctx.helpers;
    let sp = b.ins().get_stack_pointer(types::I64);
    let floor = b.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        ctx.exec_ctx,
        helpers.jit_stack_floor_offset as i32,
    );
    let stack_low = b.ins().icmp(IntCC::UnsignedLessThan, sp, floor);
    let too_deep = b.ins().icmp_imm_u(
        IntCC::UnsignedGreaterThanOrEqual,
        abi.depth,
        helpers.call_layout.max_call_depth as i64,
    );
    let raise_stack = b.create_block();
    let raise_depth = b.create_block();
    let check_depth = b.create_block();
    let cont = b.create_block();
    b.set_cold_block(raise_stack);
    b.set_cold_block(raise_depth);
    b.ins().brif(stack_low, raise_stack, &[], check_depth, &[]);

    b.switch_to_block(check_depth);
    b.ins().brif(too_deep, raise_depth, &[], cont, &[]);

    b.switch_to_block(raise_stack);
    raise_overflow(b, ctx, helpers.stack_overflow);
    b.switch_to_block(raise_depth);
    raise_overflow(b, ctx, helpers.call_depth_overflow);

    b.switch_to_block(cont);
    b.ins().iadd_imm_u(abi.depth, 1)
}

pub(crate) fn publish(b: &mut FunctionBuilder, helpers: &JitHelpers, exec_ctx: Value) {
    let sp = b.ins().get_stack_pointer(types::I64);
    let fp = b.ins().get_frame_pointer(types::I64);
    let off = helpers.jit_exit_offset as i32;
    b.ins().store(MemFlagsData::trusted(), sp, exec_ctx, off);
    b.ins()
        .store(MemFlagsData::trusted(), fp, exec_ctx, off + 8);
}
