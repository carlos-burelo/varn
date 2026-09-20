//! Static memory layout descriptors for user classes in Varn.
//!
//! Varn is statically typed. Once a class is declared, its fields, their types,
//! offsets, alignments, and sizes are known and immutable. This module provides
//! the compile-time and runtime descriptor representing that static memory layout.

use std::sync::Arc;
use varn_core::TypeTag;

/// Layout and representation of a single field within a class instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldLayout {
    /// Declared name of the field.
    pub name: Arc<str>,
    /// Static type tag of the field.
    pub type_tag: TypeTag,
    /// Byte offset relative to the payload start (after instance header).
    pub offset: u32,
    /// Size of this field in bytes (e.g. 1 for bool, 8 for int/float, 16 for VmValue).
    pub size: u32,
    /// Alignment of this field in bytes.
    pub align: u32,
    /// True if this field holds a GC reference that the collector must trace.
    pub is_gc_ref: bool,
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
    /// 64-bit bitmap indicating which 8-byte words contain GC references.
    /// Bit `i` is set if word `i` from the payload start is a GC reference.
    pub gc_mask: u64,
}

/// Bytes one field occupies in an instance payload today, and its alignment.
const SLOT_SIZE: u32 = 16;
const SLOT_ALIGN: u32 = 8;

/// Per-field physical size/align/gc-ref-ness for CLASS FIELD layout — not the
/// same table as `TypeTag::field_repr` (used for e.g. FFI/array element
/// sizing), because a class field has one representational constraint that
/// table doesn't carry: `str` can be `KIND_SSO` (an inline 16-byte `VmValue`
/// with no heap object at all, `vm_value.rs`'s `try_from_sso`), so a `str`
/// field cannot be compacted to a bare 8-byte heap index — the value itself
/// might not live on the heap. `char` is excluded for a different reason: it
/// is always `HeapObj::Char` (K1), but reading/writing a compact scalar slot
/// for it would need to intern/deref through the heap, which `InstanceData`
/// (this crate, no heap access) cannot do. Both fall to the safe universal
/// 16-byte `VmValue` slot, exactly like today. Everything else `field_repr`
/// already calls a GC reference (`Array`/`Map`/`Set`/`Class`/`Bytes`/…) is
/// ALWAYS `KIND_HEAP` in this runtime — no inline fast path exists for them —
/// so those compact safely to a bare heap index.
pub fn class_field_repr(tag: TypeTag) -> (u32, u32, bool) {
    match tag {
        TypeTag::Str | TypeTag::Char => (SLOT_SIZE, SLOT_ALIGN, true),
        other => {
            let r = other.field_repr();
            (r.size, r.align, r.is_gc_ref)
        }
    }
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
            gc_mask: 0,
        }
    }

    /// Builds a static memory layout from a list of field (name, type_tag) declarations.
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
        fields_in: &[(Arc<str>, TypeTag)],
    ) -> Self {
        let mut fields = Vec::with_capacity(fields_in.len());
        let mut cur_offset = 0u32;
        let mut max_align = 8u32;
        let mut gc_mask = 0u64;

        for (field_name, tag) in fields_in {
            let (size, align, is_gc) = class_field_repr(*tag);

            max_align = max_align.max(align);
            // Align current offset up to field's required alignment
            let padding = (align - (cur_offset % align)) % align;
            cur_offset += padding;

            let offset = cur_offset;
            if is_gc {
                // Mark every 8-byte word the field spans, not just its first
                // — a 16-byte `Dynamic` field covers two words, and a mask
                // that only ever set one bit per field was silently wrong for
                // any multi-word GC-ref field the moment something finally
                // read `gc_mask` (nothing does yet; this is the first
                // consumer-shaped use, so it gets it right from the start).
                let mut w = offset / 8;
                let end_word = (offset + size).div_ceil(8);
                while w < end_word && w < 64 {
                    gc_mask |= 1u64 << w;
                    w += 1;
                }
            }

            cur_offset += size;
            fields.push(FieldLayout {
                name: field_name.clone(),
                type_tag: *tag,
                offset,
                size,
                align,
                is_gc_ref: is_gc,
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
            gc_mask,
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
