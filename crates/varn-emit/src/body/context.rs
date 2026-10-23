use super::super::nested_types::NestedTypes;
use crate::tables::NameIndex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId};
use varn_core::{Atom, AtomInterner};
use varn_sem::output::TypeEntry;
use varn_tir::{
    BackendTy, ClassId, ClassInfo, EnumId, EnumInfo, LocalId, Signature, TirExpr, TirFunction,
    TirStmt, TyTable,
};

#[derive(Clone, Copy)]
pub(crate) struct ModuleCtx<'a> {
    pub names: &'a NameIndex,
    pub classes: &'a [ClassInfo],
    pub enums: &'a [EnumInfo],

    pub globals: &'a FxHashMap<Arc<str>, u32>,

    pub fns: &'a FxHashMap<Atom, (u32, u32)>,
    pub scoped_fns: &'a FxHashMap<Arc<str>, (u32, u32)>,
    pub namespaces: &'a crate::namespace_scope::NamespaceScope,

    pub decorated_fns: &'a FxHashSet<Atom>,

    pub call_mappings: &'a FxHashMap<AstId, Vec<Option<usize>>>,

    pub desugar: &'a varn_sem::output::Desugarings,

    pub core_ops: &'a FxHashMap<(Arc<str>, Arc<str>), u64>,

    pub math_intrinsics: &'a FxHashMap<Atom, u8>,

    pub interner: &'a AtomInterner,

    pub checker_table: &'a varn_sem::types::CheckerTyTable,

    pub annotation_types: &'a FxHashMap<AstId, varn_sem::types::Type>,
    pub type_names: &'a FxHashMap<AstId, Arc<str>>,

    pub nested_types: &'a NestedTypes,

    pub shadowed: &'a FxHashSet<u32>,

    pub declared_types: &'a FxHashMap<u32, varn_sem::types::Type>,
}

pub(crate) struct FnEmitter<'a> {
    pub ast_arena: &'a AstArena,
    pub expr_table: &'a FxHashMap<AstId, TypeEntry>,
    pub tt: &'a mut TyTable,
    pub(super) m: ModuleCtx<'a>,
    pub signatures: &'a mut Vec<Signature>,

    pub(super) out_closures: &'a mut Vec<TirFunction>,
    pub(super) closure_base: u32,
    pub locals: Vec<BackendTy>,
    pub(super) scopes: Vec<FxHashMap<Arc<str>, LocalId>>,
    pub(super) params: Vec<Arc<str>>,
    pub(super) this_class: Option<ClassId>,

    pub(super) this_enum: Option<EnumId>,

    pub(super) lexical_this: bool,

    pub(super) home_class: Option<ClassId>,

    pub namespace: Option<Arc<str>>,

    pub not_yet_bound: FxHashSet<Arc<str>>,

    pub(super) top_level: bool,

    pub(super) saw_await: bool,

    pub(super) outer_names: FxHashSet<Arc<str>>,

    pub(super) captures: Vec<Arc<str>>,

    pub(super) captured_locals: FxHashSet<u32>,

    pub(super) pending: Vec<TirStmt>,
    pub return_ty: BackendTy,
    pub yield_ty: BackendTy,

    pub(super) disposables: Vec<Vec<TirExpr>>,

    pub(super) loops: Vec<super::loop_labels::LoopFrame>,

    pub(super) pending_label: Option<varn_core::Atom>,

    pub(super) place_target: bool,
}
