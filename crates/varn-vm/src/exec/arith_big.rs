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

const MAX_BITS: u64 = 1 << 30;

fn unit_power(x: &BigInt, y: &BigInt) -> BigInt {
    let one = BigInt::from(1);
    if *x == one {
        one
    } else if x.sign() == num_bigint::Sign::NoSign {
        if y.sign() == num_bigint::Sign::NoSign {
            one
        } else {
            x.clone()
        }
    } else if y.bit(0) {
        x.clone()
    } else {
        one
    }
}

#[cold]
fn too_large() -> RuntimeError {
    RuntimeError::range_error(format!("a bigint cannot exceed {MAX_BITS} bits"))
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
        BigOp::Mul if x.bits() + y.bits() > MAX_BITS => Err(too_large()),
        BigOp::Mul => Ok(x * y),
        BigOp::Div => div_big(&x, &y).map_err(|f| fault(f, "/")),
        BigOp::Rem => rem_big(&x, &y).map_err(|f| fault(f, "%")),
        BigOp::Pow => {
            if y.is_negative() {
                Err(crate::exec::arith::negative_exponent_big(&y))
            } else if x.bits() <= 1 {
                Ok(unit_power(&x, &y))
            } else {
                match y.to_u32() {
                    Some(e) if x.bits().saturating_mul(u64::from(e)) <= MAX_BITS => {
                        Ok(num_traits::pow::Pow::pow(&x, e))
                    }
                    Some(_) | None => Err(too_large()),
                }
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
