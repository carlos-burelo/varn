use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use bigdecimal::BigDecimal as Decimal;
use num_traits::Zero;
use varn_core::{add_int, mul_int, neg_int, pow_int, sub_int, INT_MAX, INT_MIN};

/// The `integer overflow` error, naming the operands so the message points at
/// the actual computation rather than just the line.
///
/// `int` is 64 bits with hardware-checked overflow. Leaving that range
/// raises an integer overflow error.
#[cold]
#[inline(never)]
fn overflow(op: &str, a: i64, b: i64) -> RuntimeError {
    RuntimeError::integer_overflow(format!(
        "integer overflow: {a} {op} {b} is outside int ({INT_MIN}..={INT_MAX})"
    ))
}

#[cold]
#[inline(never)]
fn overflow_neg(a: i64) -> RuntimeError {
    RuntimeError::integer_overflow(format!(
        "integer overflow: -({a}) is outside int ({INT_MIN}..={INT_MAX})"
    ))
}

/// The fault of an integer `/` or `%`, as the platform error it raises.
#[cold]
#[inline(never)]
pub(crate) fn int_div_fault(
    fault: varn_core::IntDivFault,
    op: &str,
    a: i64,
    b: i64,
) -> RuntimeError {
    match fault {
        varn_core::IntDivFault::DivisionByZero => RuntimeError::division_by_zero(if op == "%" {
            "modulo by zero"
        } else {
            "division by zero"
        }),
        varn_core::IntDivFault::Overflow => overflow(op, a, b),
    }
}

/// The `decimal` payload of `v`, or `None`.
///
/// A decimal lives ONLY as a `HeapObj::Decimal`, so the heap tag test alone
/// rejects every int, float, bool, null and inline (SSO) string without ever
/// touching the heap.
///
/// This replaces a pair of `Heap::extract_val` calls that every non-int
/// arithmetic path used to make just to ask "is either side a decimal?".
/// `extract_val` builds a full `varn_types::Value` — a 24-variant enum — and
/// for `HeapObj::Array` it DEEP-COPIES the whole array into a fresh
/// `Vec<Value>` (see `heap::intern::extract_val`). Every `float - float` was
/// paying two enum constructions, and every `array + x` a full array copy, to
/// answer a question a single tag test answers.
#[inline(always)]
fn decimal_of(v: VmValue, heap: &Heap) -> Option<Decimal> {
    if !v.is_heap() {
        return None;
    }
    match heap.get(v.as_heap_idx()) {
        Some(HeapObj::Decimal(d)) => Some((**d).clone()),
        _ => None,
    }
}

/// Both operands as decimals, when this is a decimal operation at all.
///
/// `int` absorbs into `decimal` and a `decimal`/`float` mix is a checker error
/// that never reaches the VM — see `varn_core::numeric::binary_operand_kind`,
/// the single source of truth these arms mirror.
///
/// Cold and outlined: callers reach it only after the int/int fast path has
/// missed AND at least one side is a heap value, so the hot numeric paths do
/// not carry its code.
#[cold]
fn decimal_pair(a: VmValue, b: VmValue, heap: &Heap) -> Option<(Decimal, Decimal)> {
    match (decimal_of(a, heap), decimal_of(b, heap)) {
        (Some(x), Some(y)) => Some((x, y)),
        (Some(x), None) if heap.is_int(b) => Some((x, Decimal::from(heap.as_int(b)))),
        (None, Some(y)) if heap.is_int(a) => Some((Decimal::from(heap.as_int(a)), y)),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn add(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return match add_int(x, y) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("+", x, y)),
        };
    }
    // Both sides numeric → float add. A bare `is_f64() || is_f64()` here is
    // wrong: `str + float` must fall through to the concat checks below, not
    // coerce the string operand to 0.0.
    if (a.is_f64() || heap.is_int(a)) && (b.is_f64() || heap.is_int(b)) {
        return Ok(VmValue::from_f64(heap.to_f64_val(a) + heap.to_f64_val(b)));
    }

    if a.is_sso() || b.is_sso() {
        return Ok(crate::exec::strings::str_concat(a, b, heap));
    }
    if a.is_heap() || b.is_heap() {
        let a_is_str = a.is_heap() && matches!(heap.get(a.as_heap_idx()), Some(HeapObj::Str(_)));
        let b_is_str = b.is_heap() && matches!(heap.get(b.as_heap_idx()), Some(HeapObj::Str(_)));
        if a_is_str || b_is_str {
            return Ok(crate::exec::strings::str_concat(a, b, heap));
        }
        if let Some(r) = crate::exec::arith_big::binary(crate::exec::arith_big::BigOp::Add, a, b, heap) {
            return r;
        }

        if let Some((x, y)) = decimal_pair(a, b, heap) {
            return Ok(heap.alloc_decimal(x + y));
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) + heap.to_f64_val(b)))
}

