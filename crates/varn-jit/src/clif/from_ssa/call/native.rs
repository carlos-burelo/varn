use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::{box_bool, box_int};
use super::super::{heap, Ctx};
use super::invoke::boxed_window;

pub(crate) fn emit_call_native_op(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    args: &[u32],
    op_id: u64,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: native call without a frame")?;
    if args.is_empty() && op_id == varn_core::op_id::int_to_string_op_id() {
        return heap::emit_unary_boxed(b, ctx, values, object, ctx.helpers.to_string);
    }
    let receiver = heap::boxed_value(b, ctx, values, object)?;
    if args.len() == 1 && varn_core::op_id::is_str_char_index_op_id(op_id) {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (pos_tag, pos_payload) = heap::boxed_parts(b, ctx, values, args[0])?;
        let code = crate::clif::from_ssa::exceptions::call_checked_value(
            b,
            ctx,
            ctx.helpers.str_char_code_at,
            &[frame.exec_ctx, recv_tag, recv_payload, pos_tag, pos_payload],
        )?;
        return Ok(box_int(b, code));
    }
    if args.len() == 1 && op_id == varn_core::op_id::str_index_op_id() {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (pos_tag, pos_payload) = heap::boxed_parts(b, ctx, values, args[0])?;
        crate::clif::from_ssa::exceptions::call_checked(
            b,
            ctx,
            ctx.helpers.str_char_at,
            &[frame.exec_ctx, recv_tag, recv_payload, pos_tag, pos_payload],
        )?;
        return Ok(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        ));
    }
    if args.len() == 1 {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (arg_tag, arg_payload) = heap::boxed_parts(b, ctx, values, args[0])?;
        if op_id == varn_core::op_id::str_starts_with_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_starts_with,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(box_bool(b, r));
        }
        if op_id == varn_core::op_id::str_ends_with_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_ends_with,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(box_bool(b, r));
        }
        if op_id == varn_core::op_id::str_index_of_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_index_of,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(box_int(b, r));
        }
        if op_id == varn_core::op_id::str_includes_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_includes,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(box_bool(b, r));
        }
    }
    if op_id == varn_core::op_id::str_split_op_id() && args.len() <= 1 {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let argc_v = b.ins().iconst(types::I64, args.len() as i64);
        let (sep_tag, sep_payload) = if args.len() == 1 {
            heap::boxed_parts(b, ctx, values, args[0])?
        } else {
            (b.ins().iconst(types::I64, 0), b.ins().iconst(types::I64, 0))
        };
        crate::clif::from_ssa::exceptions::call_checked(
            b,
            ctx,
            ctx.helpers.str_split,
            &[
                frame.exec_ctx,
                recv_tag,
                recv_payload,
                argc_v,
                sep_tag,
                sep_payload,
            ],
        )?;
        return Ok(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        ));
    }
    if op_id == varn_core::op_id::str_substring_op_id() && (args.len() == 1 || args.len() == 2) {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (start_tag, start_payload) = heap::boxed_parts(b, ctx, values, args[0])?;
        let has_end_v = b.ins().iconst(types::I64, (args.len() - 1) as i64);
        let (end_tag, end_payload) = if args.len() == 2 {
            heap::boxed_parts(b, ctx, values, args[1])?
        } else {
            (b.ins().iconst(types::I64, 0), b.ins().iconst(types::I64, 0))
        };
        crate::clif::from_ssa::exceptions::call_checked(
            b,
            ctx,
            ctx.helpers.str_substring,
            &[
                frame.exec_ctx,
                recv_tag,
                recv_payload,
                start_tag,
                start_payload,
                has_end_v,
                end_tag,
                end_payload,
            ],
        )?;
        return Ok(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        ));
    }
    if op_id == varn_core::op_id::str_slice_op_id() && (args.len() == 1 || args.len() == 2) {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (start_tag, start_payload) = heap::boxed_parts(b, ctx, values, args[0])?;
        let has_end_v = b.ins().iconst(types::I64, (args.len() - 1) as i64);
        let (end_tag, end_payload) = if args.len() == 2 {
            heap::boxed_parts(b, ctx, values, args[1])?
        } else {
            (b.ins().iconst(types::I64, 0), b.ins().iconst(types::I64, 0))
        };
        crate::clif::from_ssa::exceptions::call_checked(
            b,
            ctx,
            ctx.helpers.str_slice_range,
            &[
                frame.exec_ctx,
                recv_tag,
                recv_payload,
                start_tag,
                start_payload,
                has_end_v,
                end_tag,
                end_payload,
            ],
        )?;
        return Ok(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        ));
    }
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let slot = {
        let mut natives = ctx.natives.borrow_mut();
        natives.push(op_id);
        natives.len() - 1
    };
    let fn_v = crate::clif::reloc::abs_value(b, crate::clif::reloc::AbsSym::Native(slot as u32));
    let op_v = b.ins().iconst(types::I64, op_id as i64);
    let total = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.jit_call_native_window,
        &[frame.exec_ctx, fn_v, op_v, window, total],
    )?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
