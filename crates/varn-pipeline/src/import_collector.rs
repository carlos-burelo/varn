use std::collections::HashSet;
use varn_core::ast::*;

pub fn collect_imports(program: &Program) -> HashSet<String> {
    let mut collector = ImportCollector::new();
    collector.visit_program(program);
    collector.imports
}

struct ImportCollector {
    imports: HashSet<String>,
}

impl ImportCollector {
    fn new() -> Self {
        Self {
            imports: HashSet::new(),
        }
    }

    fn visit_program(&mut self, program: &Program) {
        for stmt in &program.body {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        if let StmtKind::Decl(decl) = &stmt.kind {
            self.visit_decl(decl)
        }
    }

    fn visit_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Import(import) => {
                self.imports.insert(import.source.to_string());
            }
            Decl::Export(export) => match export {
                ExportDecl::Named {
                    source: Some(src), ..
                } => {
                    self.imports.insert(src.to_string());
                }
                ExportDecl::Named { .. } => {}
                ExportDecl::All { source, .. } => {
                    self.imports.insert(source.to_string());
                }
                _ => {}
            },
            Decl::Namespace(ns) => {
                for decl in &ns.body {
                    self.visit_decl(decl);
                }
            }
            _ => {}
        }
    }
}
