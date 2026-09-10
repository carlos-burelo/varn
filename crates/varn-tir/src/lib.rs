//! The typed IR: the contract between the checker and the backend.
//!
//! Every node carries its type and its resolution as fields of the node, not
//! as optional entries in a side map. See `docs/TIR_CONTRATO_TIPADO.md`.

mod ty;

pub use ty::{
    BackendTy, ClassId, DynReason, EnumId, FnId, LocalId, ModuleId, SigId, TyId, TyListId, TyTable,
};

mod resolution;
pub use resolution::Resolution;

mod node;
pub use node::{
    Span, TirArg, TirArrayEl, TirBinOp, TirClassAccessor, TirClassDef, TirClassMember, TirExport,
    TirExpr, TirExprKind, TirFunction, TirImport, TirImportKind, TirImportSpec, TirModule,
    TirObjectEntry, TirStmt, TirUnOp, TirUpvalue, TirVariantDef,
};

mod tables;
pub use tables::{ClassInfo, EnumInfo, FieldInfo, Signature, VariantInfo, VtableEntry};

mod verify;
pub use verify::{verify_module, VerifyError};

mod coverage;
pub use coverage::Coverage;
