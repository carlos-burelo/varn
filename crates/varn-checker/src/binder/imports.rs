use std::sync::Arc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, ImportDecl, ImportSpecifier, Pattern};
use varn_core::{Diagnostic, ErrorCode};

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

            let module_path: Option<Arc<str>> = resolved_target
                .clone()
                .or_else(|| {
                    if module_resolver::is_known_module(&source_str) {
                        Some(source_str.clone())
                    } else {
                        None
                    }
                })
                .map(Arc::from);
            let module_path_atom = module_path.as_ref().map(|s| self.intern_local(s));

            let exports_ref: Option<&module_resolver::ExportMap> =
                relative_exports.as_deref().or(stdlib_exports.as_deref());

            let sym = if let Some(exports) = exports_ref {
                if imported == "*" {
                    let mut s = Symbol::new(SymbolKind::Namespace, local, line);
                    s.ty = Some(crate::types::Type::named_with_origin(
                        Arc::from("*"),
                        module_path.clone(),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ));
                    s.origin_module = module_path_atom;
                    s
                } else {
                    match exports.get(&imported) {
                        Some(resolved) => {
                            // Cross the module boundary ONCE, through the same
                            // portable codec the on-disk interface uses
                            // (`types/portable.rs`): every `Atom` resolves to
                            // text and every `CheckerTyId` to the shape in its
                            // OWNER's table, then both re-intern into this
                            // binder's interner/table. A raw clone would carry
                            // the text and the id straight through, pointing at
                            // whatever sits at the same index here.
                            //
                            // `resync_interner` before the batch and
                            // `publish_interner_tail` after: `decode_symbol`
                            // mints several atoms in a row with no resolver
                            // call in between, and the live table must not
                            // diverge from this one (see `resync_interner`).
                            self.resync_interner();
                            let foreign = self.resolver.interner_snapshot();
                            let fallback_table = self.resolver.ty_table_snapshot();
                            let portable = crate::module_resolver::cache::encode_symbol(
                                resolved,
                                &fallback_table,
                                &foreign,
                                Some(self.resolver),
                            );
                            let mut s = crate::module_resolver::cache::decode_symbol(
                                portable,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                &mut self.interner,
                            );
                            self.publish_interner_tail();
                            s.full_range = resolved.full_range;
                            s.name = local;
                            s.line = line;
                            // The rehydrated symbol keeps the declaring
                            // module's alias NODE, not just its expanded
                            // `ty`: an imported alias (`Json` from
                            // `std:encoding`) must still be expandable in
                            // THIS module's context, so its body's names are
                            // resolved against — and interned into — the
                            // local type table instead of being read out of
                            // the exporter's, where the ids are foreign.
                            s.alias_node = resolved.alias_node.clone();
                            s.original_name = Some(self.intern_local(&imported));
                            s.origin_module = s.origin_module.or(module_path_atom);
                            if let (Some(ref mut ty), Some(origin)) = (&mut s.ty, &s.origin_module)
                            {
                                let origin_rc: Arc<str> = Arc::from(self.interner.resolve(*origin));
                                let origin_atom = self.resolver.intern(&origin_rc);
                                *ty = ty.with_origin(
                                    origin_atom,
                                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                );
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
