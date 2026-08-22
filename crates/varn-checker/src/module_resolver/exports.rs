use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use std::path::Path;
use std::rc::Rc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, Pattern, Stmt, StmtKind};

pub(super) fn assign_slots(exports: &mut ExportMap) {
    let mut keys: Vec<String> = exports.keys().cloned().collect();
    keys.sort();
    for (idx, key) in keys.iter().enumerate() {
        if let Some(sym) = exports.get_mut(key) {
            sym.slot_idx = Some(idx);
        }
    }
}

pub(super) fn collect_exports(
    stmts: &[Stmt],
    bind: &BindResult,
    abs_path: &str,
    base_dir: &Path,
    visiting: &mut Vec<String>,
    out: &mut ExportMap,
) {
    for stmt in stmts {
        let StmtKind::Decl(decl) = &stmt.kind else {
            continue;
        };
        let Decl::Export(e) = decl.as_ref() else {
            continue;
        };

        match e {
            ExportDecl::Decl { declaration, .. } => {
                if let Some(name) = decl_primary_name(declaration) {
                    if let Some(sym) = lookup_global(bind, &name) {
                        let mut s = sym.clone();
                        s.origin_module = Some(abs_path.to_owned().into());
                        out.insert(name.to_string(), s);
                    }
                }
                if let Decl::SumType(st) = declaration.as_ref() {
                    for variant in &st.variants {
                        if let Some(sym) = lookup_global(bind, &variant.name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(abs_path.to_owned().into());
                            out.insert(variant.name.to_string(), s);
                        }
                    }
                }
            }
            ExportDecl::Named {
                specifiers,
                source: None,
                ..
            } => {
                for spec in specifiers {
                    if let Some(sym) = lookup_global(bind, &spec.local) {
                        let mut s = sym.clone();
                        s.name = spec.exported.clone();
                        s.origin_module = s
                            .origin_module
                            .take()
                            .or_else(|| Some(abs_path.to_owned().into()));
                        out.insert(spec.exported.to_string(), s);
                    }
                }
            }
            ExportDecl::Named {
                specifiers,
                source: Some(src),
                ..
            } => {
                let src_exports = if super::paths::is_known_module(src) {
                    super::stdlib::resolve_stdlib_module_exports_ref(src)
                } else {
                    let src_abs = super::paths::resolve_relative(base_dir, src);
                    super::store::record_dep(abs_path, &src_abs);
                    super::resolve_module_exports_ref(&src_abs, visiting)
                };
                for spec in specifiers {
                    if let Some(sym) = src_exports.get(&spec.local.to_string()) {
                        let mut s = sym.clone();
                        s.name = spec.exported.clone();
                        s.re_export_path.push(abs_path.to_owned().into());
                        out.insert(spec.exported.to_string(), s);
                    }
                }
            }
            ExportDecl::All {
                source,
                alias: None,
                ..
            } => {
                let src_exports = if super::paths::is_known_module(source) {
                    super::stdlib::resolve_stdlib_module_exports_ref(source)
                } else {
                    let src_abs = super::paths::resolve_relative(base_dir, source);
                    super::store::record_dep(abs_path, &src_abs);
                    super::resolve_module_exports_ref(&src_abs, visiting)
                };
                for (name, sym) in src_exports.iter() {
                    out.entry(name.clone()).or_insert_with(|| {
                        let mut s = sym.clone();
                        s.re_export_path.push(abs_path.to_owned().into());
                        s
                    });
                }
            }
            ExportDecl::All {
                source,
                alias: Some(ns),
                ..
            } => {
                let src_abs = if super::paths::is_known_module(source) {
                    source.to_string()
                } else {
                    let src_abs = super::paths::resolve_relative(base_dir, source);
                    super::store::record_dep(abs_path, &src_abs);
                    src_abs
                };
                let src_exports = if super::paths::is_known_module(source) {
                    super::stdlib::resolve_stdlib_module_exports_ref(source)
                } else {
                    super::resolve_module_exports_ref(&src_abs, visiting)
                };
                let mut ns_sym = Symbol::new(SymbolKind::Namespace, ns.clone(), 0);
                ns_sym.ty = Some(Type::named_with_origin("*", Some(src_abs.clone())));
                ns_sym.origin_module = Some(src_abs.into());
                for (sub_name, sub_sym) in src_exports.iter() {
                    let mut s = sub_sym.clone();
                    s.re_export_path.push(abs_path.to_owned().into());
                    out.insert(format!("{ns}.{sub_name}"), s);
                }
                out.insert(ns.to_string(), ns_sym);
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    if let Some(sym) = lookup_global(bind, &f.id) {
                        let mut s = sym.clone();
                        s.name = "default".into();
                        out.insert("default".into(), s);
                    }
                }
                ExportDefaultDecl::Class(c) => {
                    if let Some(id) = &c.id {
                        if let Some(sym) = lookup_global(bind, id) {
                            let mut s = sym.clone();
                            s.name = "default".into();
                            out.insert("default".into(), s);
                        }
                    }
                }
                ExportDefaultDecl::Expr(_expr) => {
                    let mut s = Symbol::new(SymbolKind::Let, "default".into(), 0);
                    s.origin_module = Some(abs_path.to_owned().into());
                    out.insert("default".into(), s);
                }
            },
        }
    }
}

fn decl_primary_name(decl: &Decl) -> Option<Rc<str>> {
    match decl {
        Decl::Variable(v) => v.declarators.first().and_then(|d| match &d.id {
            Pattern::Identifier { name, .. } => Some(name.clone()),
            _ => None,
        }),
        Decl::Function(f) => Some(f.id.clone()),
        Decl::Class(c) => c.id.clone(),
        Decl::Enum(e) => Some(e.id.clone()),
        Decl::Interface(i) => Some(i.id.clone()),
        Decl::TypeAlias(t) => Some(t.id.clone()),
        Decl::Namespace(n) => Some(n.id.clone()),
        Decl::Struct(s) => Some(s.id.clone()),
        Decl::SumType(s) => Some(s.id.clone()),
        _ => None,
    }
}

pub(super) fn lookup_global<'a>(bind: &'a BindResult, name: &str) -> Option<&'a Symbol> {
    let scope = bind.scopes.get(bind.global_scope);
    scope.bindings.get(name).map(|&id| bind.arena.get(id))
}
