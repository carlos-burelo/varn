use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use std::path::Path;
use std::rc::Rc;
use varn_core::ast::{AstArena, Decl, ExportDecl, ExportDefaultDecl, Pattern, StmtId, StmtKind};
use varn_core::Atom;

/// `Symbol::origin_module` must name the module that DECLARES the export —
/// almost never a string `bind`'s own parse ever interned (an absolute file
/// path or `std:`-style specifier is compiler-internal bookkeeping, not
/// source text). `bind.interner` is an owned snapshot with no mutable access
/// here, so it cannot mint the atom itself: a `.get`-only lookup against it
/// silently fell back to `Atom::default()` (index 0) on the near-guaranteed
/// miss, handing every export an `origin_module` that resolved to whatever
/// text happened to occupy slot 0 of whichever interner later decoded it —
/// the cross-module "property does not exist" regression across the whole
/// stdlib (`Color`, `Pointer`, `MarkdownNode`, ... all imported types).
///
/// `resolver` is always the live, shared `AtomInterner` every module's bind
/// publishes into (see `ImportResolver::intern`), so interning `s` there
/// mints (or reuses) a real, resolvable `Atom` instead of guessing.
fn intern_origin(resolver: &dyn super::ImportResolver, s: &str) -> Atom {
    resolver.intern(s)
}

/// Re-home an exported symbol to the table of the module that DECLARES it.
///
/// A symbol imported into this module had its `ty` re-interned into THIS
/// module's `CheckerTyTable` (`binder/imports.rs`), so those ids are only
/// meaningful here. Re-exporting that symbol would leak local ids to
/// downstream importers, which decode against the declaring module's table —
/// and an id that is in range but means something else there (an observed
/// `Sender` reading as a union) silently types against the wrong shape. When
/// the declaring module is reachable, hand out ITS symbol instead.
///
/// `None` when the symbol is declared here, or when the re-export chain does
/// not expose a declaring module to fetch from — the caller keeps its clone.
fn rehome_to_declaring_module(
    resolver: &dyn super::ImportResolver,
    bind: &BindResult,
    visiting: &mut Vec<String>,
    sym: &Symbol,
) -> Option<Symbol> {
    let origin = bind.interner.try_resolve(sym.origin_module?)?;
    let original = bind.interner.try_resolve(sym.original_name?)?;
    if origin == bind.source_file.as_ref() {
        return None;
    }
    let map = if super::paths::is_known_module(origin) {
        resolver.stdlib_exports(origin)
    } else {
        resolver.module_exports(origin, visiting)
    };
    map.get(original).cloned()
}

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
    stmts: &[StmtId],
    ast_arena: &AstArena,
    bind: &BindResult,
    abs_path: &str,
    base_dir: &Path,
    visiting: &mut Vec<String>,
    out: &mut ExportMap,
) {
    for &stmt_id in stmts {
        let StmtKind::Decl(decl) = &ast_arena.stmt(stmt_id).kind else {
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
                        s.origin_module = Some(intern_origin(resolver, abs_path));
                        out.insert(name_str.to_string(), s);
                    }
                }
                if let Decl::SumType(st) = declaration.as_ref() {
                    for variant in &st.variants {
                        let variant_name = bind.interner.resolve(variant.name);
                        if let Some(sym) = lookup_global(bind, variant_name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(intern_origin(resolver, abs_path));
                            out.insert(variant_name.to_string(), s);
                        }
                    }
                }
                if let Decl::Enum(e) = declaration.as_ref() {
                    for member in &e.members {
                        let member_name = bind.interner.resolve(member.id);
                        if let Some(sym) = lookup_global(bind, member_name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(intern_origin(resolver, abs_path));
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
                        let mut s = rehome_to_declaring_module(resolver, bind, visiting, sym)
                            .unwrap_or_else(|| sym.clone());
                        s.name = spec.exported;
                        s.origin_module = s
                            .origin_module
                            .take()
                            .or_else(|| Some(intern_origin(resolver, abs_path)));
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
                        let mut s = rehome_to_declaring_module(resolver, bind, visiting, sym)
                            .unwrap_or_else(|| sym.clone());
                        s.name = spec.exported;
                        s.re_export_path.push(intern_origin(resolver, abs_path));
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
                        let mut s = rehome_to_declaring_module(resolver, bind, visiting, sym)
                            .unwrap_or_else(|| sym.clone());
                        s.re_export_path.push(intern_origin(resolver, abs_path));
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
                let mut ns_sym = Symbol::new(SymbolKind::Namespace, *ns, 0);
                // Mint straight into the LIVE type table: this symbol travels
                // through the `ExportMap` to other modules, which decode its
                // `ty` against the live table's id space. A snapshot-then-
                // publish would leave the id pointing into a local lineage
                // (`set_ty_table` merges, it does not adopt indices).
                let name_atom = resolver.intern("*");
                let origin_atom = resolver.intern(src_abs.as_str());
                let ns_ty = resolver.intern_ty(varn_core::TypeKind::Named(
                    name_atom,
                    Some(origin_atom),
                ));
                ns_sym.ty = Some(Type(ns_ty, false));
                ns_sym.origin_module = Some(intern_origin(resolver, &src_abs));
                for (sub_name, sub_sym) in src_exports.iter() {
                    let mut s = rehome_to_declaring_module(resolver, bind, visiting, sub_sym)
                        .unwrap_or_else(|| sub_sym.clone());
                    s.re_export_path.push(intern_origin(resolver, abs_path));
                    out.insert(format!("{ns_str}.{sub_name}"), s);
                }
                out.insert(ns_str.to_string(), ns_sym);
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    let fn_name = bind.interner.resolve(f.id);
                    if let Some(sym) = lookup_global(bind, fn_name) {
                        let mut s = sym.clone();
                        s.name = intern_origin(resolver, "default");
                        out.insert("default".into(), s);
                    }
                }
                ExportDefaultDecl::Class(c) => {
                    if let Some(id) = &c.id {
                        let class_name = bind.interner.resolve(*id);
                        if let Some(sym) = lookup_global(bind, class_name) {
                            let mut s = sym.clone();
                            s.name = intern_origin(resolver, "default");
                            out.insert("default".into(), s);
                        }
                    }
                }
                ExportDefaultDecl::Expr(_expr) => {
                    let mut s = Symbol::new(SymbolKind::Let, intern_origin(resolver, "default"), 0);
                    s.origin_module = Some(intern_origin(resolver, abs_path));
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
    let atom = bind.interner.get(name)?;
    scope.bindings.get(&atom).map(|&id| bind.arena.get(id))
}
