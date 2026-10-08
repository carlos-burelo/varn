use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};

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
    match heap.get(v.as_heap()) {
        Some(HeapObj::BigInt(b)) => Some((**b).clone()),
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => None,
    }
}

fn is_bigint(v: VmValue, heap: &Heap) -> bool {
    v.is_heap() && matches!(heap.get(v.as_heap()), Some(HeapObj::BigInt(_)))
}

fn alloc(heap: &mut Heap, v: BigInt) -> VmValue {
    heap.alloc_bigint(v)
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

#[cold]
pub(crate) fn negate(a: VmValue, heap: &mut Heap) -> Option<VmValue> {
    if !is_bigint(a, heap) {
        return None;
    }
    let x = bigint_of(a, heap)?;
    Some(alloc(heap, -x))
}
