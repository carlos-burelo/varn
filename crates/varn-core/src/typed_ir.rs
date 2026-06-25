use std::collections::{HashMap, HashSet};

pub use varn_base::TypeTag as NumericKind;

#[derive(Clone, Debug, Default)]
pub struct ExprAnnotation {
    pub numeric: Option<NumericKind>,
    pub type_only: bool,
    pub call_mapping: Option<Vec<Option<usize>>>,
    pub slot_idx: Option<usize>,
    pub intrinsic: Option<u8>,
    pub native_op: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct TypeAnnotations {
    inner: HashMap<u32, ExprAnnotation>,
    module_caps: Vec<String>,
    reassigned_names: HashSet<String>,
}

impl TypeAnnotations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_module_cap(&mut self, cap: String) {
        if !self.module_caps.contains(&cap) {
            self.module_caps.push(cap);
        }
    }

    pub fn module_caps(&self) -> &[String] {
        &self.module_caps
    }

    pub fn record_reassigned_name(&mut self, name: &str) {
        self.reassigned_names.insert(name.to_owned());
    }

    pub fn is_reassigned_name(&self, name: &str) -> bool {
        self.reassigned_names.contains(name)
    }

    pub fn record_numeric(&mut self, offset: u32, kind: NumericKind) {
        self.inner.entry(offset).or_default().numeric = Some(kind);
    }

    pub fn get_numeric(&self, offset: u32) -> Option<NumericKind> {
        self.inner.get(&offset)?.numeric
    }

    pub fn record_type_only(&mut self, offset: u32) {
        self.inner.entry(offset).or_default().type_only = true;
    }

    pub fn contains_type_only(&self, offset: u32) -> bool {
        self.inner.get(&offset).map_or(false, |a| a.type_only)
    }

    pub fn record_call_mapping(&mut self, call_id: u32, mapping: Vec<Option<usize>>) {
        self.inner.entry(call_id).or_default().call_mapping = Some(mapping);
    }

    pub fn get_call_mapping(&self, call_id: u32) -> Option<&Vec<Option<usize>>> {
        self.inner.get(&call_id)?.call_mapping.as_ref()
    }

    pub fn record_slot_idx(&mut self, offset: u32, slot_idx: usize) {
        self.inner.entry(offset).or_default().slot_idx = Some(slot_idx);
    }

    pub fn get_slot_idx(&self, offset: u32) -> Option<usize> {
        self.inner.get(&offset)?.slot_idx
    }

    pub fn record_intrinsic(&mut self, offset: u32, wire_byte: u8) {
        self.inner.entry(offset).or_default().intrinsic = Some(wire_byte);
    }

    pub fn get_intrinsic(&self, offset: u32) -> Option<u8> {
        self.inner.get(&offset)?.intrinsic
    }

    pub fn record_native_op(&mut self, offset: u32, op_id: u64) {
        self.inner.entry(offset).or_default().native_op = Some(op_id);
    }

    pub fn get_native_op(&self, offset: u32) -> Option<u64> {
        self.inner.get(&offset)?.native_op
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
