use std::sync::Arc;
use vn_core::ast::{ExportDecl, ExportDefaultDecl, ImportDecl, ImportSpecifier};
use vn_core::{Diagnostic, ErrorCode};
use vn_sem::symbol::{Symbol, SymbolKind};

impl<'r> super::Binder<'r> {
    pub(super) fn bind_import(&mut self, i: &ImportDecl) {
        let source_str = self.interner.resolve(i.source).to_string();
        use vn_modules::layer::{check_import, Layer};
        if let Err(message) = check_import(Layer::of_module(&self.source_file), &source_str) {
            self.emit(Diagnostic::error(ErrorCode::InvalidImportPath, message).with_range(i.range));

            if Layer::of_module(&source_str) == Layer::Core {
                return;
            }
        }

        let is_relative = source_str.starts_with('.') || source_str.starts_with('/');
        let is_package = vn_modules::is_pkg_specifier(&source_str);

        let mut package_failure = None;
        let resolved_target = if (is_relative || is_package) && !self.source_file.is_empty() {
            let base_path = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            if is_package {
                crate::paths::resolve_package_specifier_path(base_path, &source_str)
                    .map_err(|reason| package_failure = Some(reason))
                    .ok()
            } else {
                self.resolver.resolve_specifier(base_path, &source_str)
            }
        } else {
            None
        };

        if is_relative && let Some(abs) = &resolved_target {
            let base_path = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            if let Err(message) = vn_modules::contain::check_within_root(base_path, abs) {
                self.emit(
                    Diagnostic::error(ErrorCode::InvalidImportPath, message).with_range(i.range),
                );
                return;
            }
        }

        let is_stdlib = !is_relative && crate::paths::is_known_module(&source_str);

        let relative_exports = if let Some(abs) = &resolved_target {
            let mut visiting = vec![self.source_file.to_string()];
            Some(self.resolver.module_exports(abs, &mut visiting))
        } else {
            None
        };

        let syntax_failure = resolved_target
            .as_deref()
            .and_then(|abs| self.resolver.module_syntax_failure(abs))
            .map(|failure| failure.first_detail());

        let stdlib_exports = if is_stdlib {
            Some(self.resolver.stdlib_exports(&source_str))
        } else {
            None
        };
        let extension_origin = resolved_target
            .clone()
            .or_else(|| is_stdlib.then(|| source_str.clone()));
        for exports in relative_exports.iter().chain(stdlib_exports.iter()) {
            self.adopt(&exports.table);
            self.adopt_extensions(exports, extension_origin.as_deref());
        }
        if let Some(dep) = resolved_target
            .clone()
            .or_else(|| is_stdlib.then(|| source_str.clone()))
        {
            self.deps.push(Arc::from(dep));
        }

        if let Some(detail) = &syntax_failure {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidImportPath,
                    format!(
                        "module '{source_str}' has syntax errors and cannot be imported: {detail}"
                    ),
                )
                .with_range(i.range),
            );
        } else if is_relative || is_package {
            if resolved_target.is_none() {
                let base_path = std::path::Path::new(self.source_file.as_ref())
                    .parent()
                    .unwrap_or(std::path::Path::new("."));
                let attempted = match vn_modules::resolver::resolve_in(&source_str, base_path) {
                    Ok(vn_core::ModuleId::Local(path)) => Some(path.to_string()),
                    Ok(_) | Err(_) => None,
                };
                let message = match (&package_failure, &attempted) {
                    (Some(reason), _) => {
                        format!("cannot resolve module '{source_str}': {reason}")
                    }
                    (None, Some(path)) => format!(
                        "cannot resolve module '{source_str}': no file at {}",
                        vn_core::diagnostics::display_path(path)
                    ),
                    (None, None) => format!("cannot resolve module '{source_str}'"),
                };
                let mut diagnostic =
                    Diagnostic::error(ErrorCode::InvalidImportPath, message).with_range(i.range);
                let with_extension = format!("{source_str}.vn");
                if is_relative
                    && self
                        .resolver
                        .resolve_specifier(base_path, &with_extension)
                        .is_some()
                {
                    diagnostic = diagnostic.with_suggestion(vn_core::Suggestion::did_you_mean(
                        &with_extension,
                        i.range,
                    ));
                }
                self.emit(diagnostic);
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

            let exports_ref: Option<&vn_sem::exports::ExportMap> =
                relative_exports.as_deref().or(stdlib_exports.as_deref());

            if let Some(exports) = exports_ref {
                let hang: Arc<str> = Arc::from(self.interner.resolve(local));
                let found = imported_namespaces(exports, &hang, &imported);
                for ns in &found {
                    for member in &ns.members {
                        let exported_key = format!("{}::{}", ns.exported, member);
                        if let Some(forward) =
                            exports.get(&exported_key).and_then(|s| s.forward.clone())
                        {
                            self.imported_forwards
                                .insert(Arc::from(format!("{}::{}", ns.binding, member)), forward);
                        }
                    }
                }
                self.namespace_imports.extend(found);
            }

            let sym = if let Some(exports) = exports_ref {
                if imported == "*" {
                    let mut s = Symbol::new(SymbolKind::Namespace, local, line);
                    s.ty = Some(vn_sem::types::Type::named_with_origin(
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
                            if let (Some(ty), Some(origin)) = (&mut s.ty, &s.origin_module) {
                                let table = &mut *std::sync::Arc::make_mut(&mut self.ty_table);
                                *ty = ty.with_origin(*origin, table);
                            }

                            if let Some(mp) = &module_path {
                                s.intrinsic_wire =
                                    vn_core::intrinsic_ops::intrinsic_lookup(mp, &imported);
                            }
                            if let Some(forward) = &s.forward {
                                self.imported_forwards.insert(
                                    Arc::from(self.interner.resolve(local)),
                                    forward.clone(),
                                );
                            }
                            s
                        }
                        None => {
                            if syntax_failure.is_none() {
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
                            }
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

    pub(super) fn adopt_extensions(
        &mut self,
        exports: &vn_sem::exports::ExportMap,
        origin: Option<&str>,
    ) {
        self.extensions.absorb(&exports.extensions);
        let origin_atom = origin.map(|o| self.intern_local(o));
        let mut mangled: Vec<Arc<str>> = [
            &exports.extensions.methods,
            &exports.extensions.getters,
            &exports.extensions.setters,
        ]
        .into_iter()
        .flat_map(|by_type| by_type.values())
        .flat_map(|members| members.values().cloned())
        .collect();
        mangled.sort();
        mangled.dedup();
        for name in mangled {
            let Some(exported) = exports.get(name.as_ref()) else {
                continue;
            };
            let local = self.intern_local(&name);
            let mut sym = exported.clone();
            sym.name = local;
            sym.origin_module = sym.origin_module.or(origin_atom);
            if let (Some(ty), Some(origin)) = (&mut sym.ty, &sym.origin_module) {
                let table = &mut *std::sync::Arc::make_mut(&mut self.ty_table);
                *ty = ty.with_origin(*origin, table);
            }
            self.define(local, sym);
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

            ExportDecl::Named { source, .. } => {
                if let Some(source) = source
                    && self.check_reexport_layer(*source, e.range())
                {
                    self.record_reexport_dep(*source);
                }
            }
            ExportDecl::All { source, .. } => {
                if self.check_reexport_layer(*source, e.range()) {
                    self.record_reexport_dep(*source);
                }
            }
        }
    }

    fn check_reexport_layer(
        &mut self,
        source: vn_core::Atom,
        range: &vn_core::SourceRange,
    ) -> bool {
        use vn_modules::layer::{check_import, Layer};
        let source_str = self.interner.resolve(source).to_string();
        if let Err(message) = check_import(Layer::of_module(&self.source_file), &source_str) {
            self.emit(Diagnostic::error(ErrorCode::InvalidImportPath, message).with_range(*range));
            return false;
        }
        if source_str.starts_with('.') || source_str.starts_with('/') {
            let base_path = std::path::Path::new(self.source_file.as_ref())
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let joined = base_path.join(&source_str);
            let resolved = vn_modules::canonical_or_original(&joined);
            if let Err(message) = vn_modules::contain::check_within_root(base_path, &resolved) {
                self.emit(
                    Diagnostic::error(ErrorCode::InvalidImportPath, message).with_range(*range),
                );
                return false;
            }
        }
        true
    }

    fn record_reexport_dep(&mut self, source: vn_core::Atom) {
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

fn imported_namespaces(
    exports: &vn_sem::exports::ExportMap,
    hang: &Arc<str>,
    imported: &str,
) -> Vec<vn_sem::bind::NamespaceImport> {
    let mut keys: Vec<&String> = exports.keys().filter(|k| k.contains("::")).collect();
    keys.sort();
    let mut found: Vec<vn_sem::bind::NamespaceImport> = Vec::new();
    for key in keys {
        let Some((root, member)) = key.split_once("::") else {
            continue;
        };
        if imported != "*" && imported != root {
            continue;
        }
        let binding: Arc<str> = if imported == "*" {
            Arc::from(format!("{hang}::{root}"))
        } else {
            hang.clone()
        };
        match found.iter_mut().find(|ns| ns.binding == binding) {
            Some(ns) => ns.members.push(Arc::from(member)),
            None => found.push(vn_sem::bind::NamespaceImport {
                binding,
                hang: hang.clone(),
                exported: Arc::from(root),
                members: vec![Arc::from(member)],
            }),
        }
    }
    found
}
