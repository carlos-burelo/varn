use std::collections::HashSet;
use varn_core::ast::*;
use varn_core::AtomInterner;

pub fn collect_imports(program: &Program, interner: &AtomInterner) -> HashSet<String> {
    let mut collector = ImportCollector::new(interner);
    collector.visit_program(program);
    collector.imports
}

struct ImportCollector<'a> {
    imports: HashSet<String>,
    interner: &'a AtomInterner,
}

impl<'a> ImportCollector<'a> {
    fn new(interner: &'a AtomInterner) -> Self {
        Self {
            imports: HashSet::new(),
            interner,
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
                self.imports
                    .insert(self.interner.resolve(import.source).to_owned());
            }
            Decl::Export(export) => match export {
                ExportDecl::Named {
                    source: Some(src), ..
                } => {
                    self.imports.insert(self.interner.resolve(*src).to_owned());
                }
                ExportDecl::Named { .. } => {}
                ExportDecl::All { source, .. } => {
                    self.imports
                        .insert(self.interner.resolve(*source).to_owned());
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
