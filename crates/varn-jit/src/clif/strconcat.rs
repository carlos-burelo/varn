//! `StrConcat` lowering.
//!
//! Its own module because `clif::alloc.rs` is already past this repo's
//! refactor threshold, and because the fast path below is a self-contained
//! piece of value-representation knowledge — the SSO encoding from
//! `varn_types::vm_value` — that nothing else in `alloc` needs.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::alloc::{def_result, flush_boxed, live_boxed, reload_boxed, AllocCtx};
use super::emit::{box_or_pass, call_helper};
use super::kinds::K;

/// The bits that identify an SSO value: `VmValue::is_sso` is
/// `(v & (SIGN | QNAN | MASK_TAG)) == (QNAN | TAG_SSO)`.
const SSO_TAG_MASK: u64 =
    varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG;
const SSO_TAG_BITS: u64 = varn_types::vm_value::QNAN | varn_types::vm_value::TAG_SSO;

/// The five payload bytes, at bits 37, 29, 21, 13 and 5 — so bits 5..45.
/// `try_from_sso` writes byte `i` at `37 - i * 8`.
const SSO_BYTES_MASK: u64 = ((1u64 << 40) - 1) << 5;

/// Where `try_from_sso` puts the length, and its maximum.
const SSO_LEN_SHIFT: i64 = 45;
const SSO_MAX_LEN: i64 = 5;

/// `StrConcat dest, a, b` — the concatenation of two strings.
///
/// Two lowerings share one merge point:
///
/// * **inline** — both operands are SSO, so their bytes and lengths are in the
///   values themselves and the result fits in a value too. Nothing allocates,
///   so nothing has to be rooted: no flush, no call, no reload. Assembled with
///   shifts, mirroring `VmValue::try_from_sso`.
/// * **helper** — everything else. `jit_str_concat` allocates and can
///   therefore GC, so live heap refs go to their home slots first and come
///   back after.
///
/// The flush/reload lives in the helper arm ONLY. Hoisting it above the branch
/// would pay the safepoint on the path that exists to avoid it, which is the
/// whole point of the split — the same shape `alloc::emit_backedge_safepoint`
/// uses for its nursery check.
pub(super) fn emit_str_concat(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let a = box_or_pass(b, actx.vars, state, a_r);
    let bb = box_or_pass(b, actx.vars, state, b_r);

    // Guard, all inline. SSO refuses non-ASCII at construction, so a value that
    // IS sso is ASCII by induction and no byte test is needed here.
    let tag_mask = b.ins().iconst(types::I64, SSO_TAG_MASK as i64);
    let tag_bits = b.ins().iconst(types::I64, SSO_TAG_BITS as i64);
    let a_tag = b.ins().band(a, tag_mask);
    let b_tag = b.ins().band(bb, tag_mask);
    let a_sso = b.ins().icmp(IntCC::Equal, a_tag, tag_bits);
    let b_sso = b.ins().icmp(IntCC::Equal, b_tag, tag_bits);
    let both_sso = b.ins().band(a_sso, b_sso);

    // Lengths are read unconditionally: on a non-SSO operand they are garbage,
    // but `both_sso` gates the branch and the fast block is the only reader.
    let a_len = {
        let s = b.ins().ushr_imm(a, SSO_LEN_SHIFT);
        b.ins().band_imm(s, 0x7)
    };
    let b_len = {
        let s = b.ins().ushr_imm(bb, SSO_LEN_SHIFT);
        b.ins().band_imm(s, 0x7)
    };
    let total_len = b.ins().iadd(a_len, b_len);
    let fits = b
        .ins()
        .icmp_imm(IntCC::UnsignedLessThanOrEqual, total_len, SSO_MAX_LEN);
    let take_inline = b.ins().band(both_sso, fits);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64);
    b.ins().brif(take_inline, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    // `a`'s bytes are already in place: byte i of the result is byte i of `a`
    // for i < a_len. `b`'s byte j belongs at result index a_len + j, which is
    // 8 * a_len bits lower than where it sits in `b` — one shift for the whole
    // payload. Bytes past a length are zero, so no masking of the tail is
    // needed and the two payloads simply or together.
    let bytes_mask = b.ins().iconst(types::I64, SSO_BYTES_MASK as i64);
    let a_bytes = b.ins().band(a, bytes_mask);
    let b_bytes = b.ins().band(bb, bytes_mask);
    let shift = b.ins().ishl_imm(a_len, 3);
    let b_shifted = b.ins().ushr(b_bytes, shift);
    let len_field = b.ins().ishl_imm(total_len, SSO_LEN_SHIFT);
    let tagged = b.ins().bor(tag_bits, len_field);
    let with_a = b.ins().bor(tagged, a_bytes);
    let inline_res = b.ins().bor(with_a, b_shifted);
    b.ins().jump(merge, &[inline_res.into()]);

    b.switch_to_block(slow);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let called = call_helper(b, actx.cc, actx.helpers.str_concat, &[actx.exec_ctx, a, bb]);
    reload_boxed(b, actx, state, &regs);
    b.ins().jump(merge, &[called.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    def_result(b, actx, dest, res);
}
