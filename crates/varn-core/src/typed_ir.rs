use std::collections::{HashMap, HashSet};

pub use crate::type_tag::TypeTag as NumericKind;

#[derive(Clone, Debug, Default)]
pub struct ExprAnnotation {
    pub numeric: Option<NumericKind>,
    pub type_only: bool,
    pub call_mapping: Option<Vec<Option<usize>>>,
    pub slot_idx: Option<usize>,
    pub exported_slot_idx: Option<usize>,
    pub fixed_field_slot: Option<u16>,
    /// Byte offset within the instance payload for statically typed struct fields.
    pub fixed_field_offset: Option<u32>,
    /// Static TypeTag of the field (e.g. Int, Float, Bool, Str).
    pub fixed_field_tag: Option<crate::type_tag::TypeTag>,
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

/// What an annotation is attached to.
///
/// The key space is part of the key. This map used to be `HashMap<u32, _>`
/// holding two unrelated numbering schemes at once — expressions and import
/// specifiers, both under a byte offset — and a byte offset does not identify
/// an expression: `x` and `x.y` begin at the same byte, so two nodes shared
/// one record and only the disjointness of the fields they happened to set
/// kept that from showing.
///
/// An expression has an `AstId`, so it uses it. A declaration site does not —
/// only `Expr`, `Stmt` and `TypeNode` carry ids, and an import specifier, a
/// parameter or a field is none of those — so it keeps a positional key. As
/// separate variants, `Expr(7)` and `Decl(7)` are different keys and cannot
/// collide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnnKey {
    /// An expression, by its AST id.
    Expr(crate::ast::AstId),
    /// A declaration site with no id of its own, by where it starts.
    Decl(u32),
}

impl AnnKey {
    /// Key for the expression `id`.
    pub fn expr(id: crate::ast::AstId) -> Self {
        AnnKey::Expr(id)
    }

    /// Key for the declaration starting at `offset`.
    pub fn decl(offset: u32) -> Self {
        AnnKey::Decl(offset)
    }
}

/// Everything the checker tells the compiler about one annotated place.
#[derive(Clone, Debug, Default)]
pub struct TypeAnnotations {
    inner: HashMap<AnnKey, ExprAnnotation>,
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

    /// Every annotation. Unordered — callers that need a stable rendering
    /// must sort (`vn debug -p check:types` does).
    pub fn entries(&self) -> impl Iterator<Item = (&AnnKey, &ExprAnnotation)> {
        self.inner.iter()
    }

    pub fn reassigned_names(&self) -> impl Iterator<Item = &str> {
        self.reassigned_names.iter().map(|s| s.as_str())
    }

    pub fn record_reassigned_name(&mut self, name: &str) {
        self.reassigned_names.insert(name.to_owned());
    }

    pub fn is_reassigned_name(&self, name: &str) -> bool {
        self.reassigned_names.contains(name)
    }

    pub fn record_numeric(&mut self, key: AnnKey, kind: NumericKind) {
        self.inner.entry(key).or_default().numeric = Some(kind);
    }

    pub fn get_numeric(&self, key: AnnKey) -> Option<NumericKind> {
        self.inner.get(&key)?.numeric
    }

    pub fn record_type_only(&mut self, key: AnnKey) {
        self.inner.entry(key).or_default().type_only = true;
    }

    pub fn is_type_only(&self, key: AnnKey) -> bool {
        self.inner.get(&key).is_some_and(|a| a.type_only)
    }

    pub fn record_call_mapping(&mut self, key: AnnKey, mapping: Vec<Option<usize>>) {
        self.inner.entry(key).or_default().call_mapping = Some(mapping);
    }

    pub fn get_call_mapping(&self, key: AnnKey) -> Option<&Vec<Option<usize>>> {
        self.inner.get(&key)?.call_mapping.as_ref()
    }

    pub fn record_slot_idx(&mut self, key: AnnKey, slot_idx: usize) {
        self.inner.entry(key).or_default().slot_idx = Some(slot_idx);
    }

    pub fn record_exported_slot_idx(&mut self, key: AnnKey, slot_idx: usize) {
        self.inner.entry(key).or_default().exported_slot_idx = Some(slot_idx);
    }

    pub fn get_exported_slot_idx(&self, key: AnnKey) -> Option<usize> {
        self.inner.get(&key)?.exported_slot_idx
    }

    pub fn get_slot_idx(&self, key: AnnKey) -> Option<usize> {
        self.inner.get(&key)?.slot_idx
    }

    pub fn record_intrinsic(&mut self, key: AnnKey, wire_byte: u8) {
        self.inner.entry(key).or_default().intrinsic = Some(wire_byte);
    }

    pub fn get_intrinsic(&self, key: AnnKey) -> Option<u8> {
        self.inner.get(&key)?.intrinsic
    }

    pub fn record_native_op(&mut self, key: AnnKey, op_id: u64) {
        self.inner.entry(key).or_default().native_op = Some(op_id);
    }

    pub fn get_native_op(&self, key: AnnKey) -> Option<u64> {
        self.inner.get(&key)?.native_op
    }

    /// Mark the computed-member expression as a typed-array index access.
    pub fn record_array_index(&mut self, key: AnnKey) {
        self.inner.entry(key).or_default().array_index = true;
    }

    /// Whether the object of the computed-member is a known Array.
    pub fn get_array_index(&self, key: AnnKey) -> bool {
        self.inner.get(&key).is_some_and(|a| a.array_index)
    }

    /// Record the codegen projection of the annotated place's value type.
    pub fn record_cg_ty(&mut self, key: AnnKey, ty: crate::cg_ty::CgTy) {
        self.inner.entry(key).or_default().cg_ty = Some(ty);
    }

    pub fn get_cg_ty(&self, key: AnnKey) -> Option<&crate::cg_ty::CgTy> {
        self.inner.get(&key)?.cg_ty.as_ref()
    }

    /// Mark a member expression as a statically-known class fixed-field access.
    pub fn record_fixed_field_slot(&mut self, key: AnnKey, slot: u16) {
        self.inner.entry(key).or_default().fixed_field_slot = Some(slot);
    }

    pub fn record_fixed_field_layout(
        &mut self,
        key: AnnKey,
        slot: u16,
        offset: u32,
        tag: crate::type_tag::TypeTag,
    ) {
        let entry = self.inner.entry(key).or_default();
        entry.fixed_field_slot = Some(slot);
        entry.fixed_field_offset = Some(offset);
        entry.fixed_field_tag = Some(tag);
    }

    pub fn get_fixed_field_slot(&self, key: AnnKey) -> Option<u16> {
        self.inner.get(&key)?.fixed_field_slot
    }

    pub fn get_fixed_field_offset(&self, key: AnnKey) -> Option<u32> {
        self.inner.get(&key)?.fixed_field_offset
    }

    pub fn get_fixed_field_tag(&self, key: AnnKey) -> Option<crate::type_tag::TypeTag> {
        self.inner.get(&key)?.fixed_field_tag
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
