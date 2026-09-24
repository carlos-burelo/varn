use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

#[inline(always)]
pub(crate) fn eq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if heap.is_int(a) && heap.is_int(b) {
        return heap.as_int(a) == heap.as_int(b);
    }
    if a.is_f64() && b.is_f64() {
        return a.as_f64() == b.as_f64();
    }
    if a.is_bool() && b.is_bool() {
        return a.as_bool() == b.as_bool();
    }
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    if heap.is_int(a) && b.is_f64() {
        return (heap.as_int(a) as f64) == b.as_f64();
    }
    if a.is_f64() && heap.is_int(b) {
        return a.as_f64() == (heap.as_int(b) as f64);
    }

    if a.is_sso() && b.is_sso() {
        return a.bits_eq(b);
    }

    if a.is_sso() && b.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(b.as_heap_idx()) {
            if s.byte_len() != a.sso_len() {
                return false;
            }
            return a.sso_eq_bytes(s.as_str().as_bytes());
        }
        return false;
    }
    if b.is_sso() && a.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(a.as_heap_idx()) {
            if s.byte_len() != b.sso_len() {
                return false;
            }
            return b.sso_eq_bytes(s.as_str().as_bytes());
        }
        return false;
    }
    if a.is_heap() && b.is_heap() {
        let ai = a.as_heap_idx();
        let bi = b.as_heap_idx();
        if ai == bi {
            return true;
        }
        match (heap.get(ai), heap.get(bi)) {
            (Some(HeapObj::Str(sa)), Some(HeapObj::Str(sb))) => {
                if sa.byte_len() != sb.byte_len() {
                    return false;
                }
                return sa.as_str() == sb.as_str();
            }
            (Some(HeapObj::Char(ca)), Some(HeapObj::Char(cb))) => return ca == cb,
            (Some(HeapObj::BigInt(a)), Some(HeapObj::BigInt(b))) => return a == b,
            (Some(HeapObj::Decimal(da)), Some(HeapObj::Decimal(db))) => return da == db,
            (Some(HeapObj::EnumVariant(ea)), Some(HeapObj::EnumVariant(eb))) => {
                return variant_eq(ea, eb, heap);
            }
            (Some(HeapObj::Array(_)), Some(HeapObj::Array(_))) => return false,
            (Some(HeapObj::Object(_)), Some(HeapObj::Object(_))) => return false,
            (Some(HeapObj::Tuple(arr_a)), Some(HeapObj::Tuple(arr_b))) => {
                if arr_a.len() != arr_b.len() {
                    return false;
                }
                for i in 0..arr_a.len() {
                    let va = arr_a.get_vm(i).unwrap_or(VmValue::null());
                    let vb = arr_b.get_vm(i).unwrap_or(VmValue::null());
                    if !member_eq(va, vb, heap) {
                        return false;
                    }
                }
                return true;
            }
            (Some(HeapObj::Record(obj_a)), Some(HeapObj::Record(obj_b))) => {
                let keys_a: Vec<_> = obj_a.keys().collect();
                let keys_b: Vec<_> = obj_b.keys().collect();
                if keys_a.len() != keys_b.len() {
                    return false;
                }
                for k in &keys_a {
                    let k_str = k.as_ref();
                    let (Some(va), Some(vb)) = (obj_a.get(k_str), obj_b.get(k_str)) else {
                        return false;
                    };
                    if !member_eq(va, vb, heap) {
                        return false;
                    }
                }
                return true;
            }
            _ => return false,
        }
    }

    if a.is_heap() && heap.is_int(b) {
        match heap.get(a.as_heap_idx()) {
            Some(HeapObj::BigInt(av)) => return **av == num_bigint::BigInt::from(heap.as_int(b)),
            Some(HeapObj::Decimal(da)) => {
                return **da == bigdecimal::BigDecimal::from(heap.as_int(b))
            }
            Some(HeapObj::EnumVariant(ev)) => return ev.variant_tag == heap.as_int(b),
            _ => return false,
        }
    }
    if heap.is_int(a) && b.is_heap() {
        match heap.get(b.as_heap_idx()) {
            Some(HeapObj::BigInt(bv)) => return **bv == num_bigint::BigInt::from(heap.as_int(a)),
            Some(HeapObj::Decimal(db)) => {
                return bigdecimal::BigDecimal::from(heap.as_int(a)) == **db
            }
            Some(HeapObj::EnumVariant(ev)) => return ev.variant_tag == heap.as_int(a),
            _ => return false,
        }
    }
    false
}

