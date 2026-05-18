mod extensions;
mod format;
mod params;
mod symbols;

use std::collections::HashMap;

use crate::constants::{SEVERITY_ERROR, SEVERITY_HINT, SEVERITY_WARNING};
use crate::document::{
    uri_to_path, DocumentAnalysis, LspDiag, RelatedLocation, SymbolRecord, TokenRecord,
};
use varn_checker::types::FunctionType;
use varn_checker::{module_resolver, SymbolKind};
use varn_core::ast::{Decl, Stmt, StmtKind};
use varn_core::{DiagnosticKind, TokenKind, TypeKind};

pub fn run_pipeline(source: String, uri: String) -> DocumentAnalysis {
    let path = uri_to_path(&uri);
    let (raw_tokens, lex_errs) = varn_lexer::scan(&source, &path);

    let mut diagnostics: Vec<LspDiag> = Vec::new();
    for e in lex_errs {
        diagnostics.push(LspDiag {
            message: e.message,
            line: e.range.start.line.saturating_sub(1),
            col: e.range.start.column,
            end_line: e.range.end.line.saturating_sub(1),
            end_col: e.range.end.column,
            severity: SEVERITY_ERROR,
            related: Vec::new(),
            suggestions: Vec::new(),
        });
    }

    let tokens: Vec<TokenRecord> = raw_tokens
        .iter()
        .filter(|t| {
            !matches!(
                t.kind,
                TokenKind::Whitespace | TokenKind::Newline | TokenKind::EOF | TokenKind::DocComment
            )
        })
        .map(|t| TokenRecord {
            kind: t.kind,

            line: t.range.start.line.saturating_sub(1),
            col: t.range.start.column,
            length: t.lexeme.chars().count() as u32,
            offset: t.range.start.offset,
            lexeme: t.lexeme.to_string(),
        })
        .collect();

    let (mut program, parse_errs) = varn_parser::parse_partial(raw_tokens, &path);
    for e in parse_errs {
        diagnostics.push(LspDiag {
            message: e.message,
            line: e.range.start.line.saturating_sub(1),
            col: e.range.start.column,
            end_line: e.range.end.line.saturating_sub(1),
            end_col: e.range.end.column,
            severity: SEVERITY_ERROR,
            related: Vec::new(),
            suggestions: Vec::new(),
        });
    }

    varn_core::assign_ast_ids(&mut program);
    let result = varn_checker::Checker::check_for_lsp(&program);

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
            related,
            suggestions: d.suggestions.clone(),
        });
    }

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
                            module_resolver::resolve_module_bind(origin).and_then(|rb| {
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
                        sym.origin_module.as_ref().and_then(|origin| {
                            module_resolver::resolve_module_bind(origin).and_then(|rb| {
                                let name = sym.original_name.as_ref().unwrap_or(&sym.name);
                                rb.get_flattened_members(name)
                                    .or_else(|| rb.get_class_entry(name).map(|e| &e.members))
                                    .or_else(|| rb.get_interface_members_local(name))
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
                            module_resolver::resolve_module_bind(origin).and_then(|rb| {
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
                full_range: sym.full_range,
                is_from_stdlib: sym.origin_module.is_some(),
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
    symbols::inject_stdlib_symbols(&mut all_symbols, &mut symbol_map, &result.bind, &tokens);
    let extension_members = extensions::build_extension_members(&result.bind);

    let param_scopes = params::collect_param_scopes(&tokens);
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
        flattened_members: result.flattened_members.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        extension_members,
    };

    DocumentAnalysis {
        source,
        uri,
        diagnostics,
        symbols: all_symbols,
        tokens,
        symbol_map,
        param_scopes,
        type_param_names,
        db,
        import_paths,
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
