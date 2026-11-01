use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::hash::{Hash, Hasher};

const MAX_DEPTH: u32 = 64;

pub(crate) fn structural_hash(v: VmValue, heap: &Heap) -> i64 {
    let mut hasher = rustc_hash::FxHasher::default();
    feed(v, heap, &mut hasher, 0);
    hasher.finish() as i64
}

fn feed(v: VmValue, heap: &Heap, h: &mut rustc_hash::FxHasher, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    if heap.is_int(v) {
        0u8.hash(h);
        heap.as_int(v).hash(h);
        return;
    }
    if v.is_f64() {
        1u8.hash(h);
        let f = v.as_f64();
        (if f == 0.0 { 0.0f64 } else { f }).to_bits().hash(h);
        return;
    }
    if v.is_bool() {
        2u8.hash(h);
        v.as_bool().hash(h);
        return;
    }
    if v.is_null() {
        3u8.hash(h);
        return;
    }
    if heap.is_string(v) {
        4u8.hash(h);
        heap.str_repr_borrowed(v).as_ref().hash(h);
        return;
    }
    if !v.is_heap() {
        10u8.hash(h);
        return;
    }
    let r = v.as_heap();
    if let Some(inst) = heap.instance(r)
        && let Some(class) = varn_types::ClassObj::find_by_id(inst.class_id)
    {
        let layout = class.layout();
        if layout.is_value {
            5u8.hash(h);
            inst.class_id.hash(h);
            for field in &layout.fields {
                if let Some(value) = inst.read_field(field) {
                    feed(value, heap, h, depth + 1);
                }
            }
            return;
        }
    }
    match heap.get(r) {
        Some(HeapObj::Tuple(items)) => {
            6u8.hash(h);
            for i in 0..items.len() {
                if let Some(item) = items.get_vm(i) {
                    feed(item, heap, h, depth + 1);
                }
            }
        }
        Some(HeapObj::Record(record)) => {
            11u8.hash(h);
            let mut combined: u64 = 0;
            for key in record.keys() {
                let mut entry = rustc_hash::FxHasher::default();
                key.as_ref().hash(&mut entry);
                if let Some(value) = record.get(key.as_ref()) {
                    feed(value, heap, &mut entry, depth + 1);
                }
                combined = combined.wrapping_add(entry.finish());
            }
            combined.hash(h);
        }
        Some(HeapObj::EnumVariant(variant)) => {
            12u8.hash(h);
            variant.def.enum_name.as_ref().hash(h);
            variant.tag().hash(h);
            for slot in &variant.slots {
                feed(*slot, heap, h, depth + 1);
            }
        }
        Some(HeapObj::Char(c)) => {
            7u8.hash(h);
            c.hash(h);
        }
        Some(HeapObj::BigInt(b)) => {
            8u8.hash(h);
            b.to_string().hash(h);
        }
        Some(HeapObj::Decimal(d)) => {
            9u8.hash(h);
            d.to_string().hash(h);
        }
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Object(_)
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
            | HeapObj::Spread(_),
        )
        | None => {
            13u8.hash(h);
            r.addr().hash(h);
        }
    }
}
