use crate::node::{TirExpr, TirStmt};
use crate::ty::{BackendTy, ClassId, EnumId, FnId, SigId, TyTable};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TirFunction {
    pub name: Arc<str>,
    pub sig: SigId,
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
    pub locals: Vec<BackendTy>,
    pub body: Vec<TirStmt>,
    pub has_this: bool,
    pub this_class: Option<ClassId>,
    /// Declared shape, propagated from the source. Gates `Await` / `Yield` in
    /// the verifier and tells the backend to build a state machine.
    pub is_async: bool,
    pub is_generator: bool,
    /// The last parameter is `...rest`: the VM packs the trailing arguments
    /// into an array bound to it.
    pub has_rest: bool,
}

/// Everything the backend needs to BUILD a class/enum object at module load
/// and bind it to its global. The `classes` / `enums` tables describe layout
/// and dispatch; this describes construction. Instance-field names and types
/// come from the referenced table entry, not repeated here.
#[derive(Debug, Clone, Default)]
pub struct TirClassDef {
    pub name: Arc<str>,
    /// Table handle: `Some(Ok)` a class, `Some(Err)` an enum, `None` neither
    /// resolved (a generic-only or erased declaration — still built by name).
    pub class_id: Option<ClassId>,
    pub enum_id: Option<EnumId>,
    /// The base class, resolved from `extends` — more reliable than
    /// `ClassInfo::parent`, which the binder sometimes leaves unset.
    pub parent: Option<ClassId>,
    /// Hoisted temporaries from `super_class` / decorator / static-init
    /// expressions, emitted before the `MakeClass`.
    pub prelude: Vec<TirStmt>,
    /// The `extends` expression, evaluated for the `MakeClass` super argument.
    pub super_class: Option<TirExpr>,
    /// Static fields / consts: name + optional initializer.
    pub statics: Vec<(Arc<str>, Option<TirExpr>)>,
    /// Methods and the constructor: key, body `FnId`, `is_static`.
    pub methods: Vec<TirClassMember>,
    /// Getters / setters: key, body `FnId`, `is_getter`, `is_static`.
    pub accessors: Vec<TirClassAccessor>,
    /// Class decorators, applied outermost-last.
    pub decorators: Vec<TirExpr>,
    /// `static { ... }` blocks, as `FnId`s to invoke after the class is bound.
    pub static_blocks: Vec<FnId>,
    /// Enum variants: name, tag, metadata string, payload default args.
    pub variants: Vec<TirVariantDef>,
}

#[derive(Debug, Clone)]
pub struct TirClassMember {
    pub key: Arc<str>,
    pub func: FnId,
    pub is_static: bool,
    pub is_private: bool,
    /// Method decorators, applied innermost-first.
    pub decorators: Vec<TirExpr>,
}

#[derive(Debug, Clone)]
pub struct TirClassAccessor {
    pub key: Arc<str>,
    pub func: FnId,
    pub is_getter: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct TirVariantDef {
    pub name: Arc<str>,
    pub tag: i64,
    pub meta: Arc<str>,
    pub const_args: Vec<TirExpr>,
}

#[derive(Debug, Clone)]
pub enum TirImportKind {
    Default,
    Named(Arc<str>),
    Namespace,
}

#[derive(Debug, Clone)]
pub struct TirImportSpec {
    pub local: Arc<str>,
    pub kind: TirImportKind,
}

/// One `import ... from "src"` — the linkage the backend turns into a
/// `LoadModule` plus a `StoreGlobal` per bound name.
#[derive(Debug, Clone)]
pub struct TirImport {
    pub source: Arc<str>,
    pub is_type_only: bool,
    pub specs: Vec<TirImportSpec>,
}

/// One name this module exposes. The backend fills the module slot named by
/// `exported` from either a local global or, for `export {..} from "src"`, a
/// property of that source module.
#[derive(Debug, Clone)]
pub struct TirExport {
    pub exported: Arc<str>,
    pub local: Arc<str>,
    /// `Some(src)` — a re-export; the value is `src`'s `local` property.
    pub reexport_from: Option<Arc<str>>,
    /// `export * as ns from "src"` — bind the whole module object.
    pub namespace: bool,
}

#[derive(Debug)]
pub struct TirModule {
    pub source_file: Arc<str>,
    pub imports: Vec<TirImport>,
    pub exports: Vec<TirExport>,
    pub types: TyTable,
    pub classes: Vec<crate::tables::ClassInfo>,
    pub enums: Vec<crate::tables::EnumInfo>,
    pub signatures: Vec<crate::tables::Signature>,
    pub functions: Vec<TirFunction>,
    pub globals: Vec<BackendTy>,
    /// The name of each global, parallel to `globals`. A `GlobalSlot(n)`
    /// resolution names `globals[n]` / `global_names[n]`.
    pub global_names: Vec<Arc<str>>,
    /// Class / enum construction, one per top-level declaration, in source
    /// order. Empty for a module with no classes or enums.
    pub class_defs: Vec<TirClassDef>,
    pub top_level: TirFunction,
}

impl TirModule {
    pub fn class(&self, id: ClassId) -> Option<&crate::tables::ClassInfo> {
        self.classes.get(id.0 as usize)
    }
    pub fn enum_info(&self, id: EnumId) -> Option<&crate::tables::EnumInfo> {
        self.enums.get(id.0 as usize)
    }
    pub fn signature(&self, id: SigId) -> Option<&crate::tables::Signature> {
        self.signatures.get(id.0 as usize)
    }
    pub fn function(&self, id: FnId) -> Option<&TirFunction> {
        self.functions.get(id.0 as usize)
    }
}
