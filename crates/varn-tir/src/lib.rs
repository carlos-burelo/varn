




mod ty;

pub use ty::{
    BackendTy, ClassId, DynReason, EnumId, FnId, LocalId, ModuleId, SigId, TyId, TyListId, TyTable,
};

mod resolution;
pub use resolution::Resolution;

mod node;
pub use node::{
    Span, TirArg, TirArrayEl, TirBinOp, TirExpr, TirExprKind, TirObjectEntry, TirStmt, TirUnOp,
    TirUpvalue,
};

mod module;
pub use module::{
    TirClassAccessor, TirClassDef, TirClassMember, TirExport, TirFunction, TirImport,
    TirImportKind, TirImportSpec, TirModule, TirVariantDef,
};

mod tables;
pub use tables::{Ancestry, ClassInfo, EnumInfo, FieldInfo, Signature, VariantInfo, VtableEntry};

mod verify;
pub use verify::{verify_module, VerifyError};

mod coverage;
pub use coverage::Coverage;
