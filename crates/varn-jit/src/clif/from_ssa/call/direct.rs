use cranelift_codegen::ir::{
    condcodes::IntCC, types, InstBuilder, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::FunctionBuilder;

use super::super::super::abi::{wrapper_returns_via_sret, wrapper_signature};
use super::super::super::emit::call_helper_void;
use super::super::{store, Ctx};

pub(crate) fn entry_out_slot(b: &mut FunctionBuilder) -> Value {
    let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
    b.ins().stack_addr(types::I64, slot, 0)
}

pub(crate) fn run_entered_or(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    entry: Value,
    out: Value,
) -> Value {
    let entered = b.create_block();
    let declined = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);
    let taken = b.ins().icmp_imm_u(IntCC::NotEqual, entry, 0);
    b.ins().brif(taken, entered, &[], declined, &[]);

    b.switch_to_block(entered);
    let direct = run_entered(b, ctx, entry, out);
    store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[direct.into()]);

    b.switch_to_block(declined);
    let fallback = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ctx.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[fallback.into()]);

    b.switch_to_block(merge);
    b.block_params(merge)[0]
}

fn run_entered(b: &mut FunctionBuilder, ctx: &Ctx<'_>, entry: Value, out: Value) -> Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let closure = b.ins().load(types::I64, m, out, 0);
    let base = b.ins().load(types::I64, m, out, 8);
    let null = b.ins().iconst(types::I64, 0);
    let sig = b.import_signature(wrapper_signature(ctx.cc));
    let result = if wrapper_returns_via_sret(ctx.cc) {
        let slot =
            b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 4));
        let ret = b.ins().stack_addr(types::I64, slot, 0);
        b.ins()
            .call_indirect(sig, entry, &[ret, null, closure, base, ctx.exec_ctx]);
        b.ins().load(types::I128, m, ret, 0)
    } else {
        let call = b
            .ins()
            .call_indirect(sig, entry, &[null, closure, base, ctx.exec_ctx]);
        let parts = b.inst_results(call).to_vec();
        b.ins().iconcat(parts[0], parts[1])
    };
    call_helper_void(b, ctx.cc, ctx.helpers.jit_call_leave, &[ctx.exec_ctx, base]);
    result
}
