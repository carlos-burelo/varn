use std::rc::Rc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, ImportDecl, ImportSpecifier, Pattern};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use crate::module_resolver;
use crate::symbol::{Symbol, SymbolKind};

impl super::Binder {
    pub(super) fn bind_import(&mut self, i: &ImportDecl) {
        let in_stdlib_context = self.source_file.starts_with("core:")
            || self.source_file.starts_with("std:")
            || varn_modules::std_root::in_source_tree(self.source_file.as_ref());
        if i.source.starts_with("core:") && !in_stdlib_context {
            self.diagnostics.push(
                Diagnostic::error(
                    ErrorCode::InvalidImportPath,
                    format!(
                        "'{}' is an intrinsic module and cannot be imported; use 'std:' equivalents",
                        i.source
                    ),
                )
                .with_range(i.range),
            );
            return;
        }

        let is_relative = i.source.starts_with('.') || i.source.starts_with('/');
        let is_package = varn_modules::is_pkg_specifier(&i.source);

        let resolved_target = if (is_relative || is_package) && !self.source_file.is_empty() {
            let base_path = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            if is_package {
                module_resolver::resolve_package_specifier_path(base_path, &i.source)
            } else {
                module_resolver::resolve_specifier_path(base_path, &i.source)
            }
        } else {
            None
        };

        let is_stdlib = !is_relative && module_resolver::is_known_module(&i.source);

        let relative_exports = if let Some(abs) = &resolved_target {
            let mut visiting = vec![self.source_file.to_string()];
            Some(module_resolver::resolve_module_exports_ref(
                abs,
                &mut visiting,
            ))
        } else {
            None
        };

        let stdlib_exports = if is_stdlib {
            Some(module_resolver::resolve_stdlib_module_exports_ref(
                &i.source,
            ))
        } else {
            None
        };

        if is_relative || is_package {
            if resolved_target.is_none() {
                self.diagnostics.push(
                    Diagnostic::error(
                        ErrorCode::InvalidImportPath,
                        format!("cannot resolve module '{}'", i.source),
                    )
                    .with_range(i.range),
                );
            }
        } else if !is_stdlib {
            self.diagnostics.push(
                Diagnostic::error(
                    ErrorCode::InvalidImportPath,
                    format!("cannot resolve module '{}'", i.source),
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
                    local.clone(),
                    imported.to_string(),
                    range.start.line,
                    *range,
                ),
                ImportSpecifier::Default { local, range, .. } => (
                    local.clone(),
                    "default".to_owned(),
                    range.start.line,
                    *range,
                ),
                ImportSpecifier::Namespace { local, range, .. } => {
                    (local.clone(), "*".to_owned(), range.start.line, *range)
                }
            };

            let module_path = resolved_target
                .clone()
                .or_else(|| {
                    if module_resolver::is_known_module(&i.source) {
                        Some(i.source.to_string())
                    } else {
                        None
                    }
                })
                .map(Rc::from);

            let exports_ref: Option<&module_resolver::ExportMap> =
                relative_exports.as_deref().or(stdlib_exports.as_deref());

            let sym = if let Some(exports) = exports_ref {
                if imported == "*" {
                    let mut s = Symbol::new(SymbolKind::Namespace, local.clone(), line);
                    s.ty = Some(crate::types::Type(
                        TypeKind::Named(Rc::from("*"), module_path.clone()),
                        false,
                    ));
                    s.origin_module = module_path;
                    s
                } else {
                    match exports.get(&imported) {
                        Some(resolved) => {
                            let mut s = resolved.clone();
                            s.name = local.clone();
                            s.line = line;
                            s.original_name = Some(Rc::from(imported.as_ref()));
                            s.origin_module =
                                resolved.origin_module.clone().or(module_path.clone());
                            if let (Some(ref mut ty), Some(origin)) = (&mut s.ty, &s.origin_module)
                            {
                                *ty = ty.clone().with_origin(origin.clone());
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
                                        i.source, imported
                                    ),
                                )
                                .with_range(range),
                            );
                            let mut s = Symbol::new(SymbolKind::Let, local.clone(), line);
                            s.original_name = Some(Rc::from(imported.as_ref()));
                            s.origin_module = module_path;
                            s
                        }
                    }
                }
            } else {
                Symbol::new(SymbolKind::Let, local.clone(), line)
            };

            self.define(local.to_string(), sym);
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
                            self.escape_array_candidate(name.as_ref());
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
                    self.escape_array_candidate(s.local.as_ref());
                }
            }
            ExportDecl::All { .. } => {}
        }
    }
}