/// Equality of a member of a value type (a record field, a tuple element,
/// a variant payload): deep, so a nested array compares by its elements.
/// A mutable array at top level is still compared by identity.
fn member_eq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if a.is_heap() && b.is_heap() {
        if let (Some(HeapObj::Array(x)), Some(HeapObj::Array(y))) =
            (heap.get(a.as_heap_idx()), heap.get(b.as_heap_idx()))
        {
            return x.len() == y.len()
                && (0..x.len()).all(|i| {
                    member_eq(
                        x.get_vm(i).unwrap_or(VmValue::null()),
                        y.get_vm(i).unwrap_or(VmValue::null()),
                        heap,
                    )
                });
        }
    }
    eq(a, b, heap)
}

/// Two enum values are equal when they are the same variant of the same enum
/// with equal payloads.
fn variant_eq(
    a: &varn_types::value::EnumVariantData,
    b: &varn_types::value::EnumVariantData,
    heap: &Heap,
) -> bool {
    a.enum_name == b.enum_name && a.variant_tag == b.variant_tag && payload_eq(&a.payload, &b.payload, heap)
}

fn payload_eq(a: &varn_types::Value, b: &varn_types::Value, heap: &Heap) -> bool {
    use varn_types::Value;
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            let (x, y) = (x.read(), y.read());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| payload_eq(p, q, heap))
        }
        (Value::Object(x), Value::Object(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            let keys: Vec<_> = x.keys().collect();
            keys.len() == y.keys().count()
                && keys.iter().all(|k| match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => member_eq(p, q, heap),
                    _ => false,
                })
        }
        (Value::EnumVariant(x), Value::EnumVariant(y)) => variant_eq(x, y, heap),
        _ => a == b,
    }
}

#[inline(always)]
pub(crate) fn neq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    !eq(a, b, heap)
}

pub(crate) fn lt_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if heap.is_int(a) && heap.is_int(b) {
        return heap.as_int(a) < heap.as_int(b);
    }
    if a.is_f64() && b.is_f64() {
        return a.as_f64() < b.as_f64();
    }
    if heap.is_int(a) && b.is_f64() {
        return (heap.as_int(a) as f64) < b.as_f64();
    }
    if a.is_f64() && heap.is_int(b) {
        return a.as_f64() < (heap.as_int(b) as f64);
    }
    if a.is_heap() && b.is_heap() {
        let ai = a.as_heap_idx();
        let bi = b.as_heap_idx();
        match (heap.get(ai), heap.get(bi)) {
            (Some(HeapObj::BigInt(ba)), Some(HeapObj::BigInt(bb))) => return ba < bb,
            (Some(HeapObj::Decimal(da)), Some(HeapObj::Decimal(db))) => return da < db,
            _ => {}
        }
    }
    if a.is_heap() && heap.is_int(b) {
        if let Some(HeapObj::Decimal(da)) = heap.get(a.as_heap_idx()) {
            return **da < bigdecimal::BigDecimal::from(heap.as_int(b));
        }
    }
    if heap.is_int(a) && b.is_heap() {
        if let Some(HeapObj::Decimal(db)) = heap.get(b.as_heap_idx()) {
            return bigdecimal::BigDecimal::from(heap.as_int(a)) < **db;
        }
    }
    heap.to_f64_val(a) < heap.to_f64_val(b)
}

pub(crate) fn lte_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if heap.is_int(a) && heap.is_int(b) {
        return heap.as_int(a) <= heap.as_int(b);
    }
    !gt_heap(a, b, heap)
}

pub(crate) fn gt_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    lt_heap(b, a, heap)
}

pub(crate) fn gte_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if heap.is_int(a) && heap.is_int(b) {
        return heap.as_int(a) >= heap.as_int(b);
    }
    !lt_heap(a, b, heap)
}

#[inline(always)]
pub(crate) fn logical_not(a: VmValue) -> VmValue {
    VmValue::from_bool(!a.is_truthy())
}
