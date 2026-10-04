//! Static memory layout descriptors for user classes in Varn.
//!
//! Varn is statically typed. Once a class is declared, its fields, their types,
//! offsets, alignments, and sizes are known and immutable. This module provides
//! the compile-time and runtime descriptor representing that static memory layout.

use super::{GcLayout, GcSlot, TypeLayout};
use crate::RuntimeKind;
use std::sync::Arc;

/// Layout and representation of a single field within a class instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldLayout {
    /// Declared name of the field.
    pub name: Arc<str>,
    /// Runtime kind the field is laid out by; `None` is a boxed `VmValue`.
    pub kind: Option<RuntimeKind>,
    /// Byte offset relative to the payload start (after instance header).
    pub offset: u32,
    pub layout: TypeLayout,
}

impl FieldLayout {
    /// The field at `offset` laid out by `kind`, for an access whose offset
    /// the compiler baked (no `ClassLayout` lookup).
    pub fn at(offset: u32, kind: Option<RuntimeKind>) -> Self {
        Self {
            name: Arc::from(""),
            kind,
            offset,
            layout: TypeLayout::of_field(kind),
        }
    }
}

/// Static memory layout for an entire class instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClassLayout {
    /// Total instance payload size in bytes (excluding GC object header), padded to struct alignment.
    pub payload_size: u32,
    /// Maximum alignment requirement across all fields (at least 8 for 64-bit alignment).
    pub alignment: u32,
    /// Ordered list of fields.
    pub fields: Vec<FieldLayout>,
    /// Where the instance's references are: what the collector visits.
    pub gc: GcLayout,
}

impl ClassLayout {
    /// Builds a static memory layout from a list of field (name, kind) declarations.
    ///
    /// Each field takes the size, alignment and representation
    /// [`TypeLayout::of_field`] gives its kind, at the next offset aligned to
    /// it; the payload is padded to the widest alignment (at least 8). The
    /// reference slots, in field order, are the class's [`GcLayout`].
    pub fn from_fields(fields_in: &[(Arc<str>, Option<RuntimeKind>)]) -> Self {
        let mut fields = Vec::with_capacity(fields_in.len());
        let mut cur_offset = 0u32;
        let mut max_align = 8u32;
        let mut gc = GcLayout::default();

        for (field_name, kind) in fields_in {
            let layout = TypeLayout::of_field(*kind);
            let align = layout.align;
            max_align = max_align.max(align);
            let padding = (align - (cur_offset % align)) % align;
            cur_offset += padding;

            let offset = cur_offset;
            if layout.repr.holds_reference() {
                gc.slots.push(GcSlot {
                    offset,
                    repr: layout.repr,
                });
            }
            cur_offset += layout.size;
            fields.push(FieldLayout {
                name: field_name.clone(),
                kind: *kind,
                offset,
                layout,
            });
        }

        // Align total payload size up to max_align (minimum 8)
        let end_padding = (max_align - (cur_offset % max_align)) % max_align;
        let payload_size = cur_offset + end_padding;

        Self {
            payload_size,
            alignment: max_align,
            fields,
            gc,
        }
    }

    /// Computes and returns the field layout for the given field name, if it exists.
    pub fn get_field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|f| f.name.as_ref() == name)
    }

    /// Computes and returns the field layout by field index.
    pub fn get_field_by_index(&self, idx: usize) -> Option<&FieldLayout> {
        self.fields.get(idx)
    }

    /// Total number of declared fields.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}
