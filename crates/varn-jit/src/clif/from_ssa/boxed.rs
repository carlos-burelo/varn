//! Ops whose semantics live behind a boxed runtime helper.
//!
//! Integer division/modulo/power and float modulo/power are not native ISA
//! operations with Varn's exact semantics (division by zero, `int` overflow,
//! `f64` rounding): the VM owns them as helpers over boxed `VmValue`s, writing
//! the result into the live `ExecCtx`. A leaf has no `exec_ctx` parameter, so
//! the live context is recovered through the same `current_exec_ctx` getter the
//! bytecode lowering uses — the reason these ops can stay in the leaf lowering
//! instead of forcing the function frame-aware. The getter is equally valid in
//! a frame-aware body, so one path serves both.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::SsaBinOp;

use super::Ctx;

use super::super::emit::{
    box_f64, box_int, call_helper, call_helper_void, unbox_f64_coerce, unbox_int,
};

/// Boxed-destination op: operands arrive native (`I64` for `Int*`, `F64` for
/// `Float*`) and the result is returned native in the class `dest_float`
/// selects (which is `Float` for `IntDiv` even though its operands are `Int`).
pub(super) fn emit_bin(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaBinOp,
    a: Value,
    c: Value,
    dest_float: bool,
) -> Result<Value, String> {
    use SsaBinOp::*;
    let helpers = ctx.helpers;
    let cc = ctx.cc;
    let (helper, operand_float) = match op {
        IntDiv => (helpers.div, false),
        IntMod => (helpers.modulo, false),
        IntPow => (helpers.pow, false),
        FloatMod => (helpers.modulo, true),
        FloatPow => (helpers.pow, true),
        _ => return Err("from_ssa: not a boxed op".into()),
    };

    let (a_tag, a_payload) = box_native(b, a, operand_float);
    let (b_tag, b_payload) = box_native(b, c, operand_float);
    let live = call_helper(b, cc, helpers.current_exec_ctx, &[]);
    call_helper_void(b, cc, helper, &[live, a_tag, a_payload, b_tag, b_payload]);
    let boxed = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        live,
        helpers.jit_native_result_offset as i32,
    );
    Ok(if dest_float {
        unbox_f64_coerce(b, boxed)
    } else {
        unbox_int(b, boxed)
    })
}

fn box_native(b: &mut FunctionBuilder, v: Value, float: bool) -> (Value, Value) {
    let boxed = if float { box_f64(b, v) } else { box_int(b, v) };
    b.ins().isplit(boxed)
}
