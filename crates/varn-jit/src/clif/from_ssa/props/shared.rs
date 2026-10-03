use cranelift_codegen::ir::Value;
use cranelift_frontend::FunctionBuilder;

use super::super::store::use_heap;
use super::super::Ctx;

pub(crate) fn emit_this(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<Value, String> {
    use_heap(b, ctx, 0)
}

pub(crate) fn str_idx(ctx: &Ctx<'_>, s: &str) -> Result<usize, String> {
    ctx.proto
        .chunk
        .constants
        .iter()
        .position(|e| {
            matches!(e, varn_types::PoolEntry::Literal(varn_types::Literal::Str(t)) if t.as_ref() == s)
        })
        .ok_or_else(|| format!("from_ssa: string {s:?} not in pool"))
}
