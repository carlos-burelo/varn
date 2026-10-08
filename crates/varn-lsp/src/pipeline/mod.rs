mod params;

use rustc_hash::FxHashMap;

use crate::constants::{SEVERITY_ERROR, SEVERITY_HINT, SEVERITY_WARNING};
use crate::document::{uri_to_path, DocumentAnalysis, LspDiag, RelatedLocation, TokenRecord};
use varn_core::ast::{AstArena, Decl, StmtId, StmtKind};
use varn_core::{DiagnosticKind, TokenKind};
use varn_sem::symbol::SymbolKind;

pub fn run_pipeline(
    source: String,
    uri: String,
    resolver: std::sync::Arc<varn_resolver::DiskResolver>,
) -> DocumentAnalysis {
    varn_builtins::register_provider();
    let path = uri_to_path(&uri);

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

    let mut line_starts: Vec<usize> = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
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
            let start_byte = t.range.start.offset as usize;
            let end_byte = t.range.end.offset as usize;
            let line_idx = line_starts
                .partition_point(|&s| s <= start_byte)
                .saturating_sub(1);
            let line_start_byte = line_starts.get(line_idx).copied().unwrap_or(0);
            TokenRecord {
                kind: t.kind,
                line: t.range.start.line.saturating_sub(1),
                col: source[line_start_byte..start_byte].chars().count() as u32,
                length: source[start_byte..end_byte].chars().count() as u32,
                offset: t.range.start.offset,
                end: t.range.end.offset,
            }
        })
        .collect();

    let (program, parse_errs, ast_arena, interner) = varn_parser::parse_partial(
        raw_tokens,
        lexeme_buf,
        &path,
        varn_core::AtomInterner::new(),
    );
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

    let result = varn_checker::Checker::check_with(
        &program,
        &ast_arena,
        interner,
        resolver.as_ref(),
        varn_sem::output::CheckOptions::tooling(),
    );

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

    let mut resolved_types: rustc_hash::FxHashMap<
        varn_sem::symbol::SymbolId,
        varn_sem::types::Type,
    > = rustc_hash::FxHashMap::default();
    let mut all_symbols: Vec<varn_sem::symbol::SymbolId> = Vec::new();
    let mut symbol_map: FxHashMap<String, SymbolKind> = FxHashMap::default();

    for (id, sym) in result.bind.arena.all().iter().enumerate() {
        let recorded = result
            .expr_types
            .get(&sym.offset)
            .filter(|info| info.symbol_id == Some(id))
            .map(|i| i.ty)
            .filter(|t| !t.is_dynamic());
        if let Some(ty) = recorded.or(sym.ty) {
            resolved_types.insert(id, ty);
        }
        all_symbols.push(id);
        let name = result.bind.interner.resolve(sym.name).to_owned();
        symbol_map.entry(name).or_insert(sym.kind);
    }

    let (_type_param_map, mut type_param_names) = params::collect_type_params(&source, &tokens);
    for &id in &all_symbols {
        let sym = result.bind.arena.get(id);
        if sym.kind == SymbolKind::TypeParameter {
            type_param_names.insert(result.bind.interner.resolve(sym.name).to_owned());
        }
    }

    let import_paths = collect_import_paths(&program.body, &ast_arena, &result.bind.interner);

    let global_scope = result.bind.global_scope;

    let spatial_index = crate::query::SpatialIndex::build(&program, &ast_arena);

    let db = crate::document::SemanticDB {
        expr_table: result.expr_table,
        expr_types: result.expr_types,
        node_scopes: result.node_scopes,
        scope_spans: result.scope_spans,
        symbol_types: resolved_types,
        global_scope,
        flattened_members: result
            .flattened_members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        member_resolutions: result.member_resolutions,
        call_resolutions: result.call_resolutions,
        match_gaps: result.match_gaps,
        call_mappings: result.call_mappings,
        desugar: result.desugar,
        types: std::cell::RefCell::new(result.bind.ty_table.clone()),
        bind: result.bind,
    };

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
        resolver,
        import_paths,
        spatial_index,
        ast: Some(program),
        ast_arena,
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

fn collect_import_paths(
    stmts: &[StmtId],
    arena: &AstArena,
    interner: &varn_core::AtomInterner,
) -> Vec<String> {
    let mut paths = Vec::new();
    for &stmt in stmts {
        if let StmtKind::Decl(decl) = &arena.stmt(stmt).kind {
            if let Decl::Import(i) = decl.as_ref() {
                paths.push(interner.resolve(i.source).to_owned());
            }
        }
    }
    paths
}
