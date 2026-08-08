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
//! This closes the ~200–300× gap on `charCodeAt` and `substring`/`slice`
//! loops observed in `bench_str_ops.vn`.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::intrinsic_ops::str::StrOp;
use varn_core::intrinsic_ops::wire::IntrinsicDomain;
use varn_core::intrinsic_ops::intrinsic_decode;
use varn_types::register_meta::RegisterMeta;
use varn_types::VmValue;

use super::alloc::AllocCtx;
use super::emit::{box_or_pass, call_helper};
use super::kinds::K;

/// Try to lower a `Str`-domain intrinsic natively. Returns `true` when
/// handled (the CLIF for the call has been emitted); `false` to fall
/// through to the generic `dispatch_intrinsic` helper.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_str_intrinsic_native(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
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
        let receiver = box_or_pass(b, vars, state, dest);
        let pos = box_or_pass(b, vars, state, dest + 1);

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_char_code_at,
            &[actx.exec_ctx, receiver, pos],
        );
        b.def_var(vars[dest], res);
        return true;
    }

    // Substring: 2 or 3 args (receiver + start [+ end]), result is heap str.
    if op == StrOp::Substring as u8 && (arg_count == 2 || arg_count == 3) {
        let receiver = box_or_pass(b, vars, state, dest);
        let start = box_or_pass(b, vars, state, dest + 1);
        let end = if arg_count == 3 {
            box_or_pass(b, vars, state, dest + 2)
        } else {
            b.ins().iconst(types::I64, VmValue::null().0 as i64)
        };

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_substring_intrinsic,
            &[actx.exec_ctx, receiver, start, end],
        );
        b.def_var(vars[dest], res);
        return true;
    }

    // Slice: 2 or 3 args (receiver + start [+ end]), result is heap str.
    if op == StrOp::Slice as u8 && (arg_count == 2 || arg_count == 3) {
        let receiver = box_or_pass(b, vars, state, dest);
        let start = box_or_pass(b, vars, state, dest + 1);
        let end = if arg_count == 3 {
            box_or_pass(b, vars, state, dest + 2)
        } else {
            b.ins().iconst(types::I64, VmValue::null().0 as i64)
        };

        let res = call_helper(
            b,
            actx.cc,
            actx.helpers.str_slice_intrinsic,
            &[actx.exec_ctx, receiver, start, end],
        );
        b.def_var(vars[dest], res);
        return true;
    }

    false
}
