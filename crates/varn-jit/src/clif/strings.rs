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
