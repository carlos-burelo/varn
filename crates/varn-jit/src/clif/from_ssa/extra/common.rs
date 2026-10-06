use super::super::Ctx;
use crate::JitHelpers;
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

pub(super) fn stage(b: &mut FunctionBuilder, ctx: &Ctx<'_>, vals: &[Value]) -> (Value, Value) {
    let addr = super::super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    (addr, b.ins().iconst(types::I64, vals.len() as i64))
}

pub(super) fn frame<'a>(ctx: &'a Ctx<'a>) -> Result<&'a super::super::FrameIo<'a>, String> {
    ctx.frame
        .as_ref()
        .ok_or_else(|| "from_ssa: extra op without a frame".to_owned())
}

pub(super) fn native(b: &mut FunctionBuilder, ectx: Value, h: &JitHelpers) -> Value {
    b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        h.jit_native_result_offset as i32,
    )
}
