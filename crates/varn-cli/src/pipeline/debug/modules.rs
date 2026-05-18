use crate::pipeline::colors::{BLUE, BOLD, C_MODULES, DIM, RESET, YELLOW};
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, ImportSpecifier, Program, Stmt};

pub fn debug_modules(program: &Program) {
    use super::super::colors::{footer, header};
    header(C_MODULES, "module linkage", &program.filename);

    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for stmt in &program.body {
        let Stmt::Decl(decl) = stmt else { continue };
        match decl.as_ref() {
            Decl::Import(imp) => imports.push(imp),
            Decl::Export(exp) => exports.push(exp),
            _ => {}
        }
    }

    if !imports.is_empty() {
        eprintln!("  {BOLD}{BLUE}Imports{RESET}");
        eprintln!("  {DIM}{:<30} │ Symbols{RESET}", "Source");
        eprintln!("  {}", "─".repeat(70));
        for imp in &imports {
            let spec_str = format_import_specifiers(&imp.specifiers);
            eprintln!(
                "  {YELLOW}{:<30}{RESET} │ {spec_str}",
                format!("\"{}\"", imp.source)
            );
        }
        eprintln!();
    }

    if !exports.is_empty() {
        eprintln!("  {BOLD}{BLUE}Exports{RESET}");
        eprintln!("  {DIM}Kind     │ Description{RESET}");
        eprintln!("  {}", "─".repeat(70));
        for exp in &exports {
            let (kind, desc) = format_export_parts(exp);
            eprintln!("  {YELLOW}{:<8}{RESET} │ {desc}", kind);
        }
    }

    footer(
        C_MODULES,
        &format!("{} import(s), {} export(s)", imports.len(), exports.len()),
    );
}

pub fn format_import_specifiers(specs: &[ImportSpecifier]) -> String {
    if specs.is_empty() {
        return format!("{DIM}(side-effect){RESET}");
    }
    let mut named = Vec::new();
    let mut default_name = None;
    let mut ns_name = None;

    for s in specs {
        match s {
            ImportSpecifier::Named { local, .. } => named.push(local.as_str()),
            ImportSpecifier::Default { local, .. } => default_name = Some(local.as_str()),
            ImportSpecifier::Namespace { local, .. } => ns_name = Some(local.as_str()),
        }
    }

    let mut parts = Vec::new();
    if let Some(n) = default_name {
        parts.push(n.to_owned());
    }
    if let Some(ns) = ns_name {
        parts.push(format!("* as {ns}"));
    }
    if !named.is_empty() {
        parts.push(format!("{{ {} }}", named.join(", ")));
    }
    parts.join(", ")
}

fn format_export_parts(exp: &ExportDecl) -> (&'static str, String) {
    match exp {
        ExportDecl::Named {
            specifiers, source, ..
        } => {
            let names: Vec<&str> = specifiers.iter().map(|s| s.exported.as_str()).collect();
            let from = source
                .as_ref()
                .map(|s| format!(" from \"{s}\""))
                .unwrap_or_default();
            ("named", format!("{{ {} }}{}", names.join(", "), from))
        }
        ExportDecl::Default { declaration, .. } => {
            let what = match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => format!("fn {}", f.id),
                ExportDefaultDecl::Class(c) => {
                    format!("class {}", c.id.as_deref().unwrap_or("<anon>"))
                }
                ExportDefaultDecl::Expr(_) => "<expr>".to_owned(),
            };
            ("default", what)
        }
        ExportDecl::All { source, alias, .. } => {
            let alias_str = alias
                .as_ref()
                .map(|a| format!(" as {a}"))
                .unwrap_or_default();
            ("all", format!("* from \"{source}\"{alias_str}"))
        }
        ExportDecl::Decl { declaration, .. } => {
            let name = match declaration.as_ref() {
                Decl::Function(f) => format!("fn {}", f.id),
                Decl::Class(c) => format!("class {}", c.id.as_deref().unwrap_or("<anon>")),
                Decl::Variable(v) => format!("{:?} ...", v.kind),
                Decl::Interface(i) => format!("interface {}", i.id),
                Decl::TypeAlias(t) => format!("type {}", t.id),
                Decl::Enum(e) => format!("enum {}", e.id),
                Decl::Namespace(n) => format!("namespace {}", n.id),
                _ => "<decl>".to_owned(),
            };
            ("decl", name)
        }
    }
}
