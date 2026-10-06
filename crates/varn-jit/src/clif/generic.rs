


use super::emit::{call_helper, call_helper_void};
use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::FunctionBuilder;



pub(crate) fn boxed_binop(
    b: &mut FunctionBuilder,
    cc: CallConv,
    helper: usize,
    exec_ctx: cranelift_codegen::ir::Value,
    native_result_offset: i32,
    a: (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value),
    c: (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value),
) -> cranelift_codegen::ir::Value {
    call_helper_void(b, cc, helper, &[exec_ctx, a.0, a.1, c.0, c.1]);
    b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        exec_ctx,
        native_result_offset,
    )
}





pub(crate) fn boxed_compare(
    b: &mut FunctionBuilder,
    cc: CallConv,
    helper: usize,
    exec_ctx: cranelift_codegen::ir::Value,
    equality: Option<bool>,
    a: (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value),
    c: (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value),
) -> cranelift_codegen::ir::Value {
    let (a_tag, a_payload) = a;
    let (b_tag, b_payload) = c;
    let Some(is_eq) = equality else {
        return call_helper(
            b,
            cc,
            helper,
            &[exec_ctx, a_tag, a_payload, b_tag, b_payload],
        );
    };
    let tag_eq = b.ins().icmp(IntCC::Equal, a_tag, b_tag);
    let pay_eq = b.ins().icmp(IntCC::Equal, a_payload, b_payload);
    let bits_eq = b.ins().band(tag_eq, pay_eq);

    let a_kind = b.ins().band_imm_u(a_tag, 0xFF);
    let not_float = b.ins().icmp_imm_u(
        IntCC::NotEqual,
        a_kind,
        varn_types::vm_value::KIND_FLOAT as i64,
    );
    let same_non_float = b.ins().band(bits_eq, not_float);

    let a_is_sso = b
        .ins()
        .icmp_imm_u(IntCC::Equal, a_kind, varn_types::vm_value::KIND_SSO as i64);
    let b_kind = b.ins().band_imm_u(b_tag, 0xFF);
    let b_is_sso = b
        .ins()
        .icmp_imm_u(IntCC::Equal, b_kind, varn_types::vm_value::KIND_SSO as i64);
    let both_sso = b.ins().band(a_is_sso, b_is_sso);

    let can_inline = b.ins().bor(same_non_float, both_sso);

    let fast_blk = b.create_block();
    let slow_blk = b.create_block();
    let merge_blk = b.create_block();
    b.append_block_param(merge_blk, types::I64);

    b.ins().brif(can_inline, fast_blk, &[], slow_blk, &[]);

    b.switch_to_block(fast_blk);
    let eq = b.ins().uextend(types::I64, bits_eq);
    let fast_res = if is_eq { eq } else { b.ins().bxor_imm_u(eq, 1) };
    b.ins().jump(merge_blk, &[fast_res.into()]);

    b.switch_to_block(slow_blk);
    let slow_res = call_helper(
        b,
        cc,
        helper,
        &[exec_ctx, a_tag, a_payload, b_tag, b_payload],
    );
    b.ins().jump(merge_blk, &[slow_res.into()]);

    b.switch_to_block(merge_blk);
    b.block_params(merge_blk)[0]
}
