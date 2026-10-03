use super::super::nested_types::NestedTypes;
use crate::checker::TypeEntry;
use crate::emit::tables::NameIndex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId};
use varn_core::{Atom, AtomInterner};
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

    pub decorated_fns: &'a FxHashSet<Atom>,

    pub call_mappings: &'a FxHashMap<AstId, Vec<Option<usize>>>,

    pub desugar: &'a crate::checker::Desugarings,

    pub core_ops: &'a FxHashSet<(Arc<str>, Arc<str>)>,

    pub math_intrinsics: &'a FxHashMap<Atom, u8>,

    pub interner: &'a AtomInterner,

    pub checker_table: &'a crate::types::CheckerTyTable,

    pub annotation_types: &'a FxHashMap<AstId, crate::types::Type>,

    pub nested_types: &'a NestedTypes,
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

    pub(super) top_level: bool,

    pub(super) saw_await: bool,

    pub(super) outer_names: FxHashSet<Arc<str>>,

    pub(super) captures: Vec<Arc<str>>,

    pub(super) pending: Vec<TirStmt>,

    pub(super) disposables: Vec<Vec<TirExpr>>,
}
