//! The typed IR: the contract between the checker and the backend.
//!
//! Every node carries its type and its resolution as fields of the node, not
//! as optional entries in a side map. See `docs/TIR_CONTRATO_TIPADO.md`.

mod ty;

pub use ty::{
    BackendTy, ClassId, DynReason, EnumId, FnId, LocalId, ModuleId, SigId, TyId, TyListId, TyTable,
};
