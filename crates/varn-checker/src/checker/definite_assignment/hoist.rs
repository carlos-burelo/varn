use super::flow::Hoisted;
use super::free_names::{free_reads, Body};
use super::patterns::pattern_names;
use varn_core::ast::{AstArena, Decl, ExportDecl, ExprKind, Pattern, StmtId, StmtKind};

pub(super) fn block_bindings(arena: &AstArena, stmts: &[StmtId]) -> Vec<Hoisted> {
    let mut out = Vec::new();
    for &s in stmts {
        if let StmtKind::Decl(decl) = &arena.stmt(s).kind {
            hoist_decl(arena, decl, &mut out);
        }
    }
    out
}

fn hoist_decl(arena: &AstArena, decl: &Decl, out: &mut Vec<Hoisted>) {
    match decl {
        Decl::Variable(v) => {
            for d in &v.declarators {
                let reads = match (&d.id, d.init) {
                    (Pattern::Identifier { .. }, Some(init)) => match &arena.expr(init).kind {
                        ExprKind::Arrow { params, body, .. } => {
                            Some(free_reads(arena, params, Body::Arrow(body)))
                        }
                        ExprKind::Function { params, body, .. } => {
                            Some(free_reads(arena, params, Body::Stmt(*body)))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                for name in pattern_names(&d.id) {
                    out.push(Hoisted {
                        name,
                        is_binding: true,
                        reads: reads.clone(),
                    });
                }
            }
        }
        Decl::Function(f) => out.push(Hoisted {
            name: f.id,
            is_binding: false,
            reads: Some(free_reads(arena, &f.params, Body::Stmt(f.body))),
        }),
        Decl::Class(c) => {
            if let Some(name) = c.id {
                out.push(Hoisted {
                    name,
                    is_binding: true,
                    reads: None,
                });
            }
        }
        Decl::Enum(e) => out.push(Hoisted {
            name: e.id,
            is_binding: true,
            reads: None,
        }),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => hoist_decl(arena, declaration, out),
        Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Namespace(_)
        | Decl::Import(_)
        | Decl::Export(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => {}
    }
}
