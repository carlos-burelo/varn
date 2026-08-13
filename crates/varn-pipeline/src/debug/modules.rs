use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, ImportSpecifier, Program, Stmt};
use varn_term::chalk::chalk;
use varn_term::terminal::Section;

pub fn debug_modules(program: &Program) {
    Section::new("module linkage").subtitle(&program.filename).color(|c| c.cyan()).print();

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
        varn_term::terminal::log(format!("  {}", chalk("Imports").blue().bold()));
        varn_term::terminal::log(format!("  {}", chalk(format!("{:<30} │ Symbols", "Source")).dim()));
        varn_term::terminal::log(format!("  {}", "─".repeat(70)));
        for imp in &imports {
            let spec_str = format_import_specifiers(&imp.specifiers);
            varn_term::terminal::log(format!(
                "  {} │ {spec_str}",
                chalk(format!("{:<30}", format!("\"{}\"", imp.source))).yellow(),
            ));
        }
        varn_term::terminal::blank();
    }

    if !exports.is_empty() {
        varn_term::terminal::log(format!("  {}", chalk("Exports").blue().bold()));
        varn_term::terminal::log(format!("  {}", chalk("Kind     │ Description").dim()));
        varn_term::terminal::log(format!("  {}", "─".repeat(70)));
        for exp in &exports {
            let (kind, desc) = format_export_parts(exp);
            varn_term::terminal::log(format!("  {} │ {desc}", chalk(format!("{:<8}", kind)).yellow()));
        }
    }

    varn_term::terminal::log(
        chalk(format!("── {} import(s), {} export(s) ──", imports.len(), exports.len())).dim(),
    );
}

pub fn format_import_specifiers(specs: &[ImportSpecifier]) -> String {
    if specs.is_empty() {
        return chalk("(side-effect)").dim().to_string();
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
