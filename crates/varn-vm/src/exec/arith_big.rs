//! `bigint` arithmetic (spec §6–§7): exact, with `int` operands widening
//! exactly into `bigint`. Division follows `int` (truncating `/`,
//! dividend-signed `%`), faults included.

use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};
use varn_types::Value;

#[derive(Clone, Copy)]
pub(crate) enum BigOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}

fn bigint_of(v: VmValue, heap: &Heap) -> Option<BigInt> {
    if heap.is_int(v) {
        return Some(BigInt::from(heap.as_int(v)));
    }
    if !v.is_heap() {
        return None;
    }
    match heap.get(v.as_heap_idx()) {
        Some(HeapObj::BigInt(b)) => Some((**b).clone()),
        _ => None,
    }
}

fn is_bigint(v: VmValue, heap: &Heap) -> bool {
    v.is_heap() && matches!(heap.get(v.as_heap_idx()), Some(HeapObj::BigInt(_)))
}

fn alloc(heap: &mut Heap, v: BigInt) -> VmValue {
    heap.intern(Value::BigInt(Box::new(v)))
}

fn fault(f: varn_core::IntDivFault, op: &str) -> RuntimeError {
    match f {
        varn_core::IntDivFault::DivisionByZero => RuntimeError::division_by_zero(if op == "%" {
            "modulo by zero"
        } else {
            "division by zero"
        }),
        varn_core::IntDivFault::Overflow => RuntimeError::integer_overflow("bigint overflow"),
    }
}

/// `a op b` when at least one side is a `bigint` and the other is a
/// `bigint` or an `int`; `None` hands the operation back to the caller.
#[cold]
pub(crate) fn binary(
    op: BigOp,
    a: VmValue,
    b: VmValue,
    heap: &mut Heap,
) -> Option<VmResult<VmValue>> {
    if !is_bigint(a, heap) && !is_bigint(b, heap) {
        return None;
    }
    let (x, y) = (bigint_of(a, heap)?, bigint_of(b, heap)?);
    use varn_core::numeric_big::{div_big, rem_big};
    let r = match op {
        BigOp::Add => Ok(x + y),
        BigOp::Sub => Ok(x - y),
        BigOp::Mul => Ok(x * y),
        BigOp::Div => div_big(&x, &y).map_err(|f| fault(f, "/")),
        BigOp::Rem => rem_big(&x, &y).map_err(|f| fault(f, "%")),
        BigOp::Pow => {
            if y.is_negative() {
                Err(RuntimeError::new("negative exponent in bigint power"))
            } else {
                y.to_u32()
                    .map(|e| num_traits::pow::Pow::pow(&x, e))
                    .ok_or_else(|| RuntimeError::new("bigint exponent too large"))
            }
        }
    };
    Some(r.map(|v| alloc(heap, v)))
}

/// `-a` for a `bigint`, `None` otherwise.
#[cold]
pub(crate) fn negate(a: VmValue, heap: &mut Heap) -> Option<VmValue> {
    if !is_bigint(a, heap) {
        return None;
    }
    let x = bigint_of(a, heap)?;
    Some(alloc(heap, -x))
}
