mod params;

use std::collections::HashMap;

use crate::constants::{SEVERITY_ERROR, SEVERITY_HINT, SEVERITY_WARNING};
use crate::document::{
    uri_to_path, DocumentAnalysis, LspDiag, RelatedLocation, TokenRecord,
};
use varn_checker::SymbolKind;
use varn_core::ast::{Decl, Stmt, StmtKind};
use varn_core::{DiagnosticKind, TokenKind};

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

    let (program, parse_errs) = varn_parser::parse_partial(raw_tokens, lexeme_buf, &path);
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

    // Ids and one type map, not twenty fields per symbol. The rule for the
    // type is the one this used to bake into each record: the type recorded at
    // the symbol's own offset when it is not `dynamic`, else the declared type.
    let mut resolved_types: rustc_hash::FxHashMap<varn_checker::SymbolId, varn_checker::Type> =
        rustc_hash::FxHashMap::default();
    let mut all_symbols: Vec<varn_checker::SymbolId> = Vec::new();
    let mut symbol_map: HashMap<String, SymbolKind> = HashMap::new();

    for (id, sym) in result.bind.arena.all().iter().enumerate() {
        let recorded = result
            .expr_types
            .get(&sym.offset)
            .filter(|info| info.symbol_id == Some(id))
            .map(|i| i.ty.clone())
            .filter(|t| !t.is_dynamic());
        if let Some(ty) = recorded.or_else(|| sym.ty.clone()) {
            resolved_types.insert(id, ty);
        }
        all_symbols.push(id);
        symbol_map.entry(sym.name.to_string()).or_insert(sym.kind);
    }

    // Type-parameter names: the token scan, plus every `TypeParameter` the
    // checker bound. Feeds semantic tokens, which has no other way to know a
    // bare name in a type annotation is a parameter.
    let (_type_param_map, mut type_param_names) = params::collect_type_params(&tokens);
    for &id in &all_symbols {
        let sym = result.bind.arena.get(id);
        if sym.kind == SymbolKind::TypeParameter {
            type_param_names.insert(sym.name.to_string());
        }
    }

    let import_paths = collect_import_paths(&program.body);

    let global_scope = result.bind.global_scope;
    let scopes = result.bind.scopes.clone();
    let arena = result.bind.arena.clone();

    let spatial_index = crate::query::SpatialIndex::build(&program);

    let db = crate::document::SemanticDB {
        expr_table: result.expr_table,
        expr_types: result.expr_types,
        node_scopes: result.node_scopes,
        scope_spans: result.scope_spans,
        symbol_types: result.symbol_types,
        arena,
        scopes,
        global_scope,
        flattened_members: result
            .flattened_members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        member_resolutions: result.member_resolutions,
        call_resolutions: result.call_resolutions,
        bind: result.bind,
    };

    DocumentAnalysis {
        source,
        uri,
        diagnostics,
        symbols: all_symbols,
        resolved_types,
        tokens,
        trivia,
        symbol_map,
        type_param_names,
        db,
        import_paths,
        spatial_index,
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
