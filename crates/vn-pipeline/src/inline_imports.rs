use crate::ModuleResolver;
use rustc_hash::FxHashSet;
use std::path::Path;
use std::sync::Arc;
use vn_core::ast::{Decl, ImportDecl, ImportSpecifier, StmtKind};
use vn_core::AtomInterner;
use vn_sem::resolver::ImportResolver;
use vn_sem::symbol::InlineSrc;

struct Inlined {
    local: String,
    src: Arc<InlineSrc>,
}

struct Rewrite {
    start: usize,
    end: usize,
    text: String,
}

pub(crate) fn expand(source: &str, path: &str, resolver: &ModuleResolver) -> Option<String> {
    let (program, arena, interner) = crate::quiet_parse::parse_module(source, path).ok()?;
    let base_dir = Path::new(path).parent().unwrap_or(Path::new("."));
    let mut rewrites: Vec<Rewrite> = Vec::new();
    let mut appended = String::new();
    let mut emitted: FxHashSet<String> = FxHashSet::default();

    for &stmt in &program.body {
        let StmtKind::Decl(decl) = &arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Import(imp) = decl.as_ref() else {
            continue;
        };
        if imp.is_type {
            continue;
        }
        let specifier = interner.resolve(imp.source);
        let Some(exports) = exports_of(resolver, base_dir, specifier) else {
            continue;
        };
        let mut inlined: Vec<Inlined> = Vec::new();
        let mut kept: Vec<&ImportSpecifier> = Vec::new();
        for spec in &imp.specifiers {
            let found = match spec {
                ImportSpecifier::Named {
                    local, imported, ..
                } => exports
                    .get(interner.resolve(*imported))
                    .and_then(|sym| sym.inline_src.clone())
                    .map(|src| Inlined {
                        local: interner.resolve(*local).to_owned(),
                        src,
                    }),
                ImportSpecifier::Default { .. } | ImportSpecifier::Namespace { .. } => None,
            };
            match found {
                Some(found) => inlined.push(found),
                None => kept.push(spec),
            }
        }
        if inlined.is_empty() {
            continue;
        }
        let start = imp.range.start.offset as usize;
        let end = imp.range.end.offset as usize;
        let original = source.get(start..end)?;
        let mut text = if kept.is_empty() && exports.inert {
            String::new()
        } else {
            import_text(imp, &kept, &interner)
        };
        text.push_str(&"\n".repeat(original.matches('\n').count()));
        rewrites.push(Rewrite { start, end, text });
        for found in inlined {
            render(&found.src, &found.local, &mut emitted, &mut appended);
        }
    }
    if rewrites.is_empty() {
        return None;
    }
    let mut out = source.to_owned();
    for rewrite in rewrites.iter().rev() {
        out.replace_range(rewrite.start..rewrite.end, &rewrite.text);
    }
    out.push_str(&appended);
    Some(out)
}

fn exports_of(
    resolver: &ModuleResolver,
    base_dir: &Path,
    specifier: &str,
) -> Option<Arc<vn_sem::exports::ExportMap>> {
    use vn_core::ImportSpecifier as Kind;
    match Kind::parse(specifier) {
        Kind::Stdlib(_) | Kind::Core(_) => vn_modules::resolver::is_known_module(specifier)
            .then(|| resolver.stdlib_exports(specifier)),
        Kind::Relative(_) => {
            let raw = resolver.resolve_specifier(base_dir, specifier)?;
            let abs = vn_modules::canonical_or_original(Path::new(&raw));
            Some(resolver.module_exports(&abs, &mut Vec::new()))
        }
        Kind::Runtime(_) | Kind::Package(_) => None,
    }
}

fn render(src: &InlineSrc, name: &str, emitted: &mut FxHashSet<String>, out: &mut String) {
    let mut edits: Vec<(usize, usize, String)> =
        vec![(src.name_offset as usize, src.name.len(), name.to_owned())];
    for dep in &src.deps {
        let mangled = format!(
            "{}${:06x}",
            dep.src.name,
            vn_core::fingerprint::fingerprint(dep.src.module.as_bytes()) & 0xff_ffff
        );
        for &offset in &dep.offsets {
            edits.push((offset as usize, dep.src.name.len(), mangled.clone()));
        }
        if emitted.insert(mangled.clone()) {
            render(&dep.src, &mangled, emitted, out);
        }
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut text = src.text.to_string();
    for (at, len, replacement) in edits {
        text.replace_range(at..at + len, &replacement);
    }
    out.push('\n');
    out.push_str(&text);
    out.push('\n');
}

fn import_text(imp: &ImportDecl, kept: &[&ImportSpecifier], interner: &AtomInterner) -> String {
    let source = interner.resolve(imp.source);
    let mut default = None;
    let mut namespace = None;
    let mut named: Vec<String> = Vec::new();
    for spec in kept {
        match spec {
            ImportSpecifier::Default { local, .. } => default = Some(interner.resolve(*local)),
            ImportSpecifier::Namespace { local, .. } => {
                namespace = Some(interner.resolve(*local));
            }
            ImportSpecifier::Named {
                local, imported, ..
            } => {
                let (local, imported) = (interner.resolve(*local), interner.resolve(*imported));
                named.push(if local == imported {
                    local.to_owned()
                } else {
                    format!("{imported} as {local}")
                });
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(local) = default {
        parts.push(local.to_owned());
    }
    if let Some(local) = namespace {
        parts.push(format!("* as {local}"));
    }
    if !named.is_empty() {
        parts.push(format!("{{ {} }}", named.join(", ")));
    }
    if parts.is_empty() {
        format!("import {source:?}")
    } else {
        format!("import {} from {source:?}", parts.join(", "))
    }
}
