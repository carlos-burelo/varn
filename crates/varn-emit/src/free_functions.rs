use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, FunctionDecl, Param, Pattern, Program, StmtKind};
use varn_core::{Atom, AtomInterner};

pub(super) fn param_name(p: &Param, interner: &AtomInterner) -> Arc<str> {
    match &p.pattern {
        Pattern::Identifier { name, .. } => Arc::from(interner.resolve(*name)),
        Pattern::Array { .. }
        | Pattern::Object { .. }
        | Pattern::Assignment { .. }
        | Pattern::Rest { .. } => Arc::from("_"),
    }
}

pub(super) struct FreeFunctions<'a> {
    pub(super) list: Vec<(&'a FunctionDecl, Option<Arc<str>>)>,
    pub(super) index: FxHashMap<Atom, (u32, u32)>,
    pub(super) decorated: FxHashSet<Atom>,
}

pub(super) fn collect_free_functions<'a>(
    program: &Program,
    ast_arena: &'a AstArena,
    interner: &'a AtomInterner,
    shadowed: &FxHashSet<u32>,
) -> FreeFunctions<'a> {
    use super::decl_classify::{free_function, namespace_decl};
    use varn_core::ast::NamespaceDecl;
    fn ns_member_fns<'a>(
        ns: &'a NamespaceDecl,
        interner: &AtomInterner,
        out: &mut Vec<(&'a FunctionDecl, Option<Arc<str>>)>,
    ) {
        let ns_name: Arc<str> = Arc::from(interner.resolve(ns.id));
        for m in &ns.body {
            let inner = match m {
                varn_core::ast::Decl::Export(varn_core::ast::ExportDecl::Decl {
                    declaration,
                    ..
                }) => declaration.as_ref(),
                other @ varn_core::ast::Decl::Variable(_)
                | other @ varn_core::ast::Decl::Function(_)
                | other @ varn_core::ast::Decl::Class(_)
                | other @ varn_core::ast::Decl::Interface(_)
                | other @ varn_core::ast::Decl::TypeAlias(_)
                | other @ varn_core::ast::Decl::Enum(_)
                | other @ varn_core::ast::Decl::Namespace(_)
                | other @ varn_core::ast::Decl::Import(_)
                | other @ varn_core::ast::Decl::Export(_)
                | other @ varn_core::ast::Decl::Extension(_)
                | other @ varn_core::ast::Decl::Struct(_)
                | other @ varn_core::ast::Decl::SumType(_) => other,
            };
            match inner {
                varn_core::ast::Decl::Function(f) => out.push((f, Some(ns_name.clone()))),
                varn_core::ast::Decl::Namespace(inner_ns) => ns_member_fns(inner_ns, interner, out),
                varn_core::ast::Decl::Variable(_)
                | varn_core::ast::Decl::Class(_)
                | varn_core::ast::Decl::Interface(_)
                | varn_core::ast::Decl::TypeAlias(_)
                | varn_core::ast::Decl::Enum(_)
                | varn_core::ast::Decl::Import(_)
                | varn_core::ast::Decl::Export(_)
                | varn_core::ast::Decl::Extension(_)
                | varn_core::ast::Decl::Struct(_)
                | varn_core::ast::Decl::SumType(_) => {}
            }
        }
    }
    let mut free_fns: Vec<(&FunctionDecl, Option<Arc<str>>)> = program
        .body
        .iter()
        .filter_map(|&s| match &ast_arena.stmt(s).kind {
            StmtKind::Decl(d) => free_function(d).map(|f| (f, None)),
            StmtKind::Block { .. }
            | StmtKind::Empty
            | StmtKind::Expr { .. }
            | StmtKind::Error
            | StmtKind::If { .. }
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::ForIn { .. }
            | StmtKind::ForOf { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Return { .. }
            | StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Throw { .. }
            | StmtKind::Try { .. }
            | StmtKind::Using { .. }
            | StmtKind::Labeled { .. }
            | StmtKind::Debugger => None,
        })
        .collect();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        if let Some(ns) = namespace_decl(d) {
            ns_member_fns(ns, interner, &mut free_fns);
        }
    }
    let mut fn_index: FxHashMap<Atom, (u32, u32)> = FxHashMap::default();
    for (i, (f, _)) in free_fns.iter().enumerate() {
        fn_index
            .entry(f.id)
            .or_insert((i as u32, f.params.len() as u32));
    }
    let decorated_fns: FxHashSet<Atom> = free_fns
        .iter()
        .filter(|(f, ns)| {
            ns.is_none()
                && f.decorators.iter().any(|d| {
                    !varn_core::ast::decorators::is_active_builtin(
                        ast_arena,
                        interner,
                        |off| shadowed.contains(&off),
                        d,
                    )
                })
        })
        .map(|(f, _)| f.id)
        .collect();
    FreeFunctions {
        list: free_fns,
        index: fn_index,
        decorated: decorated_fns,
    }
}
