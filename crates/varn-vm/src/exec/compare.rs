use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_types::Value;

#[inline(always)]
pub fn eq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if a.is_int() && b.is_int() {
        return a.as_i32() == b.as_i32();
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
    if a.is_int() && b.is_f64() {
        return (a.as_i32() as f64) == b.as_f64();
    }
    if a.is_f64() && b.is_int() {
        return a.as_f64() == (b.as_i32() as f64);
    }

    if a.is_sso() && b.is_sso() {
        return a.0 == b.0;
    }

    if a.is_sso() && b.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(b.as_heap_idx()) {
            let mut buf = [0u8; 5];
            let s2 = a.sso_as_str(&mut buf);
            return s.as_ref() == s2;
        }
        return false;
    }
    if b.is_sso() && a.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(a.as_heap_idx()) {
            let mut buf = [0u8; 5];
            let s2 = b.sso_as_str(&mut buf);
            return s.as_ref() == s2;
        }
        return false;
    }
    if a.is_heap() || b.is_heap() {
        let av = heap.extract_val(a);
        let bv = heap.extract_val(b);
        match (&av, &bv) {
            (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => return **da == **db,
            (Ok(Value::Decimal(da)), _) if b.is_int() => {
                return **da == rust_decimal::Decimal::from(b.as_i32())
            }
            (_, Ok(Value::Decimal(db))) if a.is_int() => {
                return rust_decimal::Decimal::from(a.as_i32()) == **db
            }
            _ => {}
        }
        if a.is_heap() && b.is_heap() {
            let ai = a.as_heap_idx();
            let bi = b.as_heap_idx();
            if ai == bi {
                return true;
            }
            match (heap.get(ai), heap.get(bi)) {
                (Some(HeapObj::Str(sa)), Some(HeapObj::Str(sb))) => return sa == sb,
                (Some(HeapObj::Char(ca)), Some(HeapObj::Char(cb))) => return ca == cb,
                (Some(HeapObj::BigInt(a)), Some(HeapObj::BigInt(b))) => return a == b,
                (Some(HeapObj::EnumVariant(ea)), Some(HeapObj::EnumVariant(eb))) => {
                    return ea.variant_tag == eb.variant_tag;
                }
                _ => return false,
            }
        }

        if a.is_int() && b.is_heap() {
            match heap.get(b.as_heap_idx()) {
                Some(HeapObj::BigInt(bv)) => return *bv == a.as_i32() as i128,
                Some(HeapObj::EnumVariant(ev)) => return ev.variant_tag as i32 == a.as_i32(),
                _ => {}
            }
        }
        if a.is_heap() && b.is_int() {
            match heap.get(a.as_heap_idx()) {
                Some(HeapObj::BigInt(av)) => return *av == b.as_i32() as i128,
                Some(HeapObj::EnumVariant(ev)) => return ev.variant_tag as i32 == b.as_i32(),
                _ => {}
            }
        }
    }
    false
}

#[inline(always)]
pub fn neq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    !eq(a, b, heap)
}

#[inline(always)]
pub fn lt(a: VmValue, b: VmValue) -> bool {
    if a.is_int() && b.is_int() {
        return a.as_i32() < b.as_i32();
    }
    a.to_f64() < b.to_f64()
}

#[inline(always)]
pub fn lte(a: VmValue, b: VmValue) -> bool {
    if a.is_int() && b.is_int() {
        return a.as_i32() <= b.as_i32();
    }
    a.to_f64() <= b.to_f64()
}

#[inline(always)]
pub fn gt(a: VmValue, b: VmValue) -> bool {
    lt(b, a)
}

#[inline(always)]
pub fn gte(a: VmValue, b: VmValue) -> bool {
    lte(b, a)
}

pub fn lt_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if a.is_int() && b.is_int() {
        return a.as_i32() < b.as_i32();
    }
    if a.is_f64() && b.is_f64() {
        return a.as_f64() < b.as_f64();
    }
    if a.is_int() && b.is_f64() {
        return (a.as_i32() as f64) < b.as_f64();
    }
    if a.is_f64() && b.is_int() {
        return a.as_f64() < (b.as_i32() as f64);
    }
    if a.is_heap() || b.is_heap() {
        let av = heap.extract_val(a);
        let bv = heap.extract_val(b);
        match (&av, &bv) {
            (Ok(Value::Decimal(da)), Ok(Value::Decimal(db))) => return **da < **db,
            (Ok(Value::Decimal(da)), _) if b.is_int() => {
                return **da < rust_decimal::Decimal::from(b.as_i32())
            }
            (_, Ok(Value::Decimal(db))) if a.is_int() => {
                return rust_decimal::Decimal::from(a.as_i32()) < **db
            }
            _ if a.is_heap() && b.is_heap() => {
                if let (Some(HeapObj::BigInt(ba)), Some(HeapObj::BigInt(bb))) =
                    (heap.get(a.as_heap_idx()), heap.get(b.as_heap_idx()))
                {
                    return ba < bb;
                }
            }
            _ => {}
        }
    }
    a.to_f64() < b.to_f64()
}

pub fn lte_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if a.is_int() && b.is_int() {
        return a.as_i32() <= b.as_i32();
    }
    !gt_heap(a, b, heap)
}

pub fn gt_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    lt_heap(b, a, heap)
}

pub fn gte_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if a.is_int() && b.is_int() {
        return a.as_i32() >= b.as_i32();
    }
    !lt_heap(a, b, heap)
}

#[inline(always)]
pub fn logical_not(a: VmValue) -> VmValue {
    VmValue::from_bool(!a.is_truthy())
}

#[inline(always)]
pub fn eq_i32(a: VmValue, b: VmValue) -> bool {
    a.as_i32() == b.as_i32()
}
#[inline(always)]
pub fn neq_i32(a: VmValue, b: VmValue) -> bool {
    a.as_i32() != b.as_i32()
}
#[inline(always)]
pub fn lt_i32(a: VmValue, b: VmValue) -> bool {
    a.as_i32() < b.as_i32()
}
#[inline(always)]
pub fn lte_i32(a: VmValue, b: VmValue) -> bool {
    a.as_i32() <= b.as_i32()
}
#[inline(always)]
pub fn gt_i32(a: VmValue, b: VmValue) -> bool {
    a.as_i32() > b.as_i32()
}
#[inline(always)]
pub fn gte_i32(a: VmValue, b: VmValue) -> bool {
    a.as_i32() >= b.as_i32()
}

#[inline(always)]
pub fn eq_f64(a: VmValue, b: VmValue) -> bool {
    a.to_f64() == b.to_f64()
}
#[inline(always)]
pub fn neq_f64(a: VmValue, b: VmValue) -> bool {
    a.to_f64() != b.to_f64()
}
#[inline(always)]
pub fn lt_f64(a: VmValue, b: VmValue) -> bool {
    a.to_f64() < b.to_f64()
}
#[inline(always)]
pub fn lte_f64(a: VmValue, b: VmValue) -> bool {
    a.to_f64() <= b.to_f64()
}
#[inline(always)]
pub fn gt_f64(a: VmValue, b: VmValue) -> bool {
    a.to_f64() > b.to_f64()
}
#[inline(always)]
pub fn gte_f64(a: VmValue, b: VmValue) -> bool {
    a.to_f64() >= b.to_f64()
}
