use std::sync::Arc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, ImportDecl, ImportSpecifier, Pattern};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::symbol::{Symbol, SymbolKind};

impl<'r> super::Binder<'r> {
    pub(super) fn bind_import(&mut self, i: &ImportDecl) {
        let source_str = self.interner.resolve(i.source).to_string();
        use varn_modules::layer::{check_import, Layer};
        if let Err(message) = check_import(Layer::of_module(&self.source_file), &source_str) {
            self.emit(Diagnostic::error(ErrorCode::InvalidImportPath, message).with_range(i.range));

            if Layer::of_module(&source_str) == Layer::Core {
                return;
            }
        }

        let is_relative = source_str.starts_with('.') || source_str.starts_with('/');
        let is_package = varn_modules::is_pkg_specifier(&source_str);

        let resolved_target = if (is_relative || is_package) && !self.source_file.is_empty() {
            let base_path = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            if is_package {
                crate::paths::resolve_package_specifier_path(base_path, &source_str)
            } else {
                self.resolver.resolve_specifier(base_path, &source_str)
            }
        } else {
            None
        };

        let is_stdlib = !is_relative && crate::paths::is_known_module(&source_str);

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
        for exports in relative_exports.iter().chain(stdlib_exports.iter()) {
            self.adopt(&exports.table);
        }
        if let Some(dep) = resolved_target
            .clone()
            .or_else(|| is_stdlib.then(|| source_str.clone()))
        {
            self.deps.push(Arc::from(dep));
        }

        if is_relative || is_package {
            if resolved_target.is_none() {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::InvalidImportPath,
                        format!("cannot resolve module '{}'", source_str),
                    )
                    .with_range(i.range),
                );
            }
        } else if !is_stdlib {
            self.emit(
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
                    if crate::paths::is_known_module(&source_str) {
                        Some(source_str.clone())
                    } else {
                        None
                    }
                })
                .map(Arc::from);
            let module_path_atom = module_path.as_ref().map(|s| self.intern_local(s));

            let exports_ref: Option<&varn_sem::exports::ExportMap> =
                relative_exports.as_deref().or(stdlib_exports.as_deref());

            let sym = if let Some(exports) = exports_ref {
                if imported == "*" {
                    let mut s = Symbol::new(SymbolKind::Namespace, local, line);
                    s.ty = Some(varn_sem::types::Type::named_with_origin(
                        Arc::from("*"),
                        module_path.clone(),
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ));
                    s.origin_module = module_path_atom;
                    s
                } else {
                    match exports.get(&imported) {
                        Some(resolved) => {
                            let mut s = resolved.clone();
                            s.full_range = resolved.full_range;
                            s.name = local;
                            s.line = line;

                            s.alias_node = resolved.alias_node.clone();
                            s.original_name = Some(self.intern_local(&imported));
                            s.origin_module = s.origin_module.or(module_path_atom);
                            if let (Some(ref mut ty), Some(origin)) = (&mut s.ty, &s.origin_module)
                            {
                                *ty = ty.with_origin(
                                    *origin,
                                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                );
                            }

                            if let Some(mp) = &module_path {
                                s.intrinsic_wire = varn_core::intrinsic_ops::intrinsic_lookup(
                                    &format!("{}/{}", mp, imported),
                                );
                            }
                            s
                        }
                        None => {
                            self.emit(
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

                ExportDefaultDecl::Expr(_) => self.escape_all_open_array_candidates(),
            },

            ExportDecl::Named {
                specifiers, source, ..
            } => {
                for s in specifiers {
                    self.escape_array_candidate(s.local);
                }
                if let Some(source) = source {
                    self.record_reexport_dep(*source);
                }
            }
            ExportDecl::All { source, .. } => self.record_reexport_dep(*source),
        }
    }

    fn record_reexport_dep(&mut self, source: varn_core::Atom) {
        let spec = self.interner.resolve(source).to_string();
        let dep = if crate::paths::is_known_module(&spec) {
            Some(spec)
        } else {
            let base = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            self.resolver.resolve_specifier(base, &spec)
        };
        if let Some(dep) = dep {
            self.deps.push(Arc::from(dep));
        }
    }
}
