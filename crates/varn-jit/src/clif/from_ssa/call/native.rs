use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::super::emit::{box_bool, box_int};
use super::super::{heap, Ctx, Out};
use super::invoke::boxed_window;

pub(crate) fn emit_call_native_op(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    args: &[u32],
    op_id: u64,
) -> Result<Out, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: native call without a frame")?;
    if args.is_empty() && op_id == varn_core::op_id::int_to_string_op_id() {
        let v = heap::emit_unary_boxed(b, ctx, values, object, ctx.helpers.to_string)?;
        return Ok(Out::Boxed(v));
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
        return Ok(Out::Boxed(box_int(b, code)));
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
        return Ok(Out::Boxed(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        )));
    }
    if args.len() == 1 {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (arg_tag, arg_payload) = heap::boxed_parts(b, ctx, values, args[0])?;
        if op_id == varn_core::op_id::str_starts_with_op_id()
            && ctx.ssa.value_ty(object) == SlotKind::Str
            && ctx.ssa.value_ty(args[0]) == SlotKind::Str
        {
            return emit_str_affix(
                b,
                ctx,
                frame.exec_ctx,
                ctx.helpers.str_starts_with,
                recv_tag,
                recv_payload,
                arg_tag,
                arg_payload,
                false,
            );
        }
        if op_id == varn_core::op_id::str_ends_with_op_id()
            && ctx.ssa.value_ty(object) == SlotKind::Str
            && ctx.ssa.value_ty(args[0]) == SlotKind::Str
        {
            return emit_str_affix(
                b,
                ctx,
                frame.exec_ctx,
                ctx.helpers.str_ends_with,
                recv_tag,
                recv_payload,
                arg_tag,
                arg_payload,
                true,
            );
        }
        if op_id == varn_core::op_id::str_starts_with_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_starts_with,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(Out::Boxed(box_bool(b, r)));
        }
        if op_id == varn_core::op_id::str_ends_with_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_ends_with,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(Out::Boxed(box_bool(b, r)));
        }
        if op_id == varn_core::op_id::str_index_of_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_index_of,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(Out::Boxed(box_int(b, r)));
        }
        if op_id == varn_core::op_id::str_includes_op_id() {
            let r = crate::clif::from_ssa::exceptions::call_checked_value(
                b,
                ctx,
                ctx.helpers.str_includes,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            )?;
            return Ok(Out::Boxed(box_bool(b, r)));
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
        return Ok(Out::Boxed(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        )));
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
        return Ok(Out::Boxed(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        )));
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
        return Ok(Out::Boxed(b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        )));
    }
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let target = (ctx.helpers.resolve_native_op)(op_id);
    let fn_v = b.ins().iconst(types::I64, target.func_ptr as i64);
    let op_v = b.ins().iconst(types::I64, op_id as i64);
    let total = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.jit_call_native_window,
        &[frame.exec_ctx, fn_v, op_v, window, total],
    )?;
    Ok(Out::Boxed(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    )))
}

fn emit_str_affix(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    exec_ctx: Value,
    helper: usize,
    recv_tag: Value,
    recv_payload: Value,
    arg_tag: Value,
    arg_payload: Value,
    ends: bool,
) -> Result<Out, String> {
    let sso = varn_types::vm_value::KIND_SSO as i64;
    let fast = b.create_block();
    let slow = b.create_block();
    let cmp = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64);
    b.set_cold_block(slow);
    let ff = b.ins().iconst(types::I64, 0xFF);
    let sso_v = b.ins().iconst(types::I64, sso);
    let s_kind = b.ins().band(recv_tag, ff);
    let s_is = b.ins().icmp(IntCC::Equal, s_kind, sso_v);
    let p_kind = b.ins().band(arg_tag, ff);
    let p_is = b.ins().icmp(IntCC::Equal, p_kind, sso_v);
    let both = b.ins().band(s_is, p_is);
    b.ins().brif(both, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let eight = b.ins().iconst(types::I64, 8);
    let sby8 = b.ins().ushr(recv_tag, eight);
    let slen = b.ins().band(sby8, ff);
    let pby8 = b.ins().ushr(arg_tag, eight);
    let plen = b.ins().band(pby8, ff);
    let len_ok = b.ins().icmp(IntCC::UnsignedLessThanOrEqual, plen, slen);
    let no = b.create_block();
    b.ins().brif(len_ok, cmp, &[], no, &[]);
    b.switch_to_block(no);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().jump(merge, &[zero.into()]);

    b.switch_to_block(cmp);
    let three = b.ins().iconst(types::I64, 3);
    let sub = b.ins().isub(slen, plen);
    let shift = if ends {
        b.ins().ishl(sub, three)
    } else {
        b.ins().iconst(types::I64, 0)
    };
    let adj = b.ins().ushr(recv_payload, shift);
    let one = b.ins().iconst(types::I64, 1);
    let mul8 = b.ins().imul(plen, eight);
    let bit = b.ins().ishl(one, mul8);
    let m1 = b.ins().isub(bit, one);
    let is8 = b.ins().icmp(IntCC::Equal, plen, eight);
    let all = b.ins().iconst(types::I64, -1);
    let mask = b.ins().select(is8, all, m1);
    let x = b.ins().bxor(adj, arg_payload);
    let masked = b.ins().band(x, mask);
    let eq = b.ins().icmp(IntCC::Equal, masked, zero);
    let res = b.ins().uextend(types::I64, eq);
    b.ins().jump(merge, &[res.into()]);

    b.switch_to_block(slow);
    let r = crate::clif::from_ssa::exceptions::call_checked_value(
        b,
        ctx,
        helper,
        &[exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
    )?;
    b.ins().jump(merge, &[r.into()]);

    b.switch_to_block(merge);
    Ok(Out::Native(b.block_params(merge)[0]))
}
