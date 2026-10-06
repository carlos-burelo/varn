





use super::{GcLayout, GcSlot, TypeLayout};
use crate::RuntimeKind;
use std::sync::Arc;


#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldLayout {
    
    pub name: Arc<str>,
    
    pub kind: Option<RuntimeKind>,
    
    pub offset: u32,
    pub layout: TypeLayout,
}

impl FieldLayout {
    
    
    pub fn at(offset: u32, kind: Option<RuntimeKind>) -> Self {
        Self {
            name: Arc::from(""),
            kind,
            offset,
            layout: TypeLayout::of_field(kind),
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClassLayout {
    
    pub payload_size: u32,
    
    pub alignment: u32,
    
    pub fields: Vec<FieldLayout>,
    
    pub gc: GcLayout,
}

impl ClassLayout {
    
    
    
    
    
    
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

        
        let end_padding = (max_align - (cur_offset % max_align)) % max_align;
        let payload_size = cur_offset + end_padding;

        Self {
            payload_size,
            alignment: max_align,
            fields,
            gc,
        }
    }

    
    pub fn get_field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|f| f.name.as_ref() == name)
    }

    
    pub fn get_field_by_index(&self, idx: usize) -> Option<&FieldLayout> {
        self.fields.get(idx)
    }

    
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}
