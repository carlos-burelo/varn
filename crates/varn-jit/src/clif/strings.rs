//! `charCodeAt` inside a loop: `preheader::emit_str_caches` resolves the
//! receiver's bytes ONCE, so the access is an unsigned bounds compare and a
//! byte load, with no call at all; an unresolved receiver takes the
//! `str_char_code_at` helper.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};

use super::alloc::{box_or_load_home, def_result, AllocCtx};
use super::emit::{box_int, call_helper, use_int, LoopCaches};
use super::kinds::K;

/// `charCodeAt` served from the region's hoisted byte view, when there is one.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_char_code_inline(
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

/// The length of a `str` given as its boxed halves, boxed as an `int`. An
/// inline (SSO) string carries its length in the tag; a heap one asks the
/// `str_length` helper. The one lowering of `StrLength`, shared by the
/// bytecode and the SSA paths.
#[allow(clippy::too_many_arguments)]
pub(super) fn str_length_boxed(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    exec_ctx: cranelift_codegen::ir::Value,
    native_result_offset: i32,
    tag: cranelift_codegen::ir::Value,
    payload: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let kind = b
        .ins()
        .band_imm(tag, varn_types::vm_value::KIND_MASK as i64);
    let is_sso = b
        .ins()
        .icmp_imm(IntCC::Equal, kind, varn_types::vm_value::KIND_SSO as i64);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);

    b.ins().brif(is_sso, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let s = b.ins().ushr_imm(tag, 8);
    let sso_len = b.ins().band_imm(s, 0xFF);
    let boxed_len = box_int(b, sso_len);
    b.ins().jump(merge, &[boxed_len.into()]);

    b.switch_to_block(slow);
    super::emit::call_helper_void(b, cc, helper, &[exec_ctx, tag, payload]);
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        exec_ctx,
        native_result_offset,
    );
    b.ins().jump(merge, &[res.into()]);

    b.switch_to_block(merge);
    b.block_params(merge)[0]
}
