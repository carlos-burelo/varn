use cranelift_codegen::ir::Value;

use super::super::Ctx;

pub(crate) fn emit_this(ctx: &Ctx<'_>) -> Result<Value, String> {
    ctx.this
        .ok_or_else(|| "from_ssa: `this` in a frameless body".into())
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
