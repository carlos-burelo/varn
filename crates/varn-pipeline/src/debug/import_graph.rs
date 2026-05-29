use varn_core::ast::{Decl, ExportDecl, Program, Stmt};
use crate::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_MODULES};
use super::modules::format_import_specifiers;

pub fn debug_import_graph(program: &Program) {
    use super::colors::{footer, header};

    struct Edge {
        source: String,
        line: u32,
        specifiers: String,
        is_reexport: bool,
    }

    let mut edges: Vec<Edge> = Vec::new();
    for stmt in &program.body {
        let Stmt::Decl(decl) = stmt else { continue };
        match decl.as_ref() {
            Decl::Import(imp) => {
                edges.push(Edge {
                    source: imp.source.clone(),
                    line: imp.range.start.line,
                    specifiers: format_import_specifiers(&imp.specifiers),
                    is_reexport: false,
                });
            }
            Decl::Export(exp) => match exp {
                ExportDecl::All { source, alias, range, .. } => {
                    let specs = alias.as_ref().map(|a| format!("* as {a}")).unwrap_or_else(|| "*".to_owned());
                    edges.push(Edge { source: source.clone(), line: range.start.line, specifiers: specs, is_reexport: true });
                }
                ExportDecl::Named { specifiers, source: Some(src), range, .. } => {
                    let names: Vec<String> = specifiers.iter().map(|s| {
                        if s.local == s.exported { s.local.clone() } else { format!("{} as {}", s.local, s.exported) }
                    }).collect();
                    edges.push(Edge { source: src.clone(), line: range.start.line, specifiers: format!("{{ {} }}", names.join(", ")), is_reexport: true });
                }
                _ => {}
            },
            _ => {}
        }
    }

    header(C_MODULES, "dependency graph", &program.filename);

    if edges.is_empty() {
        eprintln!("  {DIM}(no external dependencies){RESET}");
        footer(C_MODULES, "0 edges");
        return;
    }

    eprintln!("  {BOLD}{BLUE}○ {}{RESET}", program.filename);
    
    let total = edges.len();
    for (i, edge) in edges.iter().enumerate() {
        let is_last = i == total - 1;
        let marker = if is_last { "└── " } else { "├── " };
        let re_flag = if edge.is_reexport { format!(" {YELLOW}[re-export]{RESET}") } else { String::new() };
        
        eprintln!(
            "  {DIM}{marker}{RESET}{BOLD}\"{}\"{RESET} {DIM}ln:{} │ {RESET}{}{}",
            edge.source, edge.line, edge.specifiers, re_flag
        );
    }

    footer(C_MODULES, &format!("{} dependencies resolved", total));
}
