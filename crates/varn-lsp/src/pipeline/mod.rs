use varn_checker::module_resolver::ImportResolver;
mod extensions;
mod format;
mod params;
pub(crate) mod symbols;

use std::collections::HashMap;

use crate::constants::{SEVERITY_ERROR, SEVERITY_HINT, SEVERITY_WARNING};
use crate::document::{
    uri_to_path, DocumentAnalysis, LspDiag, RelatedLocation, SymbolRecord, TokenRecord,
};
use varn_checker::types::FunctionType;
use varn_checker::SymbolKind;
use varn_core::ast::{Decl, Stmt, StmtKind};
use varn_core::{DiagnosticKind, TokenKind, TypeKind};

pub(super) fn stable_global_key(
    uri: &str,
    name: &str,
    kind: SymbolKind,
    symbol_id: Option<usize>,
    origin: Option<&str>,
    original_name: Option<&str>,
    is_global: bool,
) -> String {
    if let Some(origin_mod) = origin {
        let canonical_name = original_name.unwrap_or(name);
        let origin_uri = if origin_mod.starts_with("file://")
            || origin_mod.starts_with("std:")
            || origin_mod.starts_with("core:")
            || origin_mod.starts_with("runtime:")
        {
            origin_mod.to_owned()
        } else {
            varn_modules::resolver::path_to_uri(origin_mod)
        };
        return format!("m:{}#{kind:?}:{}", origin_uri, canonical_name);
    }
    let norm_uri = if uri.starts_with("file://")
        || uri.starts_with("std:")
        || uri.starts_with("core:")
        || uri.starts_with("runtime:")
    {
        uri.to_owned()
    } else {
        varn_modules::resolver::path_to_uri(uri)
    };
    if is_global {
        return format!("m:{}#{kind:?}:{}", norm_uri, name);
    }
    if let Some(sid) = symbol_id {
        return format!("u:{}#{kind:?}:{}", norm_uri, sid);
    }
    format!("u:{}#{kind:?}:{}", norm_uri, name)
}

