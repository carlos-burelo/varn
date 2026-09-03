//! Native string-intrinsic lowering for CLIF.
//!
//! Intercepts `Intrinsic` opcodes whose wire byte decodes to
//! `IntrinsicDomain::Str` and calls a dedicated `extern "C"` helper
//! instead of the generic `dispatch_intrinsic` path. The generic path
//! flushes every live boxed register to its home slot and reloads them
//! afterwards — O(live_regs) memory traffic per call. The dedicated
//! helpers take their arguments as direct call parameters, so the CLIF
//! caller only boxes the 2–3 operands it actually passes.
//!
//! `charCodeAt` goes one step further inside a loop: `preheader::emit_str_caches`
//! resolves the receiver's bytes ONCE, so the access itself is an unsigned
//! bounds compare and a byte load, with no call at all.
//!
//! This closes the ~200–300× gap on `charCodeAt` and `substring`/`slice`
//! loops observed in `bench_str_ops.vn`.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::intrinsic_ops::intrinsic_decode;
use varn_core::intrinsic_ops::str::StrOp;
use varn_core::intrinsic_ops::wire::IntrinsicDomain;
use varn_types::register_meta::RegisterMeta;

use super::alloc::{box_or_load_home, def_result, AllocCtx};
use super::emit::{box_bool, box_int, call_helper, call_helper_void, use_int, LoopCaches};
use super::kinds::K;

/// Try to lower a `Str`-domain intrinsic natively. Returns `true` when
/// handled (the CLIF for the call has been emitted); `false` to fall
/// through to the generic `dispatch_intrinsic` helper.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_str_intrinsic_native(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    loops: LoopCaches,
    vars: &[Variable],
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) -> bool {
    let dest = (code[ip] >> 8) as usize;
    let w1 = code[ip + 1];
    let wire_byte = (w1 >> 8) as u8;
    let arg_count = (w1 & 0xFF) as usize;

    let (domain, op) = intrinsic_decode(wire_byte);
    if domain != IntrinsicDomain::Str as u8 {
        return false;
    }

    // CharCodeAt / CodePointAt: 2 args (receiver + pos), result is int.
    if (op == StrOp::CharCodeAt as u8 || op == StrOp::CodePointAt as u8) && arg_count == 2 {
        if emit_char_code_inline(b, actx, loops, vars, state, ip, dest) {
            return true;
        }
        let receiver = box_or_load_home(b, actx, state, dest);
        let pos = box_or_load_home(b, actx, state, dest + 1);
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (pos_tag, pos_payload) = b.ins().isplit(pos);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_char_code_at,
            &[actx.exec_ctx, recv_tag, recv_payload, pos_tag, pos_payload],
        );
        let boxed = box_int(b, res);
        def_result(b, actx, dest, boxed);
        return true;
    }

    // Substring: 2 or 3 args (receiver + start [+ end]), result is heap str.
    if op == StrOp::Substring as u8 && (arg_count == 2 || arg_count == 3) {
        let receiver = box_or_load_home(b, actx, state, dest);
        let start = box_or_load_home(b, actx, state, dest + 1);
        let end = if arg_count == 3 {
            box_or_load_home(b, actx, state, dest + 2)
        } else {
            let null_v = b.ins().iconst(types::I64, 0);
            let null_tag = b.ins().iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
            b.ins().iconcat(null_tag, null_v)
        };
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (st_tag, st_payload) = b.ins().isplit(start);
        let (en_tag, en_payload) = b.ins().isplit(end);

        call_helper_void(
            b,
            actx.cc,
            actx.helpers.str_substring_intrinsic,
            &[
                actx.exec_ctx,
                recv_tag,
                recv_payload,
                st_tag,
                st_payload,
                en_tag,
                en_payload,
            ],
        );
        let res = b.ins().load(
            types::I128,
            MemFlags::trusted(),
            actx.exec_ctx,
            actx.helpers.jit_native_result_offset as i32,
        );
        def_result(b, actx, dest, res);
        return true;
    }

    // Slice: 2 or 3 args (receiver + start [+ end]), result is heap str.
    if op == StrOp::Slice as u8 && (arg_count == 2 || arg_count == 3) {
        let receiver = box_or_load_home(b, actx, state, dest);
        let start = box_or_load_home(b, actx, state, dest + 1);
        let end = if arg_count == 3 {
            box_or_load_home(b, actx, state, dest + 2)
        } else {
            let null_v = b.ins().iconst(types::I64, 0);
            let null_tag = b.ins().iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
            b.ins().iconcat(null_tag, null_v)
        };
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (st_tag, st_payload) = b.ins().isplit(start);
        let (en_tag, en_payload) = b.ins().isplit(end);

        call_helper_void(
            b,
            actx.cc,
            actx.helpers.str_slice_intrinsic,
            &[
                actx.exec_ctx,
                recv_tag,
                recv_payload,
                st_tag,
                st_payload,
                en_tag,
                en_payload,
            ],
        );
        let res = b.ins().load(
            types::I128,
            MemFlags::trusted(),
            actx.exec_ctx,
            actx.helpers.jit_native_result_offset as i32,
        );
        def_result(b, actx, dest, res);
        return true;
    }

    // StartsWith: 2 args (receiver + search), result is bool.
    if op == StrOp::StartsWith as u8 && arg_count == 2 {
        let receiver = box_or_load_home(b, actx, state, dest);
        let search = box_or_load_home(b, actx, state, dest + 1);
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (search_tag, search_payload) = b.ins().isplit(search);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_starts_with_intrinsic,
            &[actx.exec_ctx, recv_tag, recv_payload, search_tag, search_payload],
        );
        let boxed = box_bool(b, res);
        def_result(b, actx, dest, boxed);
        return true;
    }

    // EndsWith: 2 args (receiver + search), result is bool.
    if op == StrOp::EndsWith as u8 && arg_count == 2 {
        let receiver = box_or_load_home(b, actx, state, dest);
        let search = box_or_load_home(b, actx, state, dest + 1);
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (search_tag, search_payload) = b.ins().isplit(search);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_ends_with_intrinsic,
            &[actx.exec_ctx, recv_tag, recv_payload, search_tag, search_payload],
        );
        let boxed = box_bool(b, res);
        def_result(b, actx, dest, boxed);
        return true;
    }

    // Includes: 2 args (receiver + search), result is bool.
    if op == StrOp::Includes as u8 && arg_count == 2 {
        let receiver = box_or_load_home(b, actx, state, dest);
        let search = box_or_load_home(b, actx, state, dest + 1);
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (search_tag, search_payload) = b.ins().isplit(search);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_includes_intrinsic,
            &[actx.exec_ctx, recv_tag, recv_payload, search_tag, search_payload],
        );
        let boxed = box_bool(b, res);
        def_result(b, actx, dest, boxed);
        return true;
    }

    // IndexOf: 2 args (receiver + search), result is int.
    if op == StrOp::IndexOf as u8 && arg_count == 2 {
        let receiver = box_or_load_home(b, actx, state, dest);
        let search = box_or_load_home(b, actx, state, dest + 1);
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (search_tag, search_payload) = b.ins().isplit(search);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_index_of_intrinsic,
            &[actx.exec_ctx, recv_tag, recv_payload, search_tag, search_payload],
        );
        let boxed = box_int(b, res);
        def_result(b, actx, dest, boxed);
        return true;
    }

    // LastIndexOf: 2 args (receiver + search), result is int.
    if op == StrOp::LastIndexOf as u8 && arg_count == 2 {
        let receiver = box_or_load_home(b, actx, state, dest);
        let search = box_or_load_home(b, actx, state, dest + 1);
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (search_tag, search_payload) = b.ins().isplit(search);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_last_index_of_intrinsic,
            &[actx.exec_ctx, recv_tag, recv_payload, search_tag, search_payload],
        );
        let boxed = box_int(b, res);
        def_result(b, actx, dest, boxed);
        return true;
    }

    false
}

