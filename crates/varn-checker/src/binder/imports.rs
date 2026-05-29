use std::rc::Rc;
use varn_core::ast::{ExportDecl, ExportDefaultDecl, ImportDecl, ImportSpecifier};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use crate::module_resolver;
use crate::symbol::{Symbol, SymbolKind};

impl super::Binder {
    pub(super) fn bind_import(&mut self, i: &ImportDecl) {
        // core:* are intrinsic domains — not user-importable.
        // Allow within stdlib context (source_file starts with "core:" or "std:").
        let in_stdlib_context =
            self.source_file.starts_with("core:") || self.source_file.starts_with("std:");
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
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => self.bind_function(f),
                ExportDefaultDecl::Class(c) => self.bind_class(c),
                ExportDefaultDecl::Expr(_) => {}
            },
            ExportDecl::Named { .. } | ExportDecl::All { .. } => {}
        }
    }
}