pub fn run_pipeline(source: String, uri: String) -> DocumentAnalysis {
    varn_builtins::register_provider();
    let path = uri_to_path(&uri);
    // `scan_with_trivia`, not `scan`: the editor has to reproduce the source
    // (comment folding today, formatting later), and comments are the one part
    // the scanner would otherwise drop unrecoverably.
    let (raw_tokens, lexeme_buf, lex_errs, trivia) = varn_lexer::scan_with_trivia(&source, &path);

    let mut diagnostics: Vec<LspDiag> = Vec::new();
    for e in lex_errs {
        diagnostics.push(LspDiag {
            message: e.message,
            line: e.range.start.line.saturating_sub(1),
            col: e.range.start.column,
            end_line: e.range.end.line.saturating_sub(1),
            end_col: e.range.end.column,
            severity: SEVERITY_ERROR,
            code: Some(e.code),
            related: Vec::new(),
            suggestions: Vec::new(),
        });
    }

    let tokens: Vec<TokenRecord> = raw_tokens
        .iter()
        .filter(|t| {
            !matches!(
                t.kind,
                TokenKind::Whitespace
                    | TokenKind::Newline
                    | TokenKind::EOF
                    | TokenKind::DocComment
                    | TokenKind::Dynamic
            )
        })
        .map(|t| {
            let lex = t.get_lexeme(&lexeme_buf);
            let start_byte = t.range.start.offset as usize;
            let end_byte = t.range.end.offset as usize;
            let line_start_byte = source[..start_byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
            TokenRecord {
                kind: t.kind,
                line: t.range.start.line.saturating_sub(1),
                col: source[line_start_byte..start_byte].chars().count() as u32,
                length: source[start_byte..end_byte].chars().count() as u32,
                offset: t.range.start.offset,
                lexeme: lex.to_string(),
            }
        })
        .collect();

    let (mut program, parse_errs) = varn_parser::parse_partial(raw_tokens, lexeme_buf, &path);
    for e in parse_errs {
        diagnostics.push(LspDiag {
            message: e.message,
            line: e.range.start.line.saturating_sub(1),
            col: e.range.start.column,
            end_line: e.range.end.line.saturating_sub(1),
            end_col: e.range.end.column,
            severity: SEVERITY_ERROR,
            code: Some(e.code),
            related: Vec::new(),
            suggestions: Vec::new(),
        });
    }

    varn_core::assign_ast_ids(&mut program);
    let result = crate::workspace::resolver::with_resolver(|r| {
        varn_checker::Checker::check_with(&program, r, varn_checker::CheckOptions::tooling())
    });

    for d in &result.diagnostics {
        let severity = match d.kind {
            DiagnosticKind::Error => SEVERITY_ERROR,
            DiagnosticKind::Warning => SEVERITY_WARNING,
            DiagnosticKind::Hint => SEVERITY_HINT,
        };

        let related = build_related_locations(d, &uri);
        diagnostics.push(LspDiag {
            message: d.message.clone(),
            line: d.range.start.line.saturating_sub(1),
            col: d.range.start.column,
            end_line: d.range.end.line.saturating_sub(1),
            end_col: d.range.end.column,
            severity,
            code: Some(d.code),
            related,
            suggestions: d.suggestions.clone(),
        });
    }

    // `cannot resolve module 'std:*'` on every stdlib import usually means no
    // active std was found at all (missing std/, corrupt bundle, bad install)
    // rather than N unrelated typos — surface that as one clear diagnostic
    // instead of leaving the user to guess from the per-import spam.
    if diagnostics
        .iter()
        .any(|d| d.message.starts_with("cannot resolve module 'std:"))
        && varn_modules::provider::get()
            .and_then(|p| p.std_provenance())
            .is_none()
    {
        diagnostics.insert(
            0,
            LspDiag {
                message: "no standard library found for this workspace (checked varn.json \
                    'std', VARN_STD, this checkout's std/ tree, and the stdlib compiled \
                    into this binary) — rebuild or reinstall the vn toolchain"
                    .to_string(),
                line: 0,
                col: 0,
                end_line: 0,
                end_col: 0,
                severity: SEVERITY_ERROR,
                code: Some(varn_core::ErrorCode::InvalidImportPath),
                related: Vec::new(),
                suggestions: Vec::new(),
            },
        );
    }

    // Module origins of `import * as ns` aliases. A class/interface destructured
    // out of such a namespace (`const { Duration } = ns`) is bound locally with no
    // origin link, so its member table can only be recovered by resolving the name
    // back through these modules.
    let namespace_origins: Vec<String> = result
        .bind
        .arena
        .all()
        .iter()
        .filter(|s| s.kind == SymbolKind::Namespace)
        .filter_map(|s| s.origin_module.as_deref().map(str::to_string))
        .collect();

    fn resolve_bind_any(origin: &str) -> Option<std::rc::Rc<varn_checker::BindResult>> {
        if origin.starts_with("std:")
            || origin.starts_with("runtime:")
            || origin.starts_with("core:")
        {
            crate::workspace::resolver::with_resolver(|r| r.stdlib_bind(origin))
        } else {
            crate::workspace::resolver::with_resolver(|r| r.module_bind(origin))
        }
    }

    // Membership in the global scope, precomputed once: scanning the global
    // bindings per symbol is quadratic and dominates analysis of files with
    // many top-level declarations.
    let global_ids: rustc_hash::FxHashSet<usize> = result
        .bind
        .scopes
        .get(result.bind.global_scope)
        .bindings
        .values()
        .copied()
        .collect();

    let sym_records: Vec<SymbolRecord> = result
        .bind
        .arena
        .all()
        .iter()
        .enumerate()
        .map(|(id, sym)| {
            let members = if sym.kind == SymbolKind::Enum {
                result
                    .bind
                    .get_enum_members_local(&sym.name)
                    .map(|ms| symbols::map_enum_members(ms, &tokens))
                    .or_else(|| {
                        sym.origin_module.as_ref().and_then(|origin| {
                            crate::workspace::resolver::with_resolver(|r| r.module_bind(origin)).map(|b| (*b).clone()).and_then(|rb| {
                                rb.get_enum_members_local(
                                    sym.original_name.as_ref().unwrap_or(&sym.name),
                                )
                                .map(|ms| symbols::map_enum_members(ms, &tokens))
                            })
                        })
                    })
                    .unwrap_or_default()
            } else if sym.kind == SymbolKind::Class || sym.kind == SymbolKind::Interface {
                result
                    .flattened_members
                    .get(&sym.name)
                    .map(|ms| symbols::map_members(ms, &tokens))
                    .or_else(|| {
                        if sym.kind == SymbolKind::Class {
                            result
                                .bind
                                .get_class_entry(&sym.name)
                                .map(|e| symbols::map_members(&e.members, &tokens))
                        } else {
                            result
                                .bind
                                .get_interface_members_local(&sym.name)
                                .map(|ms| symbols::map_members(ms, &tokens))
                        }
                    })
                    .or_else(|| {
                        sym.origin_module.as_ref().and_then(|origin| {
                            crate::workspace::resolver::with_resolver(|r| r.module_bind(origin)).map(|b| (*b).clone()).and_then(|rb| {
                                let name = sym.original_name.as_ref().unwrap_or(&sym.name);
                                rb.get_flattened_members(name)
                                    .or_else(|| rb.get_class_entry(name).map(|e| &e.members))
                                    .or_else(|| rb.get_interface_members_local(name))
                                    .map(|ms| symbols::map_members(ms, &tokens))
                            })
                        })
                    })
                    .or_else(|| {
                        // Destructured from a namespace alias: resolve the class /
                        // interface name through the imported modules.
                        namespace_origins.iter().find_map(|origin| {
                            resolve_bind_any(origin).and_then(|rb| {
                                rb.get_flattened_members(&sym.name)
                                    .or_else(|| rb.get_class_entry(&sym.name).map(|e| &e.members))
                                    .or_else(|| rb.get_interface_members_local(&sym.name))
                                    .map(|ms| symbols::map_members(ms, &tokens))
                            })
                        })
                    })
                    .unwrap_or_default()
            } else if matches!(
                sym.kind,
                SymbolKind::Let | SymbolKind::Var | SymbolKind::Const
            ) {
                result
                    .bind
                    .type_members
                    .objects
                    .get(&sym.name)
                    .map(|ms| symbols::map_members(ms, &tokens))
                    .unwrap_or_default()
            } else if sym.kind == SymbolKind::Namespace {
                result
                    .bind
                    .type_members
                    .namespaces
                    .get(&sym.name)
                    .map(|ms| symbols::map_members(ms, &tokens))
                    .or_else(|| {
                        sym.origin_module.as_ref().and_then(|origin| {
                            crate::workspace::resolver::with_resolver(|r| r.module_bind(origin)).map(|b| (*b).clone()).and_then(|rb| {
                                rb.type_members
                                    .namespaces
                                    .get(sym.original_name.as_ref().unwrap_or(&sym.name))
                                    .map(|ms| symbols::map_members(ms, &tokens))
                            })
                        })
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let expr_info = result
                .expr_types
                .get(&sym.offset)
                .filter(|info| info.symbol_id == Some(id));

            let inferred_ty = match expr_info.map(|i| i.ty.clone()) {
                Some(ct) if !ct.is_dynamic() => Some(ct),
                _ => sym.ty.clone(),
            };
            let symbol_id = Some(id);
            let is_global = global_ids.contains(&id);
            let global_key = stable_global_key(
                &uri,
                sym.name.as_ref(),
                sym.kind,
                symbol_id,
                sym.origin_module.as_deref(),
                sym.original_name.as_deref().map(|s| s.as_ref()),
                is_global,
            );

            SymbolRecord {
                name: sym.name.to_string(),
                kind: sym.kind,
                type_str: match inferred_ty.as_ref() {
                    Some(t) => {
                        if sym.kind == SymbolKind::Function {
                            if let TypeKind::Fn(FunctionType { return_type, .. }) = &t.0 {
                                return_type.to_string()
                            } else {
                                t.to_string()
                            }
                        } else {
                            t.to_string()
                        }
                    }
                    None => String::new(),
                },
                params_str: inferred_ty
                    .as_ref()
                    .map(format::format_type_params)
                    .unwrap_or_default(),
                line: sym.line.saturating_sub(1),
                col: sym.col,
                end_line: if sym.full_range.end.line > 0 {
                    sym.full_range.end.line.saturating_sub(1)
                } else {
                    sym.line.saturating_sub(1)
                },
                end_col: if sym.full_range.end.line > 0 {
                    sym.full_range.end.column
                } else {
                    sym.col + sym.name.len() as u32
                },
                has_explicit_type: sym.has_explicit_type,
                is_async: sym.is_async,
                is_generator: sym.is_generator,
                is_arrow: if let Some(TypeKind::Fn(FunctionType { is_arrow, .. })) =
                    inferred_ty.as_ref().map(|t| &t.0)
                {
                    *is_arrow
                } else {
                    false
                },
                doc: sym.doc.as_deref().map(|s| s.to_owned()),
                members,
                type_params: sym.type_params.iter().map(|s| s.to_string()).collect(),
                ty: inferred_ty.unwrap_or(varn_checker::types::Type::Dynamic),
                symbol_id,
                global_key,
                full_range: sym.full_range,
                is_from_stdlib: sym.origin_module.as_deref().map_or(false, |m| {
                    m.starts_with("std:") || m.starts_with("core:") || m.starts_with("runtime:")
                }),
                origin: sym.origin_module.as_deref().map(|s| s.to_owned()),
            }
        })
        .collect();

    let mut symbol_map: HashMap<String, SymbolKind> =
        HashMap::with_capacity(sym_records.len() + 64);
    for sym in &sym_records {
        symbol_map.entry(sym.name.clone()).or_insert(sym.kind);
    }

    let mut all_symbols = sym_records;
    symbols::inject_stdlib_symbols(
        &mut all_symbols,
        &mut symbol_map,
        &result.bind,
        &tokens,
        &uri,
    );
    let extension_members = extensions::build_extension_members(&result.bind);

    let (type_param_map, mut type_param_names) = params::collect_type_params(&tokens);

    for sym in &all_symbols {
        if sym.kind == SymbolKind::TypeParameter {
            type_param_names.insert(sym.name.clone());
        }
    }
    for sym in &mut all_symbols {
        if sym.line != crate::constants::STDLIB_LINE_MARKER && sym.type_params.is_empty() {
            if let Some(tps) = type_param_map.get(&sym.name) {
                sym.type_params = tps.clone();
            }
        }
    }

    let import_paths = collect_import_paths(&program.body);

    let global_scope = result.bind.global_scope;
    let scopes = result.bind.scopes.clone();
    let arena = result.bind.arena.clone();

    let db = crate::document::SemanticDB {
        expr_types: result.expr_types,
        node_scopes: result.node_scopes,
        symbol_types: result.symbol_types,
        arena,
        scopes,
        global_scope,
        flattened_members: result
            .flattened_members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        extension_members,
        member_resolutions: result.member_resolutions,
        call_resolutions: result.call_resolutions,
        bind: result.bind,
    };

    let positional_index = crate::queries::indexes::PositionalIndex::build(&db.node_scopes);

    DocumentAnalysis {
        source,
        uri,
        diagnostics,
        symbols: all_symbols,
        tokens,
        trivia,
        symbol_map,
        type_param_names,
        db,
        import_paths,
        positional_index,
        ast: Some(program),
    }
}

fn build_related_locations(d: &varn_core::Diagnostic, current_uri: &str) -> Vec<RelatedLocation> {
    d.suggestions
        .iter()
        .filter_map(|s| {
            let range = s.range.as_ref()?;
            let message = if let Some(repl) = &s.replacement {
                format!("{} \u{2192} `{}`", s.message, repl)
            } else {
                s.message.clone()
            };
            Some(RelatedLocation {
                message,
                uri: current_uri.to_owned(),
                line: range.start.line.saturating_sub(1),
                col: range.start.column,
            })
        })
        .collect()
}

fn collect_import_paths(stmts: &[Stmt]) -> Vec<String> {
    let mut paths = Vec::new();
    for stmt in stmts {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Import(i) = decl.as_ref() {
                paths.push(i.source.to_string());
            }
        }
    }
    paths
}
