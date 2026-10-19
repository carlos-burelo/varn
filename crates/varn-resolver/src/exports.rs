use std::path::Path;
use std::sync::Arc;
use varn_core::ast::{AstArena, Decl, ExportDecl, ExportDefaultDecl, Pattern, StmtId, StmtKind};
use varn_core::Atom;
use varn_sem::bind::BindResult;
use varn_sem::exports::ExportMap;
use varn_sem::symbol::{Symbol, SymbolKind};
use varn_sem::types::Type;

fn intern_origin(out: &mut ExportMap, s: &str) -> Atom {
    Arc::make_mut(&mut out.table).intern_name(s)
}

fn adopt_exports(out: &mut ExportMap, from: &ExportMap) {
    Arc::make_mut(&mut out.table).absorb(&from.table);
}

fn rehome_to_declaring_module(
    resolver: &dyn varn_sem::resolver::ImportResolver,
    bind: &BindResult,
    visiting: &mut Vec<String>,
    sym: &Symbol,
    out: &mut ExportMap,
) -> Option<Symbol> {
    let text = |a: Atom| {
        bind.interner
            .try_resolve(a)
            .or_else(|| out.table.name(a))
            .map(str::to_owned)
    };
    let origin = text(sym.origin_module?)?;
    let original = text(sym.original_name?)?;
    if origin == bind.source_file.as_ref() {
        return None;
    }
    let map = if varn_binder::paths::is_known_module(&origin) {
        resolver.stdlib_exports(&origin)
    } else {
        resolver.module_exports(&origin, visiting)
    };
    let found = map.get(&original).cloned();
    if found.is_some() {
        adopt_exports(out, &map);
    }
    found
}

