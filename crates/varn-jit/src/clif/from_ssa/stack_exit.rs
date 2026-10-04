use cranelift_codegen::ir::{types, InstBuilder, MemFlagsData, Value};
use cranelift_frontend::FunctionBuilder;

use crate::JitHelpers;

pub(crate) fn publish(b: &mut FunctionBuilder, helpers: &JitHelpers, exec_ctx: Value) {
    let sp = b.ins().get_stack_pointer(types::I64);
    let fp = b.ins().get_frame_pointer(types::I64);
    let off = helpers.jit_exit_offset as i32;
    b.ins().store(MemFlagsData::trusted(), sp, exec_ctx, off);
    b.ins()
        .store(MemFlagsData::trusted(), fp, exec_ctx, off + 8);
}
