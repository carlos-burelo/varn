use std::rc::Rc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, ImportDecl, ImportSpecifier, Pattern};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use crate::module_resolver;
use crate::symbol::{Symbol, SymbolKind};

impl<'r> super::Binder<'r> {
    pub(super) fn bind_import(&mut self, i: &ImportDecl) {
        let source_str = self.interner.resolve(i.source).to_string();
        let in_stdlib_context = self.source_file.starts_with("core:")
            || self.source_file.starts_with("std:")
            || self.source_file.starts_with("runtime:")
            || varn_modules::std_root::in_source_tree(self.source_file.as_ref());
        if (source_str.starts_with("core:") || source_str.starts_with("runtime:"))
            && !in_stdlib_context
        {
            let kind = if source_str.starts_with("core:") {
                "an intrinsic"
            } else {
                "a private runtime"
            };
            self.diagnostics.push(
                Diagnostic::error(
                    ErrorCode::InvalidImportPath,
                    format!(
                        "'{}' is {kind} module and cannot be imported by user code; use 'std:' equivalents",
                        source_str
                    ),
                )
                .with_range(i.range),
            );
            return;
        }

        let is_relative = source_str.starts_with('.') || source_str.starts_with('/');
        let is_package = varn_modules::is_pkg_specifier(&source_str);

        let resolved_target = if (is_relative || is_package) && !self.source_file.is_empty() {
            let base_path = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            if is_package {
                module_resolver::resolve_package_specifier_path(base_path, &source_str)
            } else {
                self.resolver.resolve_specifier(base_path, &source_str)
            }
        } else {
            None
        };

        let is_stdlib = !is_relative && module_resolver::is_known_module(&source_str);

        let relative_exports = if let Some(abs) = &resolved_target {
            let mut visiting = vec![self.source_file.to_string()];
            Some(self.resolver.module_exports(abs, &mut visiting))
        } else {
            None
        };

        let stdlib_exports = if is_stdlib {
            Some(self.resolver.stdlib_exports(&source_str))
        } else {
            None
        };

        if is_relative || is_package {
            if resolved_target.is_none() {
                self.diagnostics.push(
                    Diagnostic::error(
                        ErrorCode::InvalidImportPath,
                        format!("cannot resolve module '{}'", source_str),
                    )
                    .with_range(i.range),
                );
            }
        } else if !is_stdlib {
            self.diagnostics.push(
                Diagnostic::error(
                    ErrorCode::InvalidImportPath,
                    format!("cannot resolve module '{}'", source_str),
                )
                .with_range(i.range),
            );
        }

        for spec in &i.specifiers {
            let (local, imported, line, range) = match spec {
                ImportSpecifier::Named {
                    local,
                    imported,
                    range,
                    ..
                } => (
                    *local,
                    self.interner.resolve(*imported).to_string(),
                    range.start.line,
                    *range,
                ),
                ImportSpecifier::Default { local, range, .. } => {
                    (*local, "default".to_owned(), range.start.line, *range)
                }
                ImportSpecifier::Namespace { local, range, .. } => {
                    (*local, "*".to_owned(), range.start.line, *range)
                }
            };

            let module_path: Option<Rc<str>> = resolved_target
                .clone()
                .or_else(|| {
                    if module_resolver::is_known_module(&source_str) {
                        Some(source_str.clone())
                    } else {
                        None
                    }
                })
                .map(Rc::from);
            let module_path_atom = module_path.as_ref().map(|s| self.intern_local(s));

            let exports_ref: Option<&module_resolver::ExportMap> =
                relative_exports.as_deref().or(stdlib_exports.as_deref());

            let sym = if let Some(exports) = exports_ref {
                if imported == "*" {
                    let mut s = Symbol::new(SymbolKind::Namespace, local, line);
                    s.ty = Some(crate::types::Type::named_with_origin(
                        Rc::from("*"),
                        module_path.clone(),
                        self.resolver,
                        &mut self.ty_table,
                    ));
                    s.origin_module = module_path_atom;
                    s
                } else {
                    match exports.get(&imported) {
                        Some(resolved) => {
                            // `resolved` was bound in whatever module declares
                            // it, against that compilation's view of the
                            // shared `Atom` table at the time — not
                            // necessarily this binder's own `self.interner`,
                            // which can be behind (or, after a nested import
                            // resolution, differently numbered) if this
                            // module's own atoms and the exporter's diverged
                            // before either got published. A raw `.clone()`
                            // would carry its `doc`/`type_params`/
                            // `origin_module`/`re_export_path` `Atom`s
                            // straight through, silently pointing at whatever
                            // text happens to sit at that index in
                            // `self.interner` instead. Round-trip through text
                            // — the same crossing `Symbol::to_cacheable`/
                            // `from_cacheable` exist for when a module
                            // interface goes to disk — decoding against the
                            // resolver's current shared snapshot, which by now
                            // holds everything the exporting bind published.
                            let foreign = self.resolver.interner_snapshot();
                            // `alias_node` is lost here too, same as the
                            // disk-cache round trip `to_cacheable` documents
                            // (this is that same round trip, just in-memory).
                            // An imported type alias's `typeof`/mapped-type
                            // expansion doesn't go through this `Symbol` at
                            // all — it goes through the foreign-module path
                            // in `resolve_type_alias` (`binder/types.rs`),
                            // which reaches the origin module's own bind
                            // instead of this locally-rehydrated copy.
                            // `from_cacheable` mints several new atoms
                            // (name/doc/type_params/...) into `self.interner`
                            // in one batch — resync it to the live table
                            // first, or those atoms number from a stale base
                            // and can collide with whatever a nested import
                            // above just published (see `resync_interner`'s
                            // doc).
                            self.resync_interner();
                            let mut s = Symbol::from_cacheable(
                                resolved.to_cacheable(&foreign),
                                &mut self.interner,
                            );
                            // Same crossing as `foreign`/`self.interner` above,
                            // for `CheckerTyId` instead of `Atom`: `s.ty` (and
                            // any `type_param_constraints`) came from
                            // `resolved`'s own bind, interned against whatever
                            // `CheckerTyTable` that module's binder/checker
                            // grew — not necessarily `self.ty_table`, which is
                            // this binder's own, still-growing-locally table
                            // and can be missing entries the exporter minted
                            // (see `CheckerTyTable::reintern`'s doc for why a
                            // raw id can't just be copied across). Decode
                            // against a fresh snapshot of the resolver's
                            // shared table (which, since the exporting module
                            // fully bound-and-published before this import
                            // could resolve, is guaranteed to contain
                            // everything `resolved.ty` references) and
                            // re-intern into `self.ty_table`.
                            let foreign_ty_table = self.resolver.ty_table_snapshot();
                            let mut ty_cache = rustc_hash::FxHashMap::default();
                            s.ty = s.ty.map(|t| {
                                crate::types::Type(
                                    self.ty_table
                                        .reintern(&foreign_ty_table, t.0, &mut ty_cache),
                                    t.1,
                                )
                            });
                            s.type_param_constraints = s
                                .type_param_constraints
                                .into_iter()
                                .map(|c| {
                                    c.map(|t| {
                                        crate::types::Type(
                                            self.ty_table.reintern(
                                                &foreign_ty_table,
                                                t.0,
                                                &mut ty_cache,
                                            ),
                                            t.1,
                                        )
                                    })
                                })
                                .collect();
                            s.full_range = resolved.full_range;
                            s.name = local;
                            s.line = line;
                            s.original_name = Some(self.intern_local(&imported));
                            s.origin_module = s.origin_module.or(module_path_atom);
                            if let (Some(ref mut ty), Some(origin)) = (&mut s.ty, &s.origin_module)
                            {
                                let origin_rc: Rc<str> = Rc::from(self.interner.resolve(*origin));
                                let origin_atom = self.resolver.intern(&origin_rc);
                                *ty = ty.with_origin(origin_atom, &mut self.ty_table);
                            }
                            // Free-function intrinsic import (e.g. `abs` from
                            // `std:math`): stamp the wire byte now, while the
                            // module specifier is in hand. `origin_module` is the
                            // resolved file path, so the call site can't rebuild
                            // the `std:math/abs` key on its own.
                            if let Some(mp) = &module_path {
                                s.intrinsic_wire = varn_core::intrinsic_ops::intrinsic_lookup(
                                    &format!("{}/{}", mp, imported),
                                );
                            }
                            s
                        }
                        None => {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    ErrorCode::UnknownSymbol,
                                    format!(
                                        "module '{}' has no exported member named '{}'",
                                        source_str, imported
                                    ),
                                )
                                .with_range(range),
                            );
                            let mut s = Symbol::new(SymbolKind::Let, local, line);
                            s.original_name = Some(self.intern_local(&imported));
                            s.origin_module = module_path_atom;
                            s
                        }
                    }
                }
            } else {
                Symbol::new(SymbolKind::Let, local, line)
            };

            self.define(local, sym);
        }
    }

    pub(super) fn bind_export(&mut self, e: &ExportDecl) {
        match e {
            ExportDecl::Decl { declaration, .. } => {
                self.bind_decl(declaration);
                // An exported binding leaves this file's scan, so its
                // element type can no longer be proved from this file alone
                // (`binder::array_evolve`, rule 1). Escaping after binding
                // is enough: candidates finalize at scope exit, never here.
                if let Decl::Variable(v) = declaration.as_ref() {
                    for d in &v.declarators {
                        if let Pattern::Identifier { name, .. } = &d.id {
                            self.escape_array_candidate(*name);
                        }
                    }
                }
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => self.bind_function(f),
                ExportDefaultDecl::Class(c) => self.bind_class(c),
                // The expression is not bound here, so an array candidate
                // named in it would never be seen as a use. Escape the lot.
                ExportDefaultDecl::Expr(_) => self.escape_all_open_array_candidates(),
            },
            // `export { a, b }` names locals without producing identifier
            // expressions the binder would otherwise visit.
            ExportDecl::Named { specifiers, .. } => {
                for s in specifiers {
                    self.escape_array_candidate(s.local);
                }
            }
            ExportDecl::All { .. } => {}
        }
    }
}
