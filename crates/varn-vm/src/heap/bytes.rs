use super::cells::CellSpace;
use super::obj::HeapObj;
use varn_types::HeapRef;

#[inline(always)]
pub(crate) fn external_bytes(cells: &CellSpace, r: HeapRef) -> u64 {
    if cells.header_kind(r) == varn_types::cell::CELL_KIND_INSTANCE {
        return 0;
    }
    external_bytes_body(cells, r)
}

#[inline(always)]
fn external_bytes_body(cells: &CellSpace, r: HeapRef) -> u64 {
    match cells.get(r) {
        HeapObj::Str(s) => s.external_bytes(),
        HeapObj::Array(a) | HeapObj::Tuple(a) => a.repr().heap_bytes(),
        HeapObj::Object(o) => o.borrow().heap_bytes(),
        HeapObj::Buffer(b) => b.heap_bytes(),
        HeapObj::Map(m) => m.read().heap_bytes(),
        HeapObj::Set(s) => s.read().heap_bytes(),
        HeapObj::Module(_)
        | HeapObj::FrozenModule(_)
        | HeapObj::VmClosure(_)
        | HeapObj::Class(_)
        | HeapObj::NativeFn(..)
        | HeapObj::BoundMethod(_)
        | HeapObj::Task(_)
        | HeapObj::TaskHandle(_)
        | HeapObj::Range(_)
        | HeapObj::EnumVariant(_)
        | HeapObj::BigInt(_)
        | HeapObj::Decimal(_)
        | HeapObj::Char(_)
        | HeapObj::Generator(_)
        | HeapObj::Spread(_) => 0,
    }
}
