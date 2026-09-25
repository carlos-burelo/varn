//! Literals of the proto's constant pool, embedded as the value the pool
//! resolved them to — the same value the interpreter's `LoadConst` reads.

use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::{Literal, PoolEntry};

use super::Ctx;

/// The resolved pool literal `matches` selects, as a boxed value; `what`
/// names it in the error when the pool has none.
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
    let payload = b.ins().iconst(types::I64, cv.raw_payload() as i64);
    Ok(b.ins().iconcat(tag, payload))
}
