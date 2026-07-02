use crate::error::{RuntimeError, VmResult};
use crate::heap::Heap;
use crate::value::VmValue;
use varn_types::Value;

#[inline(always)]
pub fn add(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if a.is_int() && b.is_int() {
        // Integer arithmetic wraps at 48 bits (varn_core::numeric);
        // `from_int` masks the payload, keeping all tiers bit-identical.
        return Ok(VmValue::from_int(a.as_int().wrapping_add(b.as_int())));
    }
    if a.is_f64() || b.is_f64() {
        return Ok(VmValue::from_f64(a.to_f64() + b.to_f64()));
    }

    // Strings before anything that calls `extract_val`: extraction deep-copies
    // extensible string views (O(len)), and concatenation must never enter the
    // content interner (the Inv6 pathology). `str_concat` handles SSO inputs
    // and the extensible-buffer accumulation fast path.
    if a.is_sso() || b.is_sso() {
        return Ok(crate::exec::strings::str_concat(a, b, heap));
    }
    if a.is_heap() || b.is_heap() {
        let a_is_str = a.is_heap()
            && matches!(
                heap.get(a.as_heap_idx()),
                Some(crate::heap::HeapObj::Str(_))
            );
        let b_is_str = b.is_heap()
            && matches!(
                heap.get(b.as_heap_idx()),
                Some(crate::heap::HeapObj::Str(_))
            );
        if a_is_str || b_is_str {
            return Ok(crate::exec::strings::str_concat(a, b, heap));
        }

        let av = heap.extract_val(a);
        let bv = heap.extract_val(b);
        match (&av, &bv) {
            (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
                return Ok(heap.alloc_decimal(**da + **db))
            }
            (Ok(Value::Decimal(da)), _) if b.is_int() => {
                return Ok(heap.alloc_decimal(**da + rust_decimal::Decimal::from(b.as_i32())))
            }
            (_, Ok(Value::Decimal(db))) if a.is_int() => {
                return Ok(heap.alloc_decimal(rust_decimal::Decimal::from(a.as_i32()) + **db))
            }
            _ => {}
        }
    }
    Ok(VmValue::from_f64(a.to_f64() + b.to_f64()))
}

#[inline(always)]
pub fn add_i32(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int().wrapping_add(b.as_int()))
}

#[inline(always)]
pub fn add_f64(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_f64(a.to_f64() + b.to_f64())
}

#[inline(always)]
pub fn sub(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if a.is_int() && b.is_int() {
        return Ok(VmValue::from_int(a.as_int().wrapping_sub(b.as_int())));
    }
    let av = heap.extract_val(a);
    let bv = heap.extract_val(b);
    match (&av, &bv) {
        (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
            return Ok(heap.alloc_decimal(**da - **db))
        }
        (Ok(Value::Decimal(da)), _) if b.is_int() => {
            return Ok(heap.alloc_decimal(**da - rust_decimal::Decimal::from(b.as_i32())))
        }
        (_, Ok(Value::Decimal(db))) if a.is_int() => {
            return Ok(heap.alloc_decimal(rust_decimal::Decimal::from(a.as_i32()) - **db))
        }
        _ => {}
    }
    Ok(VmValue::from_f64(a.to_f64() - b.to_f64()))
}

#[inline(always)]
pub fn sub_i32(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int().wrapping_sub(b.as_int()))
}

#[inline(always)]
pub fn sub_f64(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_f64(a.to_f64() - b.to_f64())
}

#[inline(always)]
pub fn mul(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if a.is_int() && b.is_int() {
        return Ok(VmValue::from_int(a.as_int().wrapping_mul(b.as_int())));
    }
    let av = heap.extract_val(a);
    let bv = heap.extract_val(b);
    match (&av, &bv) {
        (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
            return Ok(heap.alloc_decimal(**da * **db))
        }
        (Ok(Value::Decimal(da)), _) if b.is_int() => {
            return Ok(heap.alloc_decimal(**da * rust_decimal::Decimal::from(b.as_i32())))
        }
        (_, Ok(Value::Decimal(db))) if a.is_int() => {
            return Ok(heap.alloc_decimal(rust_decimal::Decimal::from(a.as_i32()) * **db))
        }
        _ => {}
    }
    Ok(VmValue::from_f64(a.to_f64() * b.to_f64()))
}

#[inline(always)]
pub fn mul_i32(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int().wrapping_mul(b.as_int()))
}

#[inline(always)]
pub fn mul_f64(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_f64(a.to_f64() * b.to_f64())
}

