mod ty;

pub use ty::{
    BackendTy, ClassId, DynReason, EnumId, FnId, LiteralStrId, LocalId, ModuleId, ShapeField,
    ShapeId, SigId, TyId, TyListId, TyTable,
};
pub use ty::PortableTable;

mod resolution;
pub use resolution::Resolution;

mod node;
pub use node::{
    CheckKind, Span, TirArg, TirArrayEl, TirBinOp, TirExpr, TirExprKind, TirObjectEntry, TirStmt,
    TirUnOp, TirUpvalue,
};

mod module;
pub use module::{
    TirClassAccessor, TirClassDef, TirClassMember, TirExport, TirFunction, TirImport,
    TirImportKind, TirImportSpec, TirImportedVariant, TirModule, TirPropertyDecorator,
    TirVariantDef,
};

mod tables;
pub use tables::{Ancestry, ClassInfo, EnumInfo, FieldInfo, Signature, VariantInfo, VtableEntry};

mod verify;
pub use verify::{verify_module, VerifyError};

mod coverage;
pub use coverage::Coverage;

mod registry;
pub mod children;
pub mod inlinable;
pub mod link;
pub mod package;
pub mod walk;
pub use registry::{ClassRegistry, ClassRegistryBuilder, GlobalClassId};
