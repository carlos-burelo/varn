//! Per-module tables: classes with their layout and vtable, enums, signatures.
//!
//! This is the ONLY authority on where a field lives. Four sites compute that
//! today and two of them disagree; the packed offset the checker computes has
//! no consumer, which is the only reason the divergence has not yet produced a
//! misaligned read.

use crate::ty::{BackendTy, ClassId};
use std::rc::Rc;

/// One field of a class instance.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: Rc<str>,
    pub ty: BackendTy,
    /// Dense index, counting the parent's fields first.
    pub slot: u16,
    /// Byte offset within the instance payload.
    pub offset: u32,
}

/// One entry of a class vtable. Its index IS the dispatch target, so the
/// position in this vector is the whole payload — the name is kept for
/// diagnostics and for the emitter to match overrides against.
#[derive(Debug, Clone, PartialEq)]
pub struct VtableEntry {
    pub name: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: Rc<str>,
    pub parent: Option<ClassId>,
    pub fields: Vec<FieldInfo>,
    pub vtable: Vec<VtableEntry>,
    pub payload_size: u32,
}

/// Bytes one field slot occupies today. Every read and write path still moves
/// a whole VmValue, so this is 16 and the offsets are `slot * 16`. It is a
/// single constant in a single file precisely so that packing later is one
/// change here, not four changes that have to agree.
const SLOT_SIZE: u32 = 16;

impl ClassInfo {
    /// A class with fields and no methods.
    ///
    /// `parent` is the id AND the info: the id goes into the record, the info
    /// supplies the prefix. Taking only one of the two is what leaves a
    /// `parent` field that never gets filled.
    pub fn new(
        name: Rc<str>,
        parent: Option<(ClassId, &ClassInfo)>,
        fields: Vec<(Rc<str>, BackendTy)>,
    ) -> Self {
        Self::new_with_methods(name, parent, fields, Vec::new())
    }

    /// A class with fields and methods, laid out against its parent.
    pub fn new_with_methods(
        name: Rc<str>,
        parent: Option<(ClassId, &ClassInfo)>,
        fields: Vec<(Rc<str>, BackendTy)>,
        methods: Vec<Rc<str>>,
    ) -> Self {
        let parent_info = parent.map(|(_, info)| info);

        // Fields: the parent's prefix, then this class's own, dense.
        let mut out: Vec<FieldInfo> = parent_info.map(|p| p.fields.clone()).unwrap_or_default();
        let mut slot = out.len() as u16;
        for (fname, fty) in fields {
            out.push(FieldInfo {
                name: fname,
                ty: fty,
                slot,
                offset: slot as u32 * SLOT_SIZE,
            });
            slot += 1;
        }
        let payload_size = out.len() as u32 * SLOT_SIZE;

        // Vtable: the parent's entries, then the new ones. A method the parent
        // already has keeps its index — that is what makes the index a valid
        // dispatch target for a base-typed receiver.
        let mut vtable: Vec<VtableEntry> =
            parent_info.map(|p| p.vtable.clone()).unwrap_or_default();
        for m in methods {
            if !vtable.iter().any(|e| e.name == m) {
                vtable.push(VtableEntry { name: m });
            }
        }

        ClassInfo {
            name,
            parent: parent.map(|(id, _)| id),
            fields: out,
            vtable,
            payload_size,
        }
    }

    pub fn field(&self, name: &str) -> Option<&FieldInfo> {
        self.fields.iter().find(|f| f.name.as_ref() == name)
    }

    pub fn field_at(&self, slot: u16) -> Option<&FieldInfo> {
        self.fields.get(slot as usize)
    }

    pub fn method_slot(&self, name: &str) -> Option<u16> {
        self.vtable.iter().position(|e| e.name.as_ref() == name).map(|i| i as u16)
    }

    pub fn method_at(&self, slot: u16) -> Option<&VtableEntry> {
        self.vtable.get(slot as usize)
    }
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: Rc<str>,
    pub tag: u16,
    pub payload: Vec<BackendTy>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Rc<str>,
    pub variants: Vec<VariantInfo>,
}

impl EnumInfo {
    pub fn variant_at(&self, tag: u16) -> Option<&VariantInfo> {
        self.variants.iter().find(|v| v.tag == tag)
    }
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
}

impl Signature {
    pub fn arity(&self) -> usize {
        self.params.len()
    }
}