#[inline(always)]
pub fn div(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    {
        let av = heap.extract_val(a);
        let bv = heap.extract_val(b);
        match (&av, &bv) {
            (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
                if **db == 0.into() {
                    return Err(RuntimeError::new("division by zero"));
                }
                return Ok(heap.alloc_decimal(**da / **db));
            }
            (Ok(Value::Decimal(da)), _) if b.is_int() => {
                let di = rust_decimal::Decimal::from(b.as_i32());
                if di == 0.into() {
                    return Err(RuntimeError::new("division by zero"));
                }
                return Ok(heap.alloc_decimal(**da / di));
            }
            (_, Ok(Value::Decimal(db))) if a.is_int() => {
                if **db == 0.into() {
                    return Err(RuntimeError::new("division by zero"));
                }
                return Ok(heap.alloc_decimal(rust_decimal::Decimal::from(a.as_i32()) / **db));
            }
            _ => {}
        }
    }
    let bv = b.to_f64();
    if bv == 0.0 {
        return Err(RuntimeError::new("division by zero"));
    }
    // `int / int` always yields float (varn_core::numeric).
    Ok(VmValue::from_f64(a.to_f64() / bv))
}

#[inline(always)]
pub fn div_i32(a: VmValue, b: VmValue) -> VmResult<VmValue> {
    let bv = b.to_i32() as i64;
    if bv == 0 {
        return Err(RuntimeError::new("division by zero"));
    }
    Ok(VmValue::from_int(a.as_int().wrapping_div(bv)))
}

#[inline(always)]
pub fn div_f64(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_f64(a.to_f64() / b.to_f64())
}

#[inline(always)]
pub fn modulo(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    {
        let av = heap.extract_val(a);
        let bv = heap.extract_val(b);
        match (&av, &bv) {
            (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
                return Ok(heap.alloc_decimal(**da % **db))
            }
            (Ok(Value::Decimal(da)), _) if b.is_int() => {
                return Ok(heap.alloc_decimal(**da % rust_decimal::Decimal::from(b.as_i32())))
            }
            (_, Ok(Value::Decimal(db))) if a.is_int() => {
                return Ok(heap.alloc_decimal(rust_decimal::Decimal::from(a.as_i32()) % **db))
            }
            _ => {}
        }
    }
    let bv = b.to_f64();
    if bv == 0.0 {
        return Err(RuntimeError::new("modulo by zero"));
    }
    if a.is_int() && b.is_int() {
        let bi = b.as_int();
        if bi == 0 {
            return Err(RuntimeError::new("modulo by zero"));
        }
        return Ok(VmValue::from_int(a.as_int() % bi));
    }
    Ok(VmValue::from_f64(a.to_f64() % bv))
}

#[inline(always)]
pub fn pow(a: VmValue, b: VmValue) -> VmResult<VmValue> {
    if a.is_int() && b.is_int() {
        // `int ** int` stays integral and wraps at 48 bits; a negative
        // exponent raises instead of silently producing a float
        // (varn_core::numeric).
        let exp = b.as_int();
        if exp < 0 {
            return Err(RuntimeError::new("negative exponent in integer power"));
        }
        let e = u32::try_from(exp).unwrap_or(u32::MAX);
        return Ok(VmValue::from_int(a.as_int().wrapping_pow(e)));
    }
    Ok(VmValue::from_f64(a.to_f64().powf(b.to_f64())))
}

#[inline(always)]
pub fn negate(a: VmValue, heap: &mut Heap) -> VmValue {
    if a.is_int() {
        return VmValue::from_int(-a.as_int());
    }
    if a.is_f64() {
        return VmValue::from_f64(-a.as_f64());
    }
    if let Ok(Value::Decimal(d)) = heap.extract_val(a) {
        return heap.alloc_decimal(-*d);
    }
    VmValue::from_f64(-a.to_f64())
}

#[inline(always)]
pub fn bit_and(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int() & b.as_int())
}

#[inline(always)]
pub fn bit_or(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int() | b.as_int())
}

#[inline(always)]
pub fn bit_xor(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int() ^ b.as_int())
}

#[inline(always)]
pub fn shl(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int().wrapping_shl(b.to_i32() as u32 & 63))
}

#[inline(always)]
pub fn shr(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(a.as_int().wrapping_shr(b.to_i32() as u32 & 63))
}

#[inline(always)]
pub fn ushr(a: VmValue, b: VmValue) -> VmValue {
    VmValue::from_int(((a.as_int() as u64).wrapping_shr(b.to_i32() as u32 & 63)) as i64)
}
