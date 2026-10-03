//! The checker emits TIR.
//!
//! The replacement for `checker_annotations/`: instead of walking the AST and
//! noting types in a side map, it builds a `varn_tir::TirModule` whose every
//! node carries its type and resolution as mandatory fields. See
//! `docs/TIR_CONTRATO_TIPADO.md`.
//!
//! The whole executable AST lowers: literals, `Var` (local / param / global /
//! upvalue), every operator, member and index access, calls (direct / vtable /
//! by-name), `new`, enum construction and matching, collections, closures,
//! `async` / generators, all loop forms, `switch`, `try`, destructuring,
//! templates. A construct with no precise TIR shape (a host intrinsic, a
//! spread the arity rule can't see, an iterator-protocol `for…of`) still emits
//! real nodes typed `Dynamic(Unannotated)` — never a bare hole.

mod body;
mod class_members;
mod classes;
mod decl_classify;
mod decorators;
mod enums;
mod functions;
mod imports_exports;
mod module_ctx;
mod module_globals;
mod module_top;
mod namespaces;
mod nested_types;
mod prelude;
mod tables;
mod ty;

pub use ty::{lower_type, NameResolver, NoNames};

use crate::binder::BindResult;
use crate::checker::TypeEntry;
use imports_exports::{collect_exports, collect_imports, math_intrinsic_imports};
use module_ctx::MCtx;
use namespaces::emit_extensions;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, Program};
use varn_tir::{BackendTy, SigId, TirFunction, TirModule, TyTable};

/// Build the TIR for one module from the same four inputs
/// `collect_type_annotations` consumes. Nothing else: a datum the checker does
/// not expose here is a gap in the checker, to be closed there.
pub fn emit_module(
    program: &Program,
    ast_arena: &AstArena,
    bind: &BindResult,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    call_mappings: &FxHashMap<AstId, Vec<Option<usize>>>,
    desugar: &crate::checker::Desugarings,
) -> TirModule {
    let interner = &bind.interner;
    let mut types = TyTable::default();
    ty::prime(&mut types);

    let tables::Tables {
        classes,
        enums,
        mut signatures,
        names,
    } = tables::build(bind, &desugar.foreign_enums, &mut types);

    let mut declared = module_globals::collect_declared(program, ast_arena, interner);
    let prelude = prelude::prelude_imports(&program.filename, &declared, ast_arena, interner);
    for import in &prelude {
        declared.extend(import.specs.iter().map(|spec| spec.local.clone()));
    }

    let module_globals::GlobalSlots {
        slots: global_slots,
        globals,
        names: global_names,
        nested,
        first_nested_ordinal,
    } = module_globals::assign_global_slots(
        program, ast_arena, bind, &mut types, &names, &declared,
    );

    let core_ops = module_globals::core_method_ops(bind);
    let math_intrinsics = math_intrinsic_imports(program, ast_arena, interner);

    let functions::FreeFunctions {
        list: free_fns,
        index: fn_index,
        decorated: decorated_fns,
    } = functions::collect_free_functions(program, ast_arena, interner);

    let ctx = MCtx {
        names: &names,
        classes: &classes,
        enums: &enums,
        globals: &global_slots,
        fns: &fn_index,
        decorated_fns: &decorated_fns,
        call_mappings,
        desugar,
        core_ops: &core_ops,
        math_intrinsics: &math_intrinsics,
        interner,
        checker_table: &bind.ty_table,
        annotation_types: &bind.annotation_types,
        nested_types: &nested,
    };

    let n_free = free_fns.len() as u32;
    let mut functions: Vec<TirFunction> = Vec::new();
    let mut closures: Vec<TirFunction> = Vec::new();

    for (f, ns) in &free_fns {
        let tf = functions::emit_function(
            f,
            ns.as_deref(),
            ast_arena,
            bind,
            expr_table,
            &mut types,
            &ctx,
            &mut signatures,
            &mut closures,
            n_free,
        );
        functions.push(tf);
    }

    let tl_base = n_free;
    let (top_locals, top_has_await, top_body) = module_top::lower_top_level(
        program,
        ast_arena,
        &ctx,
        &fn_index,
        &global_slots,
        interner,
        &mut types,
        &mut signatures,
        expr_table,
        &mut closures,
        tl_base,
    );
    let top_level = TirFunction {
        name: Arc::from("<module>"),
        sig: SigId(0),
        params: vec![],
        return_ty: BackendTy::Void,
        locals: top_locals,
        body: top_body,
        has_this: false,
        this_class: None,
        is_async: top_has_await,
        is_generator: false,
        has_rest: false,
    };

    functions.extend(closures);

    let class_defs = classes::build_class_defs(
        program,
        ast_arena,
        bind,
        &ctx,
        expr_table,
        &mut types,
        &mut signatures,
        &mut functions,
        &nested,
        first_nested_ordinal,
    );

    emit_extensions(
        program,
        ast_arena,
        &ctx,
        expr_table,
        &mut types,
        &mut signatures,
        &mut functions,
    );

    let mut imports = prelude;
    imports.extend(collect_imports(program, ast_arena, interner));
    let exports = collect_exports(program, ast_arena, interner);

    TirModule {
        source_file: Arc::from(program.filename.as_ref()),
        imports,
        exports,
        types,
        classes,
        enums,
        signatures,
        functions,
        globals,
        global_names,
        class_defs,
        top_level,
    }
}
