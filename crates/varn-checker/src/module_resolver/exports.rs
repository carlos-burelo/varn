use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use std::path::Path;
use std::rc::Rc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, Pattern, Stmt, StmtKind};
use varn_core::Atom;

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
    resolver: &dyn super::ImportResolver,
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
                    let name_str = bind.interner.resolve(name);
                    if let Some(sym) = lookup_global(bind, name_str) {
                        let mut s = sym.clone();
                        s.origin_module = Some(abs_path.to_owned().into());
                        out.insert(name_str.to_string(), s);
                    }
                }
                if let Decl::SumType(st) = declaration.as_ref() {
                    for variant in &st.variants {
                        let variant_name = bind.interner.resolve(variant.name);
                        if let Some(sym) = lookup_global(bind, variant_name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(abs_path.to_owned().into());
                            out.insert(variant_name.to_string(), s);
                        }
                    }
                }
                if let Decl::Enum(e) = declaration.as_ref() {
                    for member in &e.members {
                        let member_name = bind.interner.resolve(member.id);
                        if let Some(sym) = lookup_global(bind, member_name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(abs_path.to_owned().into());
                            out.insert(member_name.to_string(), s);
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
                    let local_name = bind.interner.resolve(spec.local);
                    let exported_name = bind.interner.resolve(spec.exported);
                    if let Some(sym) = lookup_global(bind, local_name) {
                        let mut s = sym.clone();
                        s.name = Rc::from(exported_name);
                        s.origin_module = s
                            .origin_module
                            .take()
                            .or_else(|| Some(abs_path.to_owned().into()));
                        out.insert(exported_name.to_string(), s);
                    }
                }
            }
            ExportDecl::Named {
                specifiers,
                source: Some(src),
                ..
            } => {
                let src_str = bind.interner.resolve(*src);
                let src_exports = if super::paths::is_known_module(src_str) {
                    resolver.stdlib_exports(src_str)
                } else {
                    let src_abs = super::paths::resolve_relative(resolver, base_dir, src_str);
                    resolver.record_dep(abs_path, &src_abs);
                    resolver.module_exports(&src_abs, visiting)
                };
                for spec in specifiers {
                    let local_name = bind.interner.resolve(spec.local);
                    let exported_name = bind.interner.resolve(spec.exported);
                    if let Some(sym) = src_exports.get(local_name) {
                        let mut s = sym.clone();
                        s.name = Rc::from(exported_name);
                        s.re_export_path.push(abs_path.to_owned().into());
                        out.insert(exported_name.to_string(), s);
                    }
                }
            }
            ExportDecl::All {
                source,
                alias: None,
                ..
            } => {
                let source_str = bind.interner.resolve(*source);
                let src_exports = if super::paths::is_known_module(source_str) {
                    resolver.stdlib_exports(source_str)
                } else {
                    let src_abs = super::paths::resolve_relative(resolver, base_dir, source_str);
                    resolver.record_dep(abs_path, &src_abs);
                    resolver.module_exports(&src_abs, visiting)
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
                let source_str = bind.interner.resolve(*source);
                let ns_str = bind.interner.resolve(*ns);
                let src_abs = if super::paths::is_known_module(source_str) {
                    source_str.to_string()
                } else {
                    let src_abs = super::paths::resolve_relative(resolver, base_dir, source_str);
                    resolver.record_dep(abs_path, &src_abs);
                    src_abs
                };
                let src_exports = if super::paths::is_known_module(source_str) {
                    resolver.stdlib_exports(source_str)
                } else {
                    resolver.module_exports(&src_abs, visiting)
                };
                let mut ns_sym = Symbol::new(SymbolKind::Namespace, Rc::from(ns_str), 0);
                ns_sym.ty = Some(Type::named_with_origin("*", Some(src_abs.clone())));
                ns_sym.origin_module = Some(src_abs.into());
                for (sub_name, sub_sym) in src_exports.iter() {
                    let mut s = sub_sym.clone();
                    s.re_export_path.push(abs_path.to_owned().into());
                    out.insert(format!("{ns_str}.{sub_name}"), s);
                }
                out.insert(ns_str.to_string(), ns_sym);
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    let fn_name = bind.interner.resolve(f.id);
                    if let Some(sym) = lookup_global(bind, fn_name) {
                        let mut s = sym.clone();
                        s.name = "default".into();
                        out.insert("default".into(), s);
                    }
                }
                ExportDefaultDecl::Class(c) => {
                    if let Some(id) = &c.id {
                        let class_name = bind.interner.resolve(*id);
                        if let Some(sym) = lookup_global(bind, class_name) {
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

fn decl_primary_name(decl: &Decl) -> Option<Atom> {
    match decl {
        Decl::Variable(v) => v.declarators.first().and_then(|d| match &d.id {
            Pattern::Identifier { name, .. } => Some(*name),
            _ => None,
        }),
        Decl::Function(f) => Some(f.id),
        Decl::Class(c) => c.id,
        Decl::Enum(e) => Some(e.id),
        Decl::Interface(i) => Some(i.id),
        Decl::TypeAlias(t) => Some(t.id),
        Decl::Namespace(n) => Some(n.id),
        Decl::Struct(s) => Some(s.id),
        Decl::SumType(s) => Some(s.id),
        _ => None,
    }
}

pub(super) fn lookup_global<'a>(bind: &'a BindResult, name: &str) -> Option<&'a Symbol> {
    let scope = bind.scopes.get(bind.global_scope);
    scope.bindings.get(name).map(|&id| bind.arena.get(id))
}
