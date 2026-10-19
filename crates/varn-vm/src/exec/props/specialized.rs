use varn_core::MemberKey;

use crate::heap::{Heap, HeapInner, HeapObj};
use crate::value::VmValue;

pub(super) fn resolve_specialized_property(
    obj: VmValue,
    key: &str,
    heap: &mut Heap,
) -> Option<Result<VmValue, String>> {
    if obj.is_sso() {
        if key == MemberKey::Length.as_str() {
            return Some(Ok(VmValue::from_int(heap.str_repr(obj).len() as i64)));
        }
        return None;
    }
    if !obj.is_heap() {
        return None;
    }
    match heap.get(obj.as_heap()).cloned() {
        Some(HeapObj::Class(cls)) => cls.get_static(key).or_else(|| cls.find_method(key)).map(Ok),
        Some(HeapObj::Array(arr)) => {
            if key == MemberKey::Length.as_str() {
                return Some(Ok(VmValue::from_int(arr.len() as i64)));
            }
            let n = key.parse::<usize>().ok()?;
            Some(
                arr.get_vm(n)
                    .ok_or_else(|| format!("index {n} out of bounds for array")),
            )
        }
        Some(HeapObj::Str(_)) => {
            if key == MemberKey::Length.as_str() {
                return Some(Ok(VmValue::from_int(heap.str_repr(obj).len() as i64)));
            }
            None
        }
        Some(HeapObj::Buffer(b)) => {
            if key == MemberKey::Length.as_str() {
                return Some(Ok(VmValue::from_int(b.len() as i64)));
            }
            None
        }
        Some(HeapObj::Map(m)) => {
            let found = heap
                .lookup_str_map_key(key)
                .and_then(|k| m.read().get(&k).copied());
            found.map(Ok)
        }
        Some(HeapObj::EnumVariant(ev)) => enum_variant_property(&ev, key, heap),
        Some(
            HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Record(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => None,
    }
}

fn enum_variant_property(
    ev: &varn_types::value::EnumVariantData,
    key: &str,
    heap: &mut Heap,
) -> Option<Result<VmValue, String>> {
    if let Some(f) = ev.field(key) {
        return Some(Ok(f));
    }
    match MemberKey::from_str(key) {
        Some(MemberKey::Tag) | Some(MemberKey::RawValue) => Some(Ok(VmValue::from_int(ev.tag()))),
        Some(MemberKey::VariantName) | Some(MemberKey::Name) => {
            Some(Ok(HeapInner::alloc_str(heap, &*ev.def.variant_name)))
        }
        Some(
            MemberKey::Value0
            | MemberKey::Length
            | MemberKey::Size
            | MemberKey::Variant
            | MemberKey::Callable
            | MemberKey::ToString
            | MemberKey::ValueOf
            | MemberKey::IterNext
            | MemberKey::IterDone
            | MemberKey::IterValue
            | MemberKey::Push
            | MemberKey::Pop
            | MemberKey::Shift
            | MemberKey::Unshift
            | MemberKey::Slice
            | MemberKey::Join
            | MemberKey::IndexOf
            | MemberKey::Includes
            | MemberKey::Split
            | MemberKey::Class
            | MemberKey::Type
            | MemberKey::Fields
            | MemberKey::Methods
            | MemberKey::Keys
            | MemberKey::Values
            | MemberKey::Entries
            | MemberKey::HasOwn
            | MemberKey::Repeat
            | MemberKey::PadStart
            | MemberKey::PadEnd
            | MemberKey::Start
            | MemberKey::End
            | MemberKey::StartsWith
            | MemberKey::EndsWith
            | MemberKey::CharCodeAt
            | MemberKey::CodePointAt
            | MemberKey::Substring
            | MemberKey::Substr
            | MemberKey::At
            | MemberKey::LastIndexOf
            | MemberKey::Message
            | MemberKey::Stack
            | MemberKey::Kind
            | MemberKey::IsStatic
            | MemberKey::IsPrivate
            | MemberKey::Inclusive
            | MemberKey::IterArray
            | MemberKey::IterIndex
            | MemberKey::IterResult
            | MemberKey::IterRange
            | MemberKey::CharCode,
        )
        | None => None,
    }
}
