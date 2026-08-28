use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use rust_decimal::Decimal;
use varn_core::{add_i48, mul_i48, neg_i48, pow_i48, sub_i48, INT_MAX, INT_MIN};

/// The `integer overflow` error, naming the operands so the message points at
/// the actual computation rather than just the line.
///
/// `int` is 48 bits (the NaN-box payload). Leaving that range used to wrap
/// silently, which turned `1000000007 * 1000000007` into a wrong number with
/// no signal; the range is unchanged, only the behaviour at its edge.
#[cold]
#[inline(never)]
fn overflow(op: &str, a: i64, b: i64) -> RuntimeError {
    RuntimeError::new(format!(
        "integer overflow: {a} {op} {b} is outside int ({INT_MIN}..={INT_MAX})"
    ))
}

#[cold]
#[inline(never)]
fn overflow_neg(a: i64) -> RuntimeError {
    RuntimeError::new(format!(
        "integer overflow: -({a}) is outside int ({INT_MIN}..={INT_MAX})"
    ))
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
        Some(HeapObj::Decimal(d)) => Some(**d),
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
        return match add_i48(x, y) {
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
        return match sub_i48(x, y) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("-", x, y)),
        };
    }
    if a.is_heap() || b.is_heap() {
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
        return match mul_i48(x, y) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("*", x, y)),
        };
    }
    if a.is_heap() || b.is_heap() {
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            return Ok(heap.alloc_decimal(x * y));
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) * heap.to_f64_val(b)))
}

#[inline(always)]
pub(crate) fn div(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    // `int / int` is float division by contract (varn_core::numeric), so there
    // is deliberately no integer fast path returning an int here — only the
    // decimal detour to skip.
    if a.is_heap() || b.is_heap() {
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            if y.is_zero() {
                return Err(RuntimeError::new("division by zero"));
            }
            return Ok(heap.alloc_decimal(x / y));
        }
    }
    let bv = heap.to_f64_val(b);
    if bv == 0.0 {
        return Err(RuntimeError::new("division by zero"));
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) / bv))
}

#[inline(always)]
pub(crate) fn modulo(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let bi = heap.as_int(b);
        if bi == 0 {
            return Err(RuntimeError::new("modulo by zero"));
        }
        let r = heap.as_int(a) % bi;
        return Ok(heap.make_int(r));
    }
    if a.is_heap() || b.is_heap() {
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            // The zero guard the decimal arms used to lack, while `div`'s had
            // it. `Decimal % 0` panics inside rust_decimal, so the omission
            // turned a Varn-level error into a process abort.
            if y.is_zero() {
                return Err(RuntimeError::new("modulo by zero"));
            }
            return Ok(heap.alloc_decimal(x % y));
        }
    }
    let bv = heap.to_f64_val(b);
    if bv == 0.0 {
        return Err(RuntimeError::new("modulo by zero"));
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) % bv))
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
        return match pow_i48(base, e) {
            Some(r) => Ok(VmValue::from_int(r)),
            None => Err(overflow("**", base, exp)),
        };
    }
    Ok(VmValue::from_f64(
        heap.to_f64_val(a).powf(heap.to_f64_val(b)),
    ))
}

#[inline(always)]
pub(crate) fn negate(a: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) {
        let x = heap.as_int(a);
        return match neg_i48(x) {
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
