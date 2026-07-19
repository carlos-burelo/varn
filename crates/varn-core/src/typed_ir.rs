use std::collections::{HashMap, HashSet};

pub use varn_base::TypeTag as NumericKind;

#[derive(Clone, Debug, Default)]
pub struct ExprAnnotation {
    pub numeric: Option<NumericKind>,
    pub type_only: bool,
    pub call_mapping: Option<Vec<Option<usize>>>,
    pub slot_idx: Option<usize>,
    pub fixed_field_slot: Option<u16>,
    pub intrinsic: Option<u8>,
    pub native_op: Option<u64>,
    /// Set when the object of a computed-member expression is a statically-known Array type.
    /// Enables the compiler to emit `ArrayGetIndex`/`ArraySetIndex` instead of the generic
    /// `GetIndex`/`SetIndex`, skipping the runtime heap-type dispatch.
    pub array_index: bool,
    /// Codegen projection of this expression's checker-inferred VALUE type
    /// (see `crate::cg_ty`). Recorded where the value's type matters
    /// downstream (loads, calls, identifiers); absent means Dynamic.
    pub cg_ty: Option<crate::cg_ty::CgTy>,
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

    /// Mark the computed-member expression at `offset` as a typed-array index access.
    pub fn record_array_index(&mut self, offset: u32) {
        self.inner.entry(offset).or_default().array_index = true;
    }

    /// Returns `true` when the object of the computed-member at `offset` is a known Array.
    pub fn get_array_index(&self, offset: u32) -> bool {
        self.inner.get(&offset).map_or(false, |a| a.array_index)
    }

    /// Record the codegen projection of the expression's value type.
    pub fn record_cg_ty(&mut self, offset: u32, ty: crate::cg_ty::CgTy) {
        self.inner.entry(offset).or_default().cg_ty = Some(ty);
    }

    pub fn get_cg_ty(&self, offset: u32) -> Option<&crate::cg_ty::CgTy> {
        self.inner.get(&offset)?.cg_ty.as_ref()
    }

    /// Mark the member expression at `offset` as a statically-known class fixed field slot access.
    pub fn record_fixed_field_slot(&mut self, offset: u32, slot: u16) {
        self.inner.entry(offset).or_default().fixed_field_slot = Some(slot);
    }

    /// Returns the fixed field slot index when the member expression at `offset` is on a known class type.
    pub fn get_fixed_field_slot(&self, offset: u32) -> Option<u16> {
        self.inner.get(&offset)?.fixed_field_slot
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
