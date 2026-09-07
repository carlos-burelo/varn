//! The checker emits TIR.
//!
//! This is the stage-2 replacement for `checker_annotations/`: instead of
//! walking the AST and noting types in a side map, it builds a
//! `varn_tir::TirModule` whose every node carries its type and resolution as
//! mandatory fields. See `docs/TIR_ETAPA_2_PLAN.md`.
//!
//! Sub-phase 1 (this file): the skeleton. Tables are empty, every body is a
//! single `Dynamic(NotYetSupported)` placeholder. It exists so the pipeline
//! wiring, the verifier pass and `vn debug -p tir:check` land against real
//! corpus modules before any lowering logic is written. Coverage is ~0 % by
//! construction — that is the baseline the later sub-phases move.

mod ty;

pub use ty::lower_type;

use crate::binder::BindResult;
use crate::checker::TypeEntry;
use crate::module_resolver::ImportResolver;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{AstId, Program};
use varn_tir::{
    BackendTy, DynReason, Resolution, Signature, Span, TirExpr, TirExprKind, TirFunction, TirModule,
    TirStmt, TyTable,
};

/// Build the TIR for one module from the same four inputs
/// `collect_type_annotations` consumes. Nothing else: a datum the checker does
/// not expose here is a gap in the checker, to be closed there.
pub fn emit_module(
    program: &Program,
    _bind: &BindResult,
    _resolver: &dyn ImportResolver,
    _expr_table: &FxHashMap<AstId, TypeEntry>,
) -> TirModule {
    let types = TyTable::default();

    // One empty signature; every function points at it until sub-phase "tables"
    // fills the real ones.
    let signatures = vec![Signature { params: vec![], return_ty: BackendTy::Void }];

    let top_level = stub_function("<module>");

    TirModule {
        source_file: Rc::from(program.filename.as_ref()),
        types,
        classes: vec![],
        enums: vec![],
        signatures,
        functions: vec![],
        globals: vec![],
        top_level,
    }
}

/// A function whose body is one placeholder statement. Not `NotYetSupported`
/// on a real node yet — there is no node — just an expression statement whose
/// type names the reason, so the coverage counter has something to count.
fn stub_function(name: &str) -> TirFunction {
    let placeholder = TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Dynamic(DynReason::NotYetSupported),
        res: Resolution::None,
        span: Span::EMPTY,
    };
    TirFunction {
        name: Rc::from(name),
        sig: varn_tir::SigId(0),
        params: vec![],
        return_ty: BackendTy::Void,
        locals: vec![],
        body: vec![TirStmt::Expr(placeholder)],
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_tir::{verify_module, Coverage};

    fn empty_module() -> TirModule {
        TirModule {
            source_file: Rc::from("t.vn"),
            types: TyTable::default(),
            classes: vec![],
            enums: vec![],
            signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
            functions: vec![],
            globals: vec![],
            top_level: stub_function("<module>"),
        }
    }

    #[test]
    fn the_skeleton_verifies() {
        let m = empty_module();
        assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
    }

    #[test]
    fn the_skeleton_reports_zero_static_coverage() {
        let m = empty_module();
        let c = Coverage::of(&m);
        assert_eq!(c.dynamic_by_reason(DynReason::NotYetSupported), 1);
        assert_eq!(c.static_dispatch, 0);
    }
}
