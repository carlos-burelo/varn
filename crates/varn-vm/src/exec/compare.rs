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
        if let Some(HeapObj::Str(s)) = heap.get(b.as_heap()) {
            if s.byte_len() != a.sso_len() {
                return false;
            }
            return a.sso_eq_bytes(s.as_str().as_bytes());
        }
        return false;
    }
    if b.is_sso() && a.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(a.as_heap()) {
            if s.byte_len() != b.sso_len() {
                return false;
            }
            return b.sso_eq_bytes(s.as_str().as_bytes());
        }
        return false;
    }
    if a.is_heap() && b.is_heap() {
        let ai = a.as_heap();
        let bi = b.as_heap();
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
            (Some(HeapObj::EnumVariant(_)), Some(HeapObj::EnumVariant(_))) => {
                return composite_eq(a, b, heap);
            }
            (Some(HeapObj::Array(_)), Some(HeapObj::Array(_))) => return false,
            (Some(HeapObj::Object(_)), Some(HeapObj::Object(_))) => return false,
            (Some(HeapObj::Tuple(_)), Some(HeapObj::Tuple(_)))
            | (Some(HeapObj::Record(_)), Some(HeapObj::Record(_))) => {
                return composite_eq(a, b, heap);
            }
            _ => return false,
        }
    }

    if a.is_heap() && heap.is_int(b) {
        match heap.get(a.as_heap()) {
            Some(HeapObj::BigInt(av)) => {
                let rhs = num_bigint::BigInt::from(heap.as_int(b));
                return **av == rhs;
            }
            Some(HeapObj::Decimal(da)) => {
                let rhs = bigdecimal::BigDecimal::from(heap.as_int(b));
                return **da == rhs;
            }
            Some(
                HeapObj::EnumVariant(_)
                | HeapObj::Str(_)
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
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => return false,
        }
    }
    if heap.is_int(a) && b.is_heap() {
        match heap.get(b.as_heap()) {
            Some(HeapObj::BigInt(bv)) => {
                let lhs = num_bigint::BigInt::from(heap.as_int(a));
                return **bv == lhs;
            }
            Some(HeapObj::Decimal(db)) => {
                let lhs = bigdecimal::BigDecimal::from(heap.as_int(a));
                return lhs == **db;
            }
            Some(HeapObj::EnumVariant(_)) => return false,
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
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => return false,
        }
    }
    false
}

fn composite_eq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    let mut pending = vec![(a, b, false)];
    while let Some((x, y, member)) = pending.pop() {
        match expand(x, y, member, heap, &mut pending) {
            Some(true) => {}
            Some(false) => return false,
            None => {
                if !eq(x, y, heap) {
                    return false;
                }
            }
        }
    }
    true
}

fn expand(
    a: VmValue,
    b: VmValue,
    member: bool,
    heap: &Heap,
    pending: &mut Vec<(VmValue, VmValue, bool)>,
) -> Option<bool> {
    if !(a.is_heap() && b.is_heap()) {
        return None;
    }
    let null = VmValue::null();
    match (heap.get(a.as_heap())?, heap.get(b.as_heap())?) {
        (HeapObj::Tuple(x), HeapObj::Tuple(y)) => Some(push_elements(x, y, pending, null)),
        (HeapObj::Array(x), HeapObj::Array(y)) if member => {
            Some(push_elements(x, y, pending, null))
        }
        (HeapObj::Record(x), HeapObj::Record(y)) => {
            let keys: Vec<_> = x.keys().collect();
            if keys.len() != y.keys().count() {
                return Some(false);
            }
            for key in &keys {
                let (Some(va), Some(vb)) = (x.get(key.as_ref()), y.get(key.as_ref())) else {
                    return Some(false);
                };
                pending.push((va, vb, true));
            }
            Some(true)
        }
        (HeapObj::EnumVariant(x), HeapObj::EnumVariant(y)) => {
            let same_enum = match (x.enum_class_id, y.enum_class_id) {
                (Some(p), Some(q)) => p == q,
                _ => x.def.enum_name == y.def.enum_name,
            };
            if !(same_enum && x.tag() == y.tag() && x.slots.len() == y.slots.len()) {
                return Some(false);
            }
            pending.extend(
                x.slots
                    .iter()
                    .zip(y.slots.iter())
                    .map(|(p, q)| (*p, *q, true)),
            );
            Some(true)
        }
        _ => None,
    }
}

fn push_elements(
    x: &varn_types::VmArray,
    y: &varn_types::VmArray,
    pending: &mut Vec<(VmValue, VmValue, bool)>,
    null: VmValue,
) -> bool {
    if x.len() != y.len() {
        return false;
    }
    for i in 0..x.len() {
        pending.push((
            x.get_vm(i).unwrap_or(null),
            y.get_vm(i).unwrap_or(null),
            true,
        ));
    }
    true
}

#[inline(always)]
pub(crate) fn neq(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    !eq(a, b, heap)
}

fn text_bytes<'a>(
    v: VmValue,
    heap: &'a Heap,
    buf: &'a mut [u8; varn_types::vm_value::SSO_MAX_LEN],
) -> Option<&'a [u8]> {
    if v.is_sso() {
        return Some(v.sso_as_str(buf).as_bytes());
    }
    if !v.is_heap() {
        return None;
    }
    match heap.get(v.as_heap()) {
        Some(HeapObj::Str(s)) => Some(s.as_str().as_bytes()),
        Some(HeapObj::Char(c)) => {
            let (head, _) = buf.split_at_mut(4);
            Some(c.encode_utf8(head).as_bytes())
        }
        Some(
            HeapObj::Array(_)
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
            | HeapObj::Generator(_)
            | HeapObj::Spread(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::EnumVariant(_),
        )
        | None => None,
    }
}

fn text_order(a: VmValue, b: VmValue, heap: &Heap) -> Option<std::cmp::Ordering> {
    let (mut left, mut right) = (
        [0u8; varn_types::vm_value::SSO_MAX_LEN],
        [0u8; varn_types::vm_value::SSO_MAX_LEN],
    );
    let left = text_bytes(a, heap, &mut left)?;
    let right = text_bytes(b, heap, &mut right)?;
    Some(left.cmp(right))
}

pub(crate) fn lt_heap(a: VmValue, b: VmValue, heap: &Heap) -> bool {
    if heap.is_int(a) && heap.is_int(b) {
        return heap.as_int(a) < heap.as_int(b);
    }
    if let Some(order) = text_order(a, b, heap) {
        return order == std::cmp::Ordering::Less;
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
        let ai = a.as_heap();
        let bi = b.as_heap();
        match (heap.get(ai), heap.get(bi)) {
            (Some(HeapObj::BigInt(ba)), Some(HeapObj::BigInt(bb))) => return ba < bb,
            (Some(HeapObj::Decimal(da)), Some(HeapObj::Decimal(db))) => return da < db,
            _ => {}
        }
    }
    if a.is_heap()
        && heap.is_int(b)
        && let Some(HeapObj::Decimal(da)) = heap.get(a.as_heap())
    {
        let rhs = bigdecimal::BigDecimal::from(heap.as_int(b));
        return **da < rhs;
    }
    if heap.is_int(a)
        && b.is_heap()
        && let Some(HeapObj::Decimal(db)) = heap.get(b.as_heap())
    {
        let lhs = bigdecimal::BigDecimal::from(heap.as_int(a));
        return lhs < **db;
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
