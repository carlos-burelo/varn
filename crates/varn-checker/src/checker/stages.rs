use super::profile;
use super::recorder::Recorder;
use super::Checker;
use std::sync::Arc;
use varn_binder::Binder;
use varn_core::ast::{AstArena, Program};
use varn_sem::output::{CheckProfile, CheckResult};

pub(super) fn prepare(
    program: &Program,
    ast_arena: &AstArena,
    interner: varn_core::AtomInterner,
    resolver: &dyn varn_sem::resolver::ImportResolver,
    profile: &mut CheckProfile,
) -> (varn_sem::bind::BindResult, Arc<str>) {
    let globals_ref = profile::timed(&mut profile.load_globals, || {
        varn_binder::core::loader::module_globals(&program.filename, resolver)
    });

    let mut bind = profile::timed(&mut profile.bind, || match globals_ref {
        Some(globals) => {
            Binder::bind_with_global_refs(program, ast_arena, interner, resolver, &globals)
        }
        None => Binder::bind(program, ast_arena, interner, resolver),
    });

    profile::timed(&mut profile.merge_core_members, || {
        varn_binder::core::loader::merge_core_members(&mut bind, resolver);
    });

    profile::timed(&mut profile.enrich_call_returns, || {
        super::enrich_call_returns(&mut bind, ast_arena, resolver);
    });

    let source_file: Arc<str> = Arc::from(bind.source_file.as_ref());
    (bind, source_file)
}

pub(super) fn init_checker<'r>(
    resolver: &'r dyn varn_sem::resolver::ImportResolver,
    ast_arena: &'r AstArena,
    source_file: Arc<str>,
    bind: &varn_sem::bind::BindResult,
    profile: &mut CheckProfile,
) -> Checker<'r> {
    profile::timed(&mut profile.init, || {
        let mut checker = Checker::new(
            resolver,
            ast_arena,
            source_file,
            bind.global_scope,
            bind.ty_table.clone(),
        );

        for (name, class_info) in &bind.type_members.classes {
            if class_info.is_abstract {
                checker.abstract_classes.insert(name.clone());
            }
        }
        checker
    })
}

pub(super) fn run_pass(
    checker: &mut Checker<'_>,
    rec: &mut Recorder,
    program: &Program,
    bind: &mut varn_sem::bind::BindResult,
    profile: &mut CheckProfile,
) {
    profile::timed(&mut profile.check_stmts, || {
        checker.check_stmts(rec, &program.body, bind);
        checker.check_definite_assignment(program, bind);
    });

    if rec.enabled {
        rec.project_expr_types(bind);
    }

    rec.desugar.foreign_inherited_fields = checker.collect_foreign_inherited_fields(bind);
    bind.ty_table = checker.ty_table.clone();
    bind.interner.absorb(checker.ty_table.names());

    rec.desugar.foreign_enums =
        checker.collect_foreign_enums(bind, rec.expr_table.values().map(|entry| &entry.ty));
}

pub(super) fn assemble(
    checker: Checker<'_>,
    rec: Recorder,
    mut bind: varn_sem::bind::BindResult,
    ast_arena: &AstArena,
    source_file: Arc<str>,
    profile: &mut CheckProfile,
) -> CheckResult {
    let mut rec = rec;
    let mut final_diagnostics = std::mem::take(&mut bind.diagnostics);
    final_diagnostics.extend(checker.diagnostics);
    for (kept, rejected) in bind.interner.collisions() {
        final_diagnostics.emit(varn_core::Diagnostic::error(
            varn_core::ErrorCode::CompilationInternalError,
            format!("name hash collision: '{kept}' and '{rejected}' share an Atom"),
        ));
    }

    let expr_table = std::mem::take(&mut rec.expr_table);
    let mut test_targets = Vec::new();
    for sym in bind.arena.all() {
        if sym.is_test && sym.origin_module.is_none() {
            test_targets.push(varn_sem::output::TestTarget {
                name: Arc::from(bind.interner.resolve(sym.name)),
                file: source_file.clone(),
                is_async: sym.is_async,
            });
        }
    }
    if !final_diagnostics.has_errors() {
        if let Some(&id) = expr_table
            .iter()
            .filter(|(_, entry)| entry.ty.is_error())
            .map(|(id, _)| id)
            .min()
        {
            let mut diag = varn_core::Diagnostic::error(
                varn_core::ErrorCode::CompilationInternalError,
                "type resolution failed here without reporting why",
            );
            if let Some(node) = ast_arena.exprs().nth(id as usize) {
                diag = diag.with_range(node.range);
            }
            final_diagnostics.emit(diag);
        }
    }
    profile.collect_annotations = std::time::Duration::ZERO;
    let flattened = std::mem::take(&mut bind.type_members.flattened);

    profile::timed(&mut profile.finalize, || {
        for (sid, ty) in &rec.symbol_types {
            let sym = bind.arena.get_mut(*sid);

            if sym.origin_module.is_some() {
                continue;
            }

            let current_is_weak = match &sym.ty {
                None => true,
                Some(t) => {
                    t.is_dynamic()
                        || match checker.ty_table.get(t.0) {
                            varn_core::TypeKind::Fn(fid) => varn_sem::types::Type::resolved(
                                checker.ty_table.get_function(fid).return_type,
                            )
                            .is_dynamic(),
                            varn_core::TypeKind::Primitive(_)
                            | varn_core::TypeKind::Builtin(_)
                            | varn_core::TypeKind::Literal(_)
                            | varn_core::TypeKind::This
                            | varn_core::TypeKind::Array(_)
                            | varn_core::TypeKind::Union(_)
                            | varn_core::TypeKind::Intersection(_)
                            | varn_core::TypeKind::Tuple(_)
                            | varn_core::TypeKind::Named(..)
                            | varn_core::TypeKind::Generic(..)
                            | varn_core::TypeKind::TemplateLiteral(_)
                            | varn_core::TypeKind::Object(_)
                            | varn_core::TypeKind::Typeof(_)
                            | varn_core::TypeKind::KeyOf(_)
                            | varn_core::TypeKind::IndexedAccess { .. }
                            | varn_core::TypeKind::Mapped { .. }
                            | varn_core::TypeKind::Conditional { .. }
                            | varn_core::TypeKind::Infer(_)
                            | varn_core::TypeKind::EnumVariant { .. }
                            | varn_core::TypeKind::TypePredicate { .. } => false,
                        }
                }
            };

            if !sym.has_explicit_type && current_is_weak {
                sym.ty = Some(*ty);
            }
        }
    });

    let (symbol_types, node_scopes, scope_spans) = profile::timed(&mut profile.cleanup, || {
        let symbol_types = rec.symbol_types.clone();
        let node_scopes = if rec.enabled {
            rec.node_scopes.clone()
        } else {
            rustc_hash::FxHashMap::default()
        };
        let scope_spans = if rec.enabled {
            std::mem::take(&mut rec.scope_spans)
        } else {
            Vec::new()
        };
        (symbol_types, node_scopes, scope_spans)
    });

    CheckResult {
        bind,
        diagnostics: final_diagnostics,
        expr_types: rec.expr_types,
        flattened_members: flattened,
        profile: std::mem::take(profile),
        node_scopes,
        scope_spans,
        symbol_types,
        member_resolutions: rec.member_resolutions,
        call_resolutions: rec.call_resolutions,
        match_gaps: rec.match_gaps,
        expr_table,
        call_mappings: rec.call_mappings,
        desugar: rec.desugar,
        test_targets,
    }
}
