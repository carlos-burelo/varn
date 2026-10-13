use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::emit::box_int;
pub(super) fn str_length_boxed(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    exec_ctx: cranelift_codegen::ir::Value,
    native_result_offset: i32,
    error_flag_offset: i32,
    landing: Option<Block>,
    err_ret: Option<Block>,
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
    let byte_len = b.ins().band_imm_u(s, 0xFF);
    let inverted = b.ins().bnot(payload);
    let shifted = b.ins().ishl_imm_u(inverted, 1);
    let lead_clear = b.ins().band(payload, shifted);
    let continuation = b
        .ins()
        .band_imm_u(lead_clear, varn_types::vm_value::SSO_HIGH_BITS as i64);
    let continuation = b.ins().popcnt(continuation);
    let sso_len = b.ins().isub(byte_len, continuation);
    let boxed_len = box_int(b, sso_len);
    b.ins().jump(merge, &[boxed_len.into()]);

    b.switch_to_block(slow);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().store(
        cranelift_codegen::ir::MemFlagsData::trusted(),
        zero,
        exec_ctx,
        error_flag_offset,
    );
    super::emit::call_helper_void(b, cc, helper, &[exec_ctx, tag, payload]);
    let flag = b.ins().load(
        types::I64,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        exec_ctx,
        error_flag_offset,
    );
    let cont = b.create_block();
    match (landing, err_ret) {
        (Some(t), _) => {
            let err_blk = b.create_block();
            b.ins().brif(flag, err_blk, &[], cont, &[]);
            b.switch_to_block(err_blk);
            let eboxed = b.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                exec_ctx,
                native_result_offset,
            );
            b.declare_value_needs_stack_map(eboxed);
            b.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                zero,
                exec_ctx,
                error_flag_offset,
            );
            b.ins().jump(t, &[eboxed.into()]);
            b.switch_to_block(cont);
        }
        (None, Some(e)) => {
            b.ins().brif(flag, e, &[], cont, &[]);
            b.switch_to_block(cont);
        }
        (None, None) => {}
    }
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
