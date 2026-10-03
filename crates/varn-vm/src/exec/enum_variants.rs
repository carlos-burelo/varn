//! Building a payload-less-shaped enum variant from its descriptor: the one
//! implementation behind the interpreter's `MakeEnumVariant` and both JIT
//! lowerings.

use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use std::sync::Arc;

impl ExecCtx {
    /// The variant with discriminant `tag` described by `meta`,
    /// `Enum.Variant[:field,field...]`.
    pub(crate) fn make_enum_variant(&mut self, tag: i64, meta: &str) -> VmValue {
        let (name_part, fields_part) = match meta.find(':') {
            Some(idx) => (&meta[..idx], &meta[idx + 1..]),
            None => (meta, ""),
        };
        let (enum_name, variant_name) = match name_part.rfind('.') {
            Some(idx) => (&name_part[..idx], &name_part[idx + 1..]),
            None => ("", name_part),
        };
        let fields: Vec<Arc<str>> = if fields_part.is_empty() {
            vec![]
        } else {
            fields_part.split(',').map(Arc::from).collect()
        };
        let payload = VmValue::from_heap_idx(
            self.heap
                .alloc(HeapObj::Object(varn_types::value::ObjRef::empty())),
        );
        self.heap
            .alloc_enum_variant_vm(varn_types::value::EnumVariantData {
                enum_class_id: None,
                enum_name: Arc::from(enum_name),
                variant_name: Arc::from(variant_name),
                variant_tag: tag,
                fields,
                payload,
            })
    }
}
