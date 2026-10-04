//! String primitives shared with the SSA lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::emit::box_int;

/// The length of a `str` given as its boxed halves, boxed as an `int`. An
/// inline (SSO) string carries its length in the tag; a heap one asks the
/// `str_length` helper.
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
        .band_imm_u(tag, varn_types::vm_value::KIND_MASK as i64);
    let is_sso = b
        .ins()
        .icmp_imm_u(IntCC::Equal, kind, varn_types::vm_value::KIND_SSO as i64);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);

    b.ins().brif(is_sso, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let s = b.ins().ushr_imm_u(tag, 8);
    let sso_len = b.ins().band_imm_u(s, 0xFF);
    let boxed_len = box_int(b, sso_len);
    b.ins().jump(merge, &[boxed_len.into()]);

    b.switch_to_block(slow);
    super::emit::call_helper_void(b, cc, helper, &[exec_ctx, tag, payload]);
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        exec_ctx,
        native_result_offset,
    );
    b.ins().jump(merge, &[res.into()]);

    b.switch_to_block(merge);
    b.block_params(merge)[0]
}
