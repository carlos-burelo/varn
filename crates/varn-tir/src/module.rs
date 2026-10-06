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
    
    
    pub is_async: bool,
    pub is_generator: bool,
    
    
    pub has_rest: bool,
}





#[derive(Debug, Clone, Default)]
pub struct TirClassDef {
    pub name: Arc<str>,
    
    
    pub class_id: Option<ClassId>,
    pub enum_id: Option<EnumId>,
    
    
    pub parent: Option<ClassId>,
    
    
    pub prelude: Vec<TirStmt>,
    
    pub super_class: Option<TirExpr>,
    
    pub statics: Vec<(Arc<str>, Option<TirExpr>)>,
    
    pub methods: Vec<TirClassMember>,
    
    pub accessors: Vec<TirClassAccessor>,
    
    pub decorators: Vec<TirExpr>,
    
    pub static_blocks: Vec<FnId>,
    
    pub variants: Vec<TirVariantDef>,
}

#[derive(Debug, Clone)]
pub struct TirClassMember {
    pub key: Arc<str>,
    pub func: FnId,
    pub is_static: bool,
    pub is_private: bool,
    
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



#[derive(Debug, Clone)]
pub struct TirImport {
    pub source: Arc<str>,
    pub is_type_only: bool,
    pub specs: Vec<TirImportSpec>,
}




#[derive(Debug, Clone)]
pub struct TirExport {
    pub exported: Arc<str>,
    pub local: Arc<str>,
    
    pub reexport_from: Option<Arc<str>>,
    
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
    
    
    pub global_names: Vec<Arc<str>>,
    
    
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
