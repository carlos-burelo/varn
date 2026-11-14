use super::namespaces::collect_decl_names;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use vn_core::ast::{AstArena, Decl, ExportDecl, Program, StmtKind};
use vn_core::{Atom, AtomInterner};

pub(super) fn collect_exports(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
    namespaces: &FxHashMap<Arc<str>, Vec<super::namespace_scope::Member>>,
) -> Vec<vn_tir::TirExport> {
    let mut out = Vec::new();
    let mut push = |exported: Arc<str>, local: Arc<str>, from: Option<Arc<str>>, ns: bool| {
        out.push(vn_tir::TirExport {
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
                if let Decl::Namespace(ns) = declaration.as_ref() {
                    let name = interner.resolve(ns.id);
                    for member in namespaces.get(name).into_iter().flatten() {
                        push(member.qualified.clone(), member.global.clone(), None, false);
                    }
                    continue;
                }
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
                    let local = interner.resolve(sp.local);
                    let exported = interner.resolve(sp.exported);
                    if let (None, Some(members)) = (source, namespaces.get(local)) {
                        for member in members {
                            let rest = &member.qualified[local.len()..];
                            push(
                                Arc::from(format!("{exported}{rest}")),
                                member.global.clone(),
                                None,
                                false,
                            );
                        }
                        continue;
                    }
                    push(
                        Arc::from(exported),
                        Arc::from(local),
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
            Decl::Variable(_)
            | Decl::Function(_)
            | Decl::Class(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Enum(_)
            | Decl::Namespace(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::SumType(_) => {}
        }
    }
    out
}

pub(super) fn collect_imports(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<vn_tir::TirImport> {
    use vn_core::ast::ImportSpecifier as IS;
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
                        vn_tir::TirImportKind::Default,
                    ),
                    IS::Namespace { local, .. } => (
                        Arc::from(interner.resolve(*local)),
                        vn_tir::TirImportKind::Namespace,
                    ),
                    IS::Named {
                        local, imported, ..
                    } => (
                        Arc::from(interner.resolve(*local)),
                        vn_tir::TirImportKind::Named(Arc::from(interner.resolve(*imported))),
                    ),
                };
                vn_tir::TirImportSpec {
                    local,
                    kind,
                    slot: None,
                    variants: Vec::new(),
                }
            })
            .collect();
        out.push(vn_tir::TirImport {
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
    use vn_core::ast::ImportSpecifier;
    let mut out = FxHashMap::default();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Import(imp) = d.as_ref() else {
            continue;
        };
        let source = interner.resolve(imp.source);
        for spec in &imp.specifiers {
            let ImportSpecifier::Named {
                local, imported, ..
            } = spec
            else {
                continue;
            };
            if let Some(wire) =
                vn_core::intrinsic_ops::intrinsic_lookup(source, interner.resolve(*imported))
            {
                out.insert(*local, wire);
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeTarget {
    pub(crate) op: u64,
    pub(crate) max_args: Option<u8>,
    pub(crate) typed: Option<u64>,
}

pub(super) fn native_targets(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
    bind: &vn_sem::bind::BindResult,
) -> FxHashMap<Arc<str>, NativeTarget> {
    use vn_core::ast::ImportSpecifier;
    let mut out = FxHashMap::default();
    let Some(provider) = vn_modules::provider::get() else {
        return out;
    };
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Import(imp) = d.as_ref() else {
            continue;
        };
        let source = interner.resolve(imp.source);
        for spec in &imp.specifiers {
            let ImportSpecifier::Named {
                local, imported, ..
            } = spec
            else {
                continue;
            };
            if let Some(op) = provider.native_op_id(source, "", interner.resolve(*imported)) {
                out.insert(
                    Arc::from(interner.resolve(*local)),
                    NativeTarget {
                        op,
                        max_args: None,
                        typed: None,
                    },
                );
            }
        }
    }
    for (local, forward) in &bind.imported_forwards {
        if let Some(op) = provider.native_op_id(&forward.module, "", &forward.symbol) {
            out.insert(
                local.clone(),
                NativeTarget {
                    op,
                    max_args: Some(forward.args),
                    typed: forward.typed.as_deref().map(vn_core::runtime_ops::op_id),
                },
            );
        }
    }
    out
}
