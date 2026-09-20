//! What the checker proved about which entity an expression names.

use crate::ty::{DynReason, EnumId, FnId, LocalId, ModuleId};
use std::sync::Arc;

/// The entity an expression resolves to.
///
/// Every static variant here is something the checker already proves and the
/// runtime currently re-derives — by name lookup, by bytecode rewriting, or by
/// string comparison against a hard-coded list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Literals and pure arithmetic resolve to nothing.
    None,

    Local(LocalId),
    Param(u32),
    Upvalue(u32),

    /// Numbered at compile time, so `LoadGlobalIdx` is emitted directly and
    /// the runtime rewriting pass disappears.
    GlobalSlot(u32),

    /// A native / prelude symbol (`print`, `assert`, …) at its fixed index in
    /// `GlobalStore::with_native_layout` — the host boundary, numbered by
    /// `varn_builtins::native_global_layout()`. Emitted as `LoadNativeGlobalIdx`.
    NativeGlobal(u32),
    ModuleSlot {
        module: ModuleId,
        slot: u32,
    },

    /// Instance field at a known slot.
    FieldSlot(u16),
    StaticField(u16),

    /// Method at a known vtable index — an integer, not a name.
    VtableSlot(u16),

    /// A call whose target is known: a free function, or a method that cannot
    /// be overridden.
    DirectFn(FnId),

    /// A builtin the compiler can emit directly, instead of the runtime
    /// comparing the method name against a list.
    Intrinsic(u16),

    /// A native operation, by its registered id.
    NativeOp(u64),

    EnumVariant {
        enum_id: EnumId,
        tag: u16,
    },

    /// Honestly dynamic. Carries why, so the remaining name-keyed accesses can
    /// be separated into the ones that are correct and the ones that are holes.
    ByName {
        name: Arc<str>,
        why: DynReason,
    },
}

impl Resolution {
    /// Whether this resolves to a known entity at compile time.
    ///
    /// `None` is deliberately NOT static dispatch: a literal or a pure
    /// arithmetic result resolves to no entity at all, so counting it as
    /// static would inflate every ratio built on this method. The three
    /// states are: resolves to nothing, resolves statically, resolves by name.
    pub fn is_static_dispatch(&self) -> bool {
        !matches!(self, Resolution::None | Resolution::ByName { .. })
    }

    /// Whether this resolution is deferred to runtime by name.
    pub fn is_dynamic_dispatch(&self) -> bool {
        matches!(self, Resolution::ByName { .. })
    }

    /// The reason, when this resolution is dynamic.
    pub fn dyn_reason(&self) -> Option<DynReason> {
        match self {
            Resolution::ByName { why, .. } => Some(*why),
            _ => None,
        }
    }

    /// Whether this resolution only makes sense against a class receiver. The
    /// verifier uses it to demand that a FieldSlot's object actually be one.
    pub fn requires_class_receiver(&self) -> bool {
        matches!(
            self,
            Resolution::FieldSlot(_) | Resolution::StaticField(_) | Resolution::VtableSlot(_)
        )
    }
}
