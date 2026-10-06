

use crate::ty::{BackendTy, ClassId, SigId, TyTable};
use std::sync::Arc;
use varn_core::layout::ClassLayout;


#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: Arc<str>,
    pub ty: BackendTy,
    
    pub slot: u16,
}





#[derive(Debug, Clone, PartialEq)]
pub struct VtableEntry {
    pub name: Arc<str>,
    pub sig: SigId,
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: Arc<str>,
    pub parent: Option<ClassId>,
    pub fields: Vec<FieldInfo>,
    pub vtable: Vec<VtableEntry>,
    pub layout: ClassLayout,
    
    
    pub constructor: Option<SigId>,
}

pub enum Ancestry<'a> {
    Root,
    Local(ClassId, &'a ClassInfo),
    Foreign(Vec<(Arc<str>, BackendTy)>),
}

impl ClassInfo {
    
    
    
    
    
    pub fn new(
        name: Arc<str>,
        parent: Ancestry<'_>,
        fields: Vec<(Arc<str>, BackendTy)>,
        types: &TyTable,
    ) -> Self {
        Self::new_with_methods(name, parent, fields, Vec::new(), types)
    }

    
    pub fn new_with_methods(
        name: Arc<str>,
        parent: Ancestry<'_>,
        fields: Vec<(Arc<str>, BackendTy)>,
        methods: Vec<(Arc<str>, SigId)>,
        types: &TyTable,
    ) -> Self {
        let (parent_id, parent_info, foreign) = match parent {
            Ancestry::Root => (None, None, Vec::new()),
            Ancestry::Local(id, info) => (Some(id), Some(info), Vec::new()),
            Ancestry::Foreign(prefix) => (None, None, prefix),
        };

        let mut out: Vec<FieldInfo> = parent_info.map(|p| p.fields.clone()).unwrap_or_default();
        for (slot, (fname, fty)) in (out.len() as u16..).zip(foreign) {
            out.push(FieldInfo {
                name: fname,
                ty: fty,
                slot,
            });
        }
        let first_slot = out.len() as u16;
        for (slot, (fname, fty)) in (first_slot..).zip(fields) {
            out.push(FieldInfo {
                name: fname,
                ty: fty,
                slot,
            });
        }
        let kinds: Vec<(Arc<str>, Option<varn_core::RuntimeKind>)> = out
            .iter()
            .map(|f| (f.name.clone(), f.ty.field_kind(types)))
            .collect();
        let layout = ClassLayout::from_fields(&kinds);

        
        
        
        
        
        let mut vtable: Vec<VtableEntry> =
            parent_info.map(|p| p.vtable.clone()).unwrap_or_default();
        for (m_name, m_sig) in methods {
            if let Some(entry) = vtable.iter_mut().find(|e| e.name == m_name) {
                
                entry.sig = m_sig;
            } else {
                
                vtable.push(VtableEntry {
                    name: m_name,
                    sig: m_sig,
                });
            }
        }

        ClassInfo {
            name,
            parent: parent_id,
            fields: out,
            vtable,
            layout,
            constructor: None,
        }
    }

    pub fn field(&self, name: &str) -> Option<&FieldInfo> {
        self.fields.iter().find(|f| f.name.as_ref() == name)
    }

    pub fn field_at(&self, slot: u16) -> Option<&FieldInfo> {
        self.fields.get(slot as usize)
    }

    pub fn method_slot(&self, name: &str) -> Option<u16> {
        self.vtable
            .iter()
            .position(|e| e.name.as_ref() == name)
            .map(|i| i as u16)
    }

    pub fn method_at(&self, slot: u16) -> Option<&VtableEntry> {
        self.vtable.get(slot as usize)
    }
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: Arc<str>,
    pub tag: u16,
    pub payload: Vec<BackendTy>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Arc<str>,
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
    
    
    pub has_rest: bool,
}

impl Signature {
    pub fn arity(&self) -> usize {
        self.params.len()
    }
}