/// `charCodeAt` served from the region's hoisted byte view, when there is one.
#[allow(clippy::too_many_arguments)]
fn emit_char_code_inline(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    loops: LoopCaches,
    vars: &[Variable],
    state: &[K],
    ip: usize,
    dest: usize,
) -> bool {
    let Some(cache) = loops.string_at(ip) else {
        return false;
    };
    let Ok(pos) = use_int(b, vars, state, dest + 1) else {
        return false;
    };

    let bytes = b.use_var(cache.bytes);
    let len = b.use_var(cache.len);

    let inline_path = b.create_block();
    let helper_path = b.create_block();
    let in_range = b.create_block();
    let out_of_range = b.create_block();
    let done = b.create_block();
    b.append_block_param(done, types::I128);

    b.ins().brif(bytes, inline_path, &[], helper_path, &[]);

    b.switch_to_block(inline_path);
    let within = b.ins().icmp(IntCC::UnsignedLessThan, pos, len);
    b.ins().brif(within, in_range, &[], out_of_range, &[]);

    b.switch_to_block(in_range);
    let addr = b.ins().iadd(bytes, pos);
    let byte = b.ins().uload8(types::I64, MemFlags::trusted(), addr, 0);
    let boxed = box_int(b, byte);
    b.ins().jump(done, &[boxed.into()]);

    b.switch_to_block(out_of_range);
    let minus_one = b.ins().iconst(types::I64, -1);
    let boxed = box_int(b, minus_one);
    b.ins().jump(done, &[boxed.into()]);

    // Unresolved receiver: the general implementation, which handles SSO,
    // non-ASCII and non-string receivers.
    b.switch_to_block(helper_path);
    let receiver = box_or_load_home(b, actx, state, dest);
    let boxed_pos = box_or_load_home(b, actx, state, dest + 1);
    let (recv_tag, recv_payload) = b.ins().isplit(receiver);
    let (pos_tag, pos_payload) = b.ins().isplit(boxed_pos);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.str_char_code_at,
        &[actx.exec_ctx, recv_tag, recv_payload, pos_tag, pos_payload],
    );
    let boxed = box_int(b, res);
    b.ins().jump(done, &[boxed.into()]);

    b.switch_to_block(done);
    let res = b.block_params(done)[0];
    def_result(b, actx, dest, res);
    true
}
