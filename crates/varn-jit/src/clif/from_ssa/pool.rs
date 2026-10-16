use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::{Literal, PoolEntry, VmValue};

use super::Ctx;

pub(super) fn literal(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    what: &str,
    matches: impl Fn(&Literal) -> bool,
) -> Result<Value, String> {
    let idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| matches!(e, PoolEntry::Literal(l) if matches(l)))
        .ok_or_else(|| format!("from_ssa: {what} constant not in pool"))?;
    let cv = ctx
        .constants
        .get(idx)
        .ok_or_else(|| format!("from_ssa: unresolved {what} constant"))?;
    let tag = b.ins().iconst(types::I64, cv.raw_tag() as i64);
    let payload = if payload_is_value(*cv) {
        b.ins().iconst(types::I64, cv.raw_payload() as i64)
    } else {
        crate::clif::reloc::abs_value(b, crate::clif::reloc::AbsSym::Literal(idx as u32))
    };
    Ok(b.ins().iconcat(tag, payload))
}

fn payload_is_value(v: VmValue) -> bool {
    use varn_types::vm_value::{
        KIND_BOOL, KIND_FLOAT, KIND_HEAP, KIND_INT, KIND_NULL, KIND_SSO, KIND_SYMBOL,
    };
    match v.kind() {
        KIND_NULL | KIND_BOOL | KIND_INT | KIND_FLOAT | KIND_SSO => true,
        KIND_HEAP | KIND_SYMBOL => false,
        _ => false,
    }
}
