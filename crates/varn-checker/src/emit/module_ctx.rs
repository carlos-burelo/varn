use super::body;
use super::nested_types::NestedTypes;
use super::tables::NameIndex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::AstId;
use varn_core::{Atom, AtomInterner};

pub(super) struct MCtx<'a> {
    pub(super) names: &'a NameIndex,
    pub(super) classes: &'a [varn_tir::ClassInfo],
    pub(super) enums: &'a [varn_tir::EnumInfo],
    pub(super) globals: &'a FxHashMap<Arc<str>, u32>,
    pub(super) fns: &'a FxHashMap<Atom, (u32, u32)>,
    pub(super) decorated_fns: &'a FxHashSet<Atom>,
    pub(super) call_mappings: &'a FxHashMap<AstId, Vec<Option<usize>>>,
    pub(super) desugar: &'a crate::checker::Desugarings,
    pub(super) core_ops: &'a FxHashSet<(Arc<str>, Arc<str>)>,
    pub(super) math_intrinsics: &'a FxHashMap<Atom, u8>,
    pub(super) interner: &'a AtomInterner,
    pub(super) checker_table: &'a crate::types::CheckerTyTable,
    pub(super) nested_types: &'a NestedTypes,
}

impl<'a> MCtx<'a> {
    pub(super) fn as_module_ctx(&self) -> body::ModuleCtx<'a> {
        body::ModuleCtx {
            names: self.names,
            classes: self.classes,
            enums: self.enums,
            globals: self.globals,
            fns: self.fns,
            decorated_fns: self.decorated_fns,
            call_mappings: self.call_mappings,
            desugar: self.desugar,
            core_ops: self.core_ops,
            math_intrinsics: self.math_intrinsics,
            interner: self.interner,
            checker_table: self.checker_table,
            nested_types: self.nested_types,
        }
    }
}
