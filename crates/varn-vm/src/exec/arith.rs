use crate::error::{RuntimeError, VmResult};
use crate::heap::Heap;
use crate::value::VmValue;
use varn_types::Value;

#[inline(always)]
pub fn add(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let r = heap.as_int(a).wrapping_add(heap.as_int(b));
        return Ok(heap.make_int(r));
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
            (Ok(Value::Decimal(da)), _) if heap.is_int(b) => {
                let bi = rust_decimal::Decimal::from(heap.as_int(b));
                return Ok(heap.alloc_decimal(**da + bi))
            }
            (_, Ok(Value::Decimal(db))) if heap.is_int(a) => {
                let ai = rust_decimal::Decimal::from(heap.as_int(a));
                return Ok(heap.alloc_decimal(ai + **db))
            }
            _ => {}
        }
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) + heap.to_f64_val(b)))
}

#[inline(always)]
pub fn add_i32(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a).wrapping_add(heap.as_int(b));
    heap.make_int(r)
}

#[inline(always)]
pub fn add_f64(a: VmValue, b: VmValue, heap: &Heap) -> VmValue {
    VmValue::from_f64(heap.to_f64_val(a) + heap.to_f64_val(b))
}

#[inline(always)]
pub fn sub(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let r = heap.as_int(a).wrapping_sub(heap.as_int(b));
        return Ok(heap.make_int(r));
    }
    let av = heap.extract_val(a);
    let bv = heap.extract_val(b);
    match (&av, &bv) {
        (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
            return Ok(heap.alloc_decimal(**da - **db))
        }
        (Ok(Value::Decimal(da)), _) if heap.is_int(b) => {
            let bi = rust_decimal::Decimal::from(heap.as_int(b));
            return Ok(heap.alloc_decimal(**da - bi))
        }
        (_, Ok(Value::Decimal(db))) if heap.is_int(a) => {
            let ai = rust_decimal::Decimal::from(heap.as_int(a));
            return Ok(heap.alloc_decimal(ai - **db))
        }
        _ => {}
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) - heap.to_f64_val(b)))
}

#[inline(always)]
pub fn sub_i32(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a).wrapping_sub(heap.as_int(b));
    heap.make_int(r)
}

#[inline(always)]
pub fn sub_f64(a: VmValue, b: VmValue, heap: &Heap) -> VmValue {
    VmValue::from_f64(heap.to_f64_val(a) - heap.to_f64_val(b))
}

#[inline(always)]
pub fn mul(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let r = heap.as_int(a).wrapping_mul(heap.as_int(b));
        return Ok(heap.make_int(r));
    }
    let av = heap.extract_val(a);
    let bv = heap.extract_val(b);
    match (&av, &bv) {
        (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => {
            return Ok(heap.alloc_decimal(**da * **db))
        }
        (Ok(Value::Decimal(da)), _) if heap.is_int(b) => {
            let bi = rust_decimal::Decimal::from(heap.as_int(b));
            return Ok(heap.alloc_decimal(**da * bi))
        }
        (_, Ok(Value::Decimal(db))) if heap.is_int(a) => {
            let ai = rust_decimal::Decimal::from(heap.as_int(a));
            return Ok(heap.alloc_decimal(ai * **db))
        }
        _ => {}
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) * heap.to_f64_val(b)))
}

#[inline(always)]
pub fn mul_i32(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a).wrapping_mul(heap.as_int(b));
    heap.make_int(r)
}

