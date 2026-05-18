use std::collections::HashMap;

pub use varn_base::TypeTag as NumericKind;

#[derive(Clone, Debug)]
pub struct TypeAnnotations {
    pub numerics: HashMap<u32, NumericKind>,
    pub type_only_offsets: std::collections::HashSet<u32>,

    pub call_mappings: HashMap<u32, Vec<Option<usize>>>,
}

impl TypeAnnotations {
    pub fn new() -> Self {
        TypeAnnotations {
            numerics: HashMap::new(),
            type_only_offsets: std::collections::HashSet::new(),
            call_mappings: HashMap::new(),
        }
    }
    pub fn record_numeric(&mut self, offset: u32, kind: NumericKind) {
        self.numerics.insert(offset, kind);
    }

    pub fn get_numeric(&self, offset: u32) -> Option<NumericKind> {
        self.numerics.get(&offset).copied()
    }

    pub fn record_type_only(&mut self, offset: u32) {
        self.type_only_offsets.insert(offset);
    }

    pub fn contains_type_only(&self, offset: u32) -> bool {
        self.type_only_offsets.contains(&offset)
    }

    pub fn record_call_mapping(&mut self, call_id: u32, mapping: Vec<Option<usize>>) {
        self.call_mappings.insert(call_id, mapping);
    }

    pub fn get_call_mapping(&self, call_id: u32) -> Option<&Vec<Option<usize>>> {
        self.call_mappings.get(&call_id)
    }

    pub fn is_empty(&self) -> bool {
        self.numerics.is_empty()
            && self.type_only_offsets.is_empty()
            && self.call_mappings.is_empty()
    }
}

impl Default for TypeAnnotations {
    fn default() -> Self {
        Self::new()
    }
}
