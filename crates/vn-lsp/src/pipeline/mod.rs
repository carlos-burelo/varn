mod params;

use rustc_hash::FxHashMap;
use vn_pipeline::{Session, Source};

use crate::document::{uri_to_path, DocumentAnalysis, LspDiag, RelatedLocation, TokenRecord};
use vn_core::ast::{AstArena, Decl, StmtId, StmtKind};
use vn_core::{DiagnosticKind, TokenKind};
use vn_sem::symbol::SymbolKind;

fn position_of(source: &str, line_starts: &[usize], offset: u32) -> (u32, u32) {
    let mut at = (offset as usize).min(source.len());
    while !source.is_char_boundary(at) {
        at -= 1;
    }
    let line = line_starts
        .partition_point(|&start| start <= at)
        .saturating_sub(1);
    let start = line_starts.get(line).copied().unwrap_or(0);
    (line as u32, source[start..at].encode_utf16().count() as u32)
}

fn span_of(
    source: &str,
    line_starts: &[usize],
    range: vn_core::SourceRange,
) -> (u32, u32, u32, u32) {
    let (line, col) = position_of(source, line_starts, range.start.offset);
    let (end_line, end_col) = position_of(source, line_starts, range.end.offset);
    (line, col, end_line, end_col)
}

pub fn run_pipeline(source: String, uri: String, session: &Session) -> DocumentAnalysis {
    vn_builtins::register_provider();
    let path = uri_to_path(&uri);
    let resolver = session.shared_resolver();

    let analysis = session.analyze(&Source::virtual_source(&path, &source));
    let raw_tokens = analysis.tokens.tokens;
    let trivia = analysis.trivia;
    let program = analysis.parsed.program;
    let ast_arena = analysis.parsed.arena;
    let result = analysis.checked;
    let mut syntax = analysis.syntax;

    let mut line_starts: Vec<usize> = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let mut diagnostics: Vec<LspDiag> = Vec::new();
    for e in syntax.take() {
        let (line, col, end_line, end_col) = span_of(&source, &line_starts, e.range);
        diagnostics.push(LspDiag {
            message: e.message,
            line,
            col,
            end_line,
            end_col,
            kind: e.kind,
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

    for d in &result.diagnostics {
        let related = build_related_locations(d, &uri);
        let (line, col, end_line, end_col) = span_of(&source, &line_starts, d.range);
        diagnostics.push(LspDiag {
            message: d.message.clone(),
            line,
            col,
            end_line,
            end_col,
            kind: d.kind,
            code: Some(d.code),
            related,
            suggestions: d.suggestions.clone(),
        });
    }

    if diagnostics
        .iter()
        .any(|d| d.message.starts_with("cannot resolve module 'std:"))
        && vn_modules::provider::get()
            .and_then(|p| p.std_provenance())
            .is_none()
    {
        diagnostics.insert(
            0,
            LspDiag {
                message: "no standard library found for this workspace (checked vn.toml \
                    'std', VN_STD, this checkout's std/ tree, and the stdlib compiled \
                    into this binary) — rebuild or reinstall the vn toolchain"
                    .to_string(),
                line: 0,
                col: 0,
                end_line: 0,
                end_col: 0,
                kind: DiagnosticKind::Error,
                code: Some(vn_core::ErrorCode::InvalidImportPath),
                related: Vec::new(),
                suggestions: Vec::new(),
            },
        );
    }

    let mut resolved_types: rustc_hash::FxHashMap<
        vn_sem::symbol::SymbolId,
        vn_sem::types::Type,
    > = rustc_hash::FxHashMap::default();
    let mut all_symbols: Vec<vn_sem::symbol::SymbolId> = Vec::new();
    let mut symbol_map: FxHashMap<String, SymbolKind> = FxHashMap::default();

    for (id, sym) in result.bind.arena.all().iter().enumerate() {
        let recorded = result
            .expr_types
            .get(&sym.offset)
            .filter(|info| info.symbol_id == Some(id))
            .map(|i| i.ty)
            .filter(|t| !t.is_unresolved());
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

fn build_related_locations(d: &vn_core::Diagnostic, current_uri: &str) -> Vec<RelatedLocation> {
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
    interner: &vn_core::AtomInterner,
) -> Vec<String> {
    let mut paths = Vec::new();
    for &stmt in stmts {
        if let StmtKind::Decl(decl) = &arena.stmt(stmt).kind
            && let Decl::Import(i) = decl.as_ref()
        {
            paths.push(interner.resolve(i.source).to_owned());
        }
    }
    paths
}