#[inline(always)]
pub(crate) fn sub(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return match sub_int(x, y) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("-", x, y)),
        };
    }
    if a.is_heap() || b.is_heap() {
        if let Some(r) = crate::exec::arith_big::binary(crate::exec::arith_big::BigOp::Sub, a, b, heap) {
            return r;
        }
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            return Ok(heap.alloc_decimal(x - y));
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) - heap.to_f64_val(b)))
}

#[inline(always)]
pub(crate) fn mul(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return match mul_int(x, y) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("*", x, y)),
        };
    }
    if a.is_heap() || b.is_heap() {
        if let Some(r) = crate::exec::arith_big::binary(crate::exec::arith_big::BigOp::Mul, a, b, heap) {
            return r;
        }
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            return Ok(heap.alloc_decimal(x * y));
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) * heap.to_f64_val(b)))
}

#[inline(always)]
pub(crate) fn div(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return varn_core::div_int(x, y)
            .map(VmValue::from_int)
            .map_err(|f| int_div_fault(f, "/", x, y));
    }
    if a.is_heap() || b.is_heap() {
        if let Some(r) = crate::exec::arith_big::binary(crate::exec::arith_big::BigOp::Div, a, b, heap) {
            return r;
        }
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            if y.is_zero() {
                return Err(RuntimeError::division_by_zero("division by zero"));
            }
            let q = varn_core::numeric_big::div_decimal(&x, &y)
                .map_err(|f| int_div_fault(f, "/", 0, 0))?;
            return Ok(heap.alloc_decimal(q));
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) / heap.to_f64_val(b)))
}

#[inline(always)]
pub(crate) fn modulo(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return varn_core::rem_int(x, y)
            .map(|r| heap.make_int(r))
            .map_err(|f| int_div_fault(f, "%", x, y));
    }
    if a.is_heap() || b.is_heap() {
        if let Some(r) = crate::exec::arith_big::binary(crate::exec::arith_big::BigOp::Rem, a, b, heap) {
            return r;
        }
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            if y.is_zero() {
                return Err(RuntimeError::division_by_zero("modulo by zero"));
            }
            return Ok(heap.alloc_decimal(x % y));
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) % heap.to_f64_val(b)))
}

#[inline(always)]
pub(crate) fn pow(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let exp = heap.as_int(b);
        if exp < 0 {
            return Err(RuntimeError::new("negative exponent in integer power"));
        }
        let base = heap.as_int(a);
        let e = u32::try_from(exp).unwrap_or(u32::MAX);
        return match pow_int(base, e) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("**", base, exp)),
        };
    }
    if let Some(r) = crate::exec::arith_big::binary(crate::exec::arith_big::BigOp::Pow, a, b, heap) {
        return r;
    }
    Ok(VmValue::from_f64(
        heap.to_f64_val(a).powf(heap.to_f64_val(b)),
    ))
}

#[inline(always)]
pub(crate) fn negate(a: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) {
        let x = heap.as_int(a);
        return match neg_int(x) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow_neg(x)),
        };
    }
    if a.is_f64() {
        return Ok(VmValue::from_f64(-a.as_f64()));
    }
    if let Some(d) = decimal_of(a, &*heap) {
        return Ok(heap.alloc_decimal(-d));
    }
    if let Some(r) = crate::exec::arith_big::negate(a, heap) {
        return Ok(r);
    }
    Ok(VmValue::from_f64(-heap.to_f64_val(a)))
}

#[inline(always)]
pub(crate) fn bit_and(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a) & heap.as_int(b);
    heap.make_int(r)
}

#[inline(always)]
pub(crate) fn bit_or(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a) | heap.as_int(b);
    heap.make_int(r)
}

#[inline(always)]
pub(crate) fn bit_xor(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a) ^ heap.as_int(b);
    heap.make_int(r)
}

#[inline(always)]
pub(crate) fn shl(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap
        .as_int(a)
        .wrapping_shl(heap.to_f64_val(b) as i32 as u32 & 63);
    VmValue::from_int_wrapping(r)
}

#[inline(always)]
pub(crate) fn shr(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap
        .as_int(a)
        .wrapping_shr(heap.to_f64_val(b) as i32 as u32 & 63);
    VmValue::from_int_wrapping(r)
}

#[inline(always)]
pub(crate) fn ushr(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = ((heap.as_int(a) as u64).wrapping_shr(heap.to_f64_val(b) as i32 as u32 & 63)) as i64;
    VmValue::from_int_wrapping(r)
}
