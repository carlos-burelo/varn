//! Numeric semantics of binary operators — single source of truth.
//!
//! Every layer that reasons about numeric binary operations (binder
//! inference, checker validation, expression annotations, HIR lowering,
//! VM and JIT fast paths) must derive its answer from the rules here.
//! Divergent hand-copies of these rules are how the interpreter and the
//! JIT once disagreed about the same program.
//!
//! The semantics:
//!
//! - `int` is a 48-bit two's-complement integer (the NaN-box payload).
//!   All integer arithmetic **wraps at 48 bits**, identically in the
//!   interpreter and the JIT. There is no silent promotion to float on
//!   overflow.
//! - `int / int` always produces a `float`. Integer division that
//!   sometimes returned `int` (when exact) made a value's runtime type
//!   depend on the values themselves, which poisons every typed fast
//!   path downstream.
//! - `int % int` produces an `int` (truncated remainder). Zero divisor
//!   raises a runtime error, as does `int / 0`.
//! - `int ** int` produces an `int` (wrapping). A negative exponent
//!   raises a runtime error instead of silently producing a float.
//! - Mixed `int`/`float` operands promote to `float`; `decimal` absorbs
//!   `int`. `decimal`/`float` mixes are a checker error and have no
//!   numeric class.

use crate::ast::operators::BinaryOp;

/// Wrap a value to Varn's `int`: 48-bit two's complement. This is the
/// canonical definition of integer overflow — compile-time folding, the
/// interpreter and the JIT must all agree with it bit for bit.
#[inline(always)]
pub fn wrap_i48(v: i64) -> i64 {
    (v << 16) >> 16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericOperand {
    Int,
    Float,
    Decimal,
}

/// The common operand class of a numeric binary operation, or `None` when
/// the two sides don't reduce to a single numeric class (unknown types,
/// strings, decimal/float mixes, ...).
pub fn binary_operand_kind(
    l: Option<NumericOperand>,
    r: Option<NumericOperand>,
) -> Option<NumericOperand> {
    use NumericOperand::*;
    match (l?, r?) {
        (Int, Int) => Some(Int),
        (Decimal, Decimal) | (Decimal, Int) | (Int, Decimal) => Some(Decimal),
        (Float, Float) | (Float, Int) | (Int, Float) => Some(Float),
        (Decimal, Float) | (Float, Decimal) => None,
    }
}

/// Result class of an arithmetic op whose operands share class `operands`.
/// `int / int → float` is the one place where the result class differs
/// from the operand class.
pub fn binary_result_kind(op: BinaryOp, operands: NumericOperand) -> NumericOperand {
    match (op, operands) {
        (BinaryOp::Div, NumericOperand::Int) => NumericOperand::Float,
        (_, k) => k,
    }
}