fn origin_text(bind: &BindResult, out: &ExportMap, atom: Option<Atom>, fallback: &str) -> String {
    atom.and_then(|a| {
        bind.interner
            .try_resolve(a)
            .or_else(|| out.table.name(a))
            .map(str::to_owned)
    })
    .unwrap_or_else(|| fallback.to_owned())
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
    resolver: &dyn varn_sem::resolver::ImportResolver,
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
                for name in decl_export_names(declaration) {
                    let name_str = bind.interner.resolve(name);
                    if let Some(sym) = lookup_global(bind, name_str) {
                        let mut s = sym.clone();
                        s.origin_module = Some(intern_origin(out, abs_path));
                        out.insert(name_str.to_string(), s);
                    }
                }
                if let Decl::Namespace(ns) = declaration.as_ref() {
                    let name = bind.interner.resolve(ns.id);
                    crate::namespace_exports::export_declared(bind, name, name, abs_path, out);
                }
                if let Decl::SumType(st) = declaration.as_ref() {
                    for variant in &st.variants {
                        let variant_name = bind.interner.resolve(variant.name);
                        if let Some(sym) = lookup_global(bind, variant_name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(intern_origin(out, abs_path));
                            out.insert(variant_name.to_string(), s);
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
                        let mut s = rehome_to_declaring_module(resolver, bind, visiting, sym, out)
                            .unwrap_or_else(|| sym.clone());
                        s.name = spec.exported;
                        let declared_here = s.origin_module.is_none();
                        s.origin_module = s
                            .origin_module
                            .take()
                            .or_else(|| Some(intern_origin(out, abs_path)));
                        out.insert(exported_name.to_string(), s);
                        if declared_here {
                            crate::namespace_exports::export_declared(
                                bind,
                                local_name,
                                exported_name,
                                abs_path,
                                out,
                            );
                        }
                        let carried: Vec<(Arc<str>, Vec<Arc<str>>)> = bind
                            .namespace_imports
                            .iter()
                            .filter(|ns| ns.binding.as_ref() == local_name)
                            .map(|ns| (ns.exported.clone(), ns.members.clone()))
                            .collect();
                        for (root, members) in carried {
                            crate::namespace_exports::carry_imported(
                                bind,
                                sym,
                                &root,
                                &members,
                                exported_name,
                                out,
                            );
                        }
                    }
                }
            }
            ExportDecl::Named {
                specifiers,
                source: Some(src),
                ..
            } => {
                let src_str = bind.interner.resolve(*src);
                let src_exports = if varn_binder::paths::is_known_module(src_str) {
                    resolver.stdlib_exports(src_str)
                } else {
                    let src_abs = varn_binder::paths::resolve_relative(resolver, base_dir, src_str);
                    resolver.record_dep(abs_path, &src_abs);
                    resolver.module_exports(&src_abs, visiting)
                };
                adopt_exports(out, &src_exports);
                for spec in specifiers {
                    let local_name = bind.interner.resolve(spec.local);
                    let exported_name = bind.interner.resolve(spec.exported);
                    if let Some(sym) = src_exports.get(local_name) {
                        let mut s = rehome_to_declaring_module(resolver, bind, visiting, sym, out)
                            .unwrap_or_else(|| sym.clone());
                        s.name = spec.exported;
                        s.re_export_path.push(intern_origin(out, abs_path));
                        out.insert(exported_name.to_string(), s);
                        crate::namespace_exports::forward(
                            &src_exports,
                            local_name,
                            exported_name,
                            out,
                        );
                    }
                }
            }
            ExportDecl::All {
                source,
                alias: None,
                ..
            } => {
                let source_str = bind.interner.resolve(*source);
                let src_exports = if varn_binder::paths::is_known_module(source_str) {
                    resolver.stdlib_exports(source_str)
                } else {
                    let src_abs =
                        varn_binder::paths::resolve_relative(resolver, base_dir, source_str);
                    resolver.record_dep(abs_path, &src_abs);
                    resolver.module_exports(&src_abs, visiting)
                };
                adopt_exports(out, &src_exports);
                let mut sorted_names: Vec<String> = src_exports.keys().cloned().collect();
                sorted_names.sort();
                for name in sorted_names {
                    if let Some(kept) = out.get(&name) {
                        let kept_from = origin_text(bind, out, kept.origin_module, abs_path);
                        let dropped_from = src_exports
                            .get(&name)
                            .map(|s| origin_text(bind, out, s.origin_module, source_str))
                            .unwrap_or_else(|| source_str.to_owned());
                        out.record_collision(name.clone(), kept_from, dropped_from);
                        continue;
                    }
                    let sym = src_exports.get(&name).expect("sorted export key missing");
                    let mut s = rehome_to_declaring_module(resolver, bind, visiting, sym, out)
                        .unwrap_or_else(|| sym.clone());
                    s.re_export_path.push(intern_origin(out, abs_path));
                    out.insert(name.clone(), s);
                }
            }
            ExportDecl::All {
                source,
                alias: Some(ns),
                ..
            } => {
                let source_str = bind.interner.resolve(*source);
                let ns_str = bind.interner.resolve(*ns);
                let src_abs = if varn_binder::paths::is_known_module(source_str) {
                    source_str.to_string()
                } else {
                    let src_abs =
                        varn_binder::paths::resolve_relative(resolver, base_dir, source_str);
                    resolver.record_dep(abs_path, &src_abs);
                    src_abs
                };
                let src_exports = if varn_binder::paths::is_known_module(source_str) {
                    resolver.stdlib_exports(source_str)
                } else {
                    resolver.module_exports(&src_abs, visiting)
                };
                let mut ns_sym = Symbol::new(SymbolKind::Namespace, *ns, 0);
                adopt_exports(out, &src_exports);
                let table = Arc::make_mut(&mut out.table);
                let name_atom = table.intern_name("*");
                let origin_atom = table.intern_name(src_abs.as_str());
                let ns_ty = table.intern(varn_core::TypeKind::Named(name_atom, Some(origin_atom)));
                ns_sym.ty = Some(Type::resolved(ns_ty));
                ns_sym.origin_module = Some(intern_origin(out, &src_abs));
                if let Some(kept) = out.get(ns_str) {
                    let kept_from = origin_text(bind, out, kept.origin_module, abs_path);
                    out.record_collision(ns_str.to_string(), kept_from, src_abs.clone());
                } else {
                    out.insert(ns_str.to_string(), ns_sym);
                }
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    let fn_name = bind.interner.resolve(f.id);
                    if let Some(sym) = lookup_global(bind, fn_name) {
                        let mut s = sym.clone();
                        s.name = intern_origin(out, "default");
                        out.insert("default".into(), s);
                    }
                }
                ExportDefaultDecl::Class(c) => {
                    if let Some(id) = &c.id {
                        let class_name = bind.interner.resolve(*id);
                        if let Some(sym) = lookup_global(bind, class_name) {
                            let mut s = sym.clone();
                            s.name = intern_origin(out, "default");
                            out.insert("default".into(), s);
                        }
                    }
                }
                ExportDefaultDecl::Expr(_expr) => {
                    let mut s = Symbol::new(SymbolKind::Let, intern_origin(out, "default"), 0);
                    s.origin_module = Some(intern_origin(out, abs_path));
                    out.insert("default".into(), s);
                }
            },
        }
    }
}

fn decl_export_names(decl: &Decl) -> Vec<Atom> {
    match decl {
        Decl::Variable(v) => v
            .declarators
            .iter()
            .filter_map(|d| match &d.id {
                Pattern::Identifier { name, .. } => Some(*name),
                Pattern::Array { .. }
                | Pattern::Object { .. }
                | Pattern::Assignment { .. }
                | Pattern::Rest { .. } => None,
            })
            .collect(),
        Decl::Function(f) => vec![f.id],
        Decl::Class(c) => c.id.into_iter().collect(),
        Decl::Enum(e) => vec![e.id],
        Decl::Interface(i) => vec![i.id],
        Decl::TypeAlias(t) => vec![t.id],
        Decl::Namespace(n) => vec![n.id],
        Decl::Struct(s) => vec![s.id],
        Decl::SumType(s) => vec![s.id],
        Decl::Import(_) | Decl::Export(_) | Decl::Extension(_) => Vec::new(),
    }
}

pub(super) fn lookup_global<'a>(bind: &'a BindResult, name: &str) -> Option<&'a Symbol> {
    let scope = bind.scopes.get(bind.global_scope);
    let atom = bind.interner.get(name)?;
    scope.bindings.get(&atom).map(|&id| bind.arena.get(id))
}
