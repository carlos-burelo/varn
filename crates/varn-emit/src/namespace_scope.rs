use super::namespaces::scoped_name;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, Decl, ExportDecl, NamespaceDecl, Pattern, Program, StmtKind};
use varn_core::AtomInterner;
use varn_sem::bind::NamespaceImport;
use varn_tir::{TirImport, TirImportKind, TirImportSpec};

pub(super) struct NamespaceScope {
    pub(super) paths: FxHashSet<Arc<str>>,
}

pub(super) fn declared_name(
    names: &FxHashMap<varn_core::ast::AstId, Arc<str>>,
    ast_id: varn_core::ast::AstId,
    bare: &str,
) -> Arc<str> {
    names.get(&ast_id).cloned().unwrap_or_else(|| Arc::from(bare))
}

pub(super) fn scoped_lookup<T>(
    scope: Option<&str>,
    written: &str,
    find: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    let written = written.replace('.', "::");
    let mut rest = scope;
    while let Some(outer) = rest {
        if let Some(found) = find(&format!("{outer}::{written}")) {
            return Some(found);
        }
        rest = outer.rsplit_once("::").map(|(up, _)| up);
    }
    find(&written)
}

pub(super) struct Member {
    pub(super) qualified: Arc<str>,
    pub(super) global: Arc<str>,
}

fn unwrapped(decl: &Decl) -> (&Decl, bool) {
    match decl {
        Decl::Export(ExportDecl::Decl { declaration, .. }) => (declaration.as_ref(), true),
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
        | Decl::Struct(_)
        | Decl::SumType(_) => (decl, false),
    }
}

fn walk(
    ns: &NamespaceDecl,
    outer: Option<&str>,
    visible: bool,
    interner: &AtomInterner,
    scope: &mut NamespaceScope,
    exported: &mut Vec<Member>,
) {
    let path = scoped_name(outer, interner.resolve(ns.id));
    for m in &ns.body {
        let (inner, is_exported) = unwrapped(m);
        let visible = visible && is_exported;
        let mut value = |bare: &str| {
            if visible {
                let qualified = scoped_name(Some(&path), bare);
                exported.push(Member {
                    global: qualified.clone(),
                    qualified,
                });
            }
        };
        match inner {
            Decl::Namespace(inner_ns) => {
                walk(inner_ns, Some(&path), visible, interner, scope, exported)
            }
            Decl::Function(f) => value(interner.resolve(f.id)),
            Decl::Variable(v) => {
                for d in &v.declarators {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        value(interner.resolve(*name));
                    }
                }
            }
            Decl::Class(c) => {
                if let Some(id) = c.id {
                    value(interner.resolve(id));
                }
            }
            Decl::Enum(e) => value(interner.resolve(e.id)),
            Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }
    scope.paths.insert(path);
}

pub(super) fn declared_scope(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> (NamespaceScope, FxHashMap<Arc<str>, Vec<Member>>) {
    let mut scope = NamespaceScope {
        paths: FxHashSet::default(),
    };
    let mut exported = FxHashMap::default();
    for &stmt in &program.body {
        if let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind {
            if let Some(ns) = super::decl_classify::namespace_decl(d) {
                let mut members = Vec::new();
                walk(ns, None, true, interner, &mut scope, &mut members);
                exported.insert(Arc::from(interner.resolve(ns.id)), members);
            }
        }
    }
    (scope, exported)
}

pub(super) fn imported_globals(imports: &[NamespaceImport]) -> Vec<Arc<str>> {
    imports
        .iter()
        .flat_map(|ns| {
            ns.members
                .iter()
                .map(|member| scoped_name(Some(&ns.binding), member))
        })
        .collect()
}

pub(super) fn add_imported_paths(imports: &[NamespaceImport], scope: &mut NamespaceScope) {
    for ns in imports {
        scope.paths.insert(ns.hang.clone());
        scope.paths.insert(ns.binding.clone());
        for member in &ns.members {
            let mut rest = member.as_ref();
            while let Some((outer, _)) = rest.rsplit_once("::") {
                scope.paths.insert(scoped_name(Some(&ns.binding), outer));
                rest = outer;
            }
        }
    }
}

pub(super) fn import_members(imports: &mut [TirImport], namespaces: &[NamespaceImport]) {
    for import in imports.iter_mut().filter(|i| !i.is_type_only) {
        let mut specs = Vec::new();
        for spec in std::mem::take(&mut import.specs) {
            let hung: Vec<&NamespaceImport> = namespaces
                .iter()
                .filter(|ns| ns.hang == spec.local)
                .collect();
            for ns in &hung {
                for member in &ns.members {
                    specs.push(TirImportSpec {
                        local: scoped_name(Some(&ns.binding), member),
                        kind: TirImportKind::Named(scoped_name(Some(&ns.exported), member)),
                        slot: None,
                    });
                }
            }
            if !hung.iter().any(|ns| ns.binding == ns.hang) {
                specs.push(spec);
            }
        }
        import.specs = specs;
    }
}
