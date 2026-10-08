use super::namespaces::collect_decl_names;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, Decl, ExportDecl, Program, StmtKind};
use varn_core::{Atom, AtomInterner};

pub(super) fn collect_exports(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<varn_tir::TirExport> {
    let mut out = Vec::new();
    let mut push = |exported: Arc<str>, local: Arc<str>, from: Option<Arc<str>>, ns: bool| {
        out.push(varn_tir::TirExport {
            exported,
            local,
            reexport_from: from,
            namespace: ns,
        });
    };
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        match d.as_ref() {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => {
                let mut names = FxHashSet::default();
                collect_decl_names(declaration, ast_arena, &mut names, interner);
                for n in names {
                    push(n.clone(), n, None, false);
                }
            }
            Decl::Export(ExportDecl::Named {
                specifiers, source, ..
            }) => {
                for sp in specifiers {
                    push(
                        Arc::from(interner.resolve(sp.exported)),
                        Arc::from(interner.resolve(sp.local)),
                        source.map(|s| Arc::from(interner.resolve(s))),
                        false,
                    );
                }
            }
            Decl::Export(ExportDecl::All {
                source,
                alias: Some(alias),
                ..
            }) => {
                let alias: Arc<str> = Arc::from(interner.resolve(*alias));
                push(
                    alias.clone(),
                    alias,
                    Some(Arc::from(interner.resolve(*source))),
                    true,
                );
            }
            Decl::Export(ExportDecl::Default { .. }) => {
                push(Arc::from("default"), Arc::from("default"), None, false);
            }
            Decl::Variable(_) | Decl::Function(_) | Decl::Class(_) | Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Enum(_) | Decl::Namespace(_) | Decl::Import(_) | Decl::Export(_) | Decl::Extension(_) | Decl::Struct(_) | Decl::SumType(_) => {}
        }
    }
    out
}

pub(super) fn collect_imports(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<varn_tir::TirImport> {
    use varn_core::ast::ImportSpecifier as IS;
    let mut out = Vec::new();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Import(imp) = d.as_ref() else {
            continue;
        };
        let specs = imp
            .specifiers
            .iter()
            .map(|s| {
                let (local, kind) = match s {
                    IS::Default { local, .. } => (
                        Arc::from(interner.resolve(*local)),
                        varn_tir::TirImportKind::Default,
                    ),
                    IS::Namespace { local, .. } => (
                        Arc::from(interner.resolve(*local)),
                        varn_tir::TirImportKind::Namespace,
                    ),
                    IS::Named {
                        local, imported, ..
                    } => (
                        Arc::from(interner.resolve(*local)),
                        varn_tir::TirImportKind::Named(Arc::from(interner.resolve(*imported))),
                    ),
                };
                varn_tir::TirImportSpec { local, kind }
            })
            .collect();
        out.push(varn_tir::TirImport {
            source: Arc::from(interner.resolve(imp.source)),
            is_type_only: imp.is_type,
            specs,
        });
    }
    out
}

pub(super) fn math_intrinsic_imports(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> FxHashMap<Atom, u8> {
    use varn_core::ast::ImportSpecifier;
    let mut out = FxHashMap::default();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Import(imp) = d.as_ref() else {
            continue;
        };
        if interner.resolve(imp.source) != "std:math" {
            continue;
        }
        for spec in &imp.specifiers {
            let ImportSpecifier::Named {
                local, imported, ..
            } = spec
            else {
                continue;
            };
            if let Some(wire) = varn_core::intrinsic_ops::intrinsic_lookup(&format!(
                "std:math/{}",
                interner.resolve(*imported)
            )) {
                out.insert(*local, wire);
            }
        }
    }
    out
}
