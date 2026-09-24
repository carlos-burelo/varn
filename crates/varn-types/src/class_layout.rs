//! Static memory layout descriptors for user classes in Varn.
//!
//! Varn is statically typed. Once a class is declared, its fields, their types,
//! offsets, alignments, and sizes are known and immutable. This module provides
//! the compile-time and runtime descriptor representing that static memory layout.

use crate::layout::{GcLayout, GcSlot, TypeLayout};
use std::sync::Arc;
use varn_core::RuntimeKind;

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
    /// Class name.
    pub name: Arc<str>,
    /// Unique class id.
    pub class_id: u32,
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
    /// Creates a new empty class layout with default alignment of 8.
    pub fn new(name: impl Into<Arc<str>>, class_id: u32) -> Self {
        Self {
            name: name.into(),
            class_id,
            payload_size: 0,
            alignment: 8,
            fields: Vec::new(),
            gc: GcLayout::default(),
        }
    }

    /// Builds a static memory layout from a list of field (name, kind) declarations.
    ///
    /// Computes aligned byte offsets for all fields according to native static typing rules:
    /// - `int` (i64): size 8, align 8
    /// - `float` (f64): size 8, align 8
    /// - `bool`: size 1, align 1
    /// - `char`: size 4, align 4
    /// - GC references (`str`, `object`, `class`, `array`, `map`, `set`, etc.): size 8 (packed pointer), align 8
    /// - Dynamic/Unknown: size 16 (`VmValue`), align 8
    pub fn from_fields(
        name: impl Into<Arc<str>>,
        class_id: u32,
        fields_in: &[(Arc<str>, Option<RuntimeKind>)],
    ) -> Self {
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
            name: name.into(),
            class_id,
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