#[inline(always)]
pub fn mul_f64(a: VmValue, b: VmValue, heap: &Heap) -> VmValue {
    VmValue::from_f64(heap.to_f64_val(a) * heap.to_f64_val(b))
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
            (Ok(Value::Decimal(da)), _) if heap.is_int(b) => {
                let di = rust_decimal::Decimal::from(heap.as_int(b));
                if di == 0.into() {
                    return Err(RuntimeError::new("division by zero"));
                }
                return Ok(heap.alloc_decimal(**da / di));
            }
            (_, Ok(Value::Decimal(db))) if heap.is_int(a) => {
                if **db == 0.into() {
                    return Err(RuntimeError::new("division by zero"));
                }
                let ai = rust_decimal::Decimal::from(heap.as_int(a));
                return Ok(heap.alloc_decimal(ai / **db));
            }
            _ => {}
        }
    }
    let bv = heap.to_f64_val(b);
    if bv == 0.0 {
        return Err(RuntimeError::new("division by zero"));
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) / bv))
}

#[inline(always)]
pub fn div_i32(a: VmValue, b: VmValue, heap: &Heap) -> VmResult<VmValue> {
    let bv = heap.as_int(b);
    if bv == 0 {
        return Err(RuntimeError::new("division by zero"));
    }
    Ok(heap.extract(a).as_int().map(|av| VmValue::from_f64(av as f64 / bv as f64)).unwrap_or_else(VmValue::null))
}

#[inline(always)]
pub fn div_f64(a: VmValue, b: VmValue, heap: &Heap) -> VmValue {
    VmValue::from_f64(heap.to_f64_val(a) / heap.to_f64_val(b))
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
            (Ok(Value::Decimal(da)), _) if heap.is_int(b) => {
                let bi = rust_decimal::Decimal::from(heap.as_int(b));
                return Ok(heap.alloc_decimal(**da % bi))
            }
            (_, Ok(Value::Decimal(db))) if heap.is_int(a) => {
                let ai = rust_decimal::Decimal::from(heap.as_int(a));
                return Ok(heap.alloc_decimal(ai % **db))
            }
            _ => {}
        }
    }
    let bv = heap.to_f64_val(b);
    if bv == 0.0 {
        return Err(RuntimeError::new("modulo by zero"));
    }
    if heap.is_int(a) && heap.is_int(b) {
        let bi = heap.as_int(b);
        if bi == 0 {
            return Err(RuntimeError::new("modulo by zero"));
        }
        let r = heap.as_int(a) % bi;
        return Ok(heap.make_int(r));
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) % bv))
}

#[inline(always)]
pub fn pow(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let exp = heap.as_int(b);
        if exp < 0 {
            return Err(RuntimeError::new("negative exponent in integer power"));
        }
        let e = u32::try_from(exp).unwrap_or(u32::MAX);
        let r = heap.as_int(a).wrapping_pow(e);
        return Ok(heap.make_int(r));
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a).powf(heap.to_f64_val(b))))
}

#[inline(always)]
pub fn negate(a: VmValue, heap: &mut Heap) -> VmValue {
    if heap.is_int(a) {
        let r = -heap.as_int(a);
        return heap.make_int(r);
    }
    if a.is_f64() {
        return VmValue::from_f64(-a.as_f64());
    }
    if let Ok(Value::Decimal(d)) = heap.extract_val(a) {
        return heap.alloc_decimal(-*d);
    }
    VmValue::from_f64(-heap.to_f64_val(a))
}

#[inline(always)]
pub fn bit_and(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a) & heap.as_int(b);
    heap.make_int(r)
}

#[inline(always)]
pub fn bit_or(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a) | heap.as_int(b);
    heap.make_int(r)
}

#[inline(always)]
pub fn bit_xor(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a) ^ heap.as_int(b);
    heap.make_int(r)
}

#[inline(always)]
pub fn shl(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a).wrapping_shl(heap.to_f64_val(b) as i32 as u32 & 63);
    heap.make_int(r)
}

#[inline(always)]
pub fn shr(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = heap.as_int(a).wrapping_shr(heap.to_f64_val(b) as i32 as u32 & 63);
    heap.make_int(r)
}

#[inline(always)]
pub fn ushr(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let r = ((heap.as_int(a) as u64).wrapping_shr(heap.to_f64_val(b) as i32 as u32 & 63)) as i64;
    heap.make_int(r)
}
