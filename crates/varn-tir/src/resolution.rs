

use crate::ty::{DynReason, EnumId, FnId, LocalId, ModuleId};
use std::sync::Arc;






#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    
    None,

    Local(LocalId),
    Param(u32),
    Upvalue(u32),

    
    
    GlobalSlot(u32),

    
    
    
    NativeGlobal(u32),
    ModuleSlot {
        module: ModuleId,
        slot: u32,
    },

    
    FieldSlot(u16),
    StaticField(u16),

    
    VtableSlot(u16),

    
    
    DirectFn(FnId),

    
    
    Intrinsic(u16),

    
    NativeOp(u64),

    EnumVariant {
        enum_id: EnumId,
        tag: u16,
    },

    
    
    ByName {
        name: Arc<str>,
        why: DynReason,
    },
}

impl Resolution {
    
    
    
    
    
    
    pub fn is_static_dispatch(&self) -> bool {
        !matches!(self, Resolution::None | Resolution::ByName { .. })
    }

    
    pub fn is_dynamic_dispatch(&self) -> bool {
        matches!(self, Resolution::ByName { .. })
    }

    
    pub fn dyn_reason(&self) -> Option<DynReason> {
        match self {
            Resolution::ByName { why, .. } => Some(*why),
            _ => None,
        }
    }

    
    
    pub fn requires_class_receiver(&self) -> bool {
        matches!(
            self,
            Resolution::FieldSlot(_) | Resolution::StaticField(_) | Resolution::VtableSlot(_)
        )
    }
}
