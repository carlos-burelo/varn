use tower_lsp::lsp_types::HoverContents;
use crate::opts::DebugFlags;

pub fn debug_lsp(path: &str, source: &str, flags: &DebugFlags) {
    use super::super::colors::{footer, header, BOLD, C_TYPES, R, BLUE, YELLOW, DIM, GREEN};

    header(C_TYPES, "lsp analysis dashboard", path);

    let uri = if cfg!(windows) {
        format!("file:///{}", path.replace('\\', "/"))
    } else {
        format!("file://{}", path)
    };

    let analysis = varn_lsp::pipeline::run_pipeline(source.to_string(), uri);

    // 1. SYMBOLS
    if flags.lsp_symbols {
        eprintln!("  {BOLD}{BLUE}Symbols:{RESET}", BOLD=BOLD, BLUE=BLUE, RESET=R);
        for sym in &analysis.symbols {
            if sym.line == u32::MAX { continue; }
            let kind_str = format!("{:?}", sym.kind);
            eprintln!(
                "    {DIM}{:<12}{RESET} {BOLD}{:<20}{RESET} : {YELLOW}{:<20}{RESET} {DIM}(ln:{}){RESET}",
                kind_str, sym.name, sym.type_str, sym.line + 1,
                DIM=DIM, RESET=R, BOLD=BOLD, YELLOW=YELLOW
            );
        }
        eprintln!();
    }

    // 2. HOVERS
    if flags.lsp_hovers {
        eprintln!("  {BOLD}{BLUE}Simulated Hovers:{RESET}", BOLD=BOLD, BLUE=BLUE, RESET=R);
        for tok in &analysis.tokens {
            if tok.kind == varn_core::TokenKind::Identifier || tok.kind.can_be_identifier() {
                if let Some(hover) = varn_lsp::features::hover::build_hover(&analysis, tok.line, tok.col) {
                    let content = match hover.contents {
                        HoverContents::Scalar(c) => format_marked_string(c),
                        HoverContents::Array(arr) => arr.into_iter().map(format_marked_string).collect::<Vec<_>>().join(" | "),
                        HoverContents::Markup(m) => m.value,
                    };
                    eprintln!(
                        "    {DIM}({:>2}:{:>2}){RESET} {YELLOW}{:<15}{RESET} → {BOLD}{}{RESET}",
                        tok.line + 1, tok.col + 1, tok.lexeme, content.replace('\n', " "),
                        DIM=DIM, RESET=R, YELLOW=YELLOW, BOLD=BOLD
                    );
                }
            }
        }
        eprintln!();
    }

    // 3. COMPLETIONS
    if flags.lsp_completions {
        eprintln!("  {BOLD}{BLUE}Simulated Completions:{RESET}", BOLD=BOLD, BLUE=BLUE, RESET=R);
        for tok in &analysis.tokens {
            if matches!(tok.kind, varn_core::TokenKind::Dot | varn_core::TokenKind::LParen | varn_core::TokenKind::Comma) {
                let trigger = if tok.kind == varn_core::TokenKind::Dot { "." } else { "" };
                let (resp, _) = varn_lsp::features::completion::build_completion_response(
                    &analysis, tok.line, tok.col + 1, Some(trigger), "Invoked".to_string(), None,
                );
                if let Some(tower_lsp::lsp_types::CompletionResponse::Array(items)) = resp {
                    let labels: Vec<_> = items.iter().map(|it| it.label.clone()).collect();
                    if !labels.is_empty() {
                        eprintln!(
                            "    {DIM}({:>2}:{:>2}){RESET} {BOLD}{}{RESET} → [{}]",
                            tok.line + 1, tok.col + 1, tok.lexeme, labels.join(", "),
                            DIM=DIM, RESET=R, BOLD=BOLD
                        );
                    }
                }
            }
        }
        eprintln!();
    }

    // 4. EXPRESSION TYPES
    if flags.lsp_types && !analysis.db.expr_types.is_empty() {
        eprintln!("  {BOLD}{BLUE}Expression Types (Detailed Mapping):{RESET}", BOLD=BOLD, BLUE=BLUE, RESET=R);
        let mut sorted: Vec<_> = analysis.db.expr_types.iter().collect();
        sorted.sort_by_key(|(off, _)| *off);
        for (off, ty) in sorted {
            eprintln!("    {DIM}offset {:>4}{RESET} : {YELLOW}{}{RESET}", off, ty, DIM=DIM, RESET=R, YELLOW=YELLOW);
        }
        eprintln!();
    }

    // 5. SEMANTIC TOKENS
    let sem_tokens = varn_lsp::features::semantic_tokens::build_semantic_tokens(&analysis);
    if flags.lsp_semantic {
        eprintln!("  {BOLD}{BLUE}Semantic Tokens Mapping:{RESET}", BOLD=BOLD, BLUE=BLUE, RESET=R);
        let legend = &varn_lsp::features::semantic_tokens::LEGEND;
        let mut curr_line = 0;
        let mut curr_col = 0;
        for chunk in sem_tokens.chunks_exact(5) {
            curr_line += chunk[0];
            if chunk[0] == 0 { curr_col += chunk[1]; } else { curr_col = chunk[1]; }
            let type_name = legend.token_types.get(chunk[3] as usize).map(|t| t.as_str()).unwrap_or("dynamic");
            let lexeme = find_lexeme(&analysis, curr_line, curr_col, chunk[2]);
            eprintln!(
                "    {DIM}({:>2}:{:>2}){RESET} {YELLOW}{:<15}{RESET} → {GREEN}{}{RESET}",
                curr_line + 1, curr_col + 1, lexeme, type_name,
                DIM=DIM, RESET=R, YELLOW=YELLOW, GREEN=GREEN
            );
        }
        eprintln!();
    }

    if !analysis.diagnostics.is_empty() {
        eprintln!("  {BOLD}{BLUE}LSP Diagnostics:{RESET}", BOLD=BOLD, BLUE=BLUE, RESET=R);
        for diag in &analysis.diagnostics {
            eprintln!("    {BOLD}[line {}]{RESET} {}", diag.line + 1, diag.message, BOLD=BOLD, RESET=R);
        }
    }

    footer(
        C_TYPES,
        &format!(
            "{} symbols, {} expressions, {} semantic tokens analyzed",
            analysis.symbols.len(),
            analysis.db.expr_types.len(),
            sem_tokens.len() / 5
        ),
    );
}

fn find_lexeme(analysis: &varn_lsp::document::DocumentAnalysis, line: u32, col: u32, length: u32) -> String {
    analysis.tokens.iter()
        .find(|t| t.line == line && t.col == col)
        .map(|t| t.lexeme[..(length as usize).min(t.lexeme.len())].to_string())
        .or_else(|| {
            analysis.tokens.iter()
                .find(|t| t.line == line && t.col <= col && col < t.col + t.length)
                .map(|t| {
                    let start = (col - t.col) as usize;
                    let end = (start + length as usize).min(t.lexeme.len());
                    t.lexeme[start..end].to_string()
                })
        })
        .unwrap_or_else(|| "???".to_string())
}

fn format_marked_string(ms: tower_lsp::lsp_types::MarkedString) -> String {
    match ms {
        tower_lsp::lsp_types::MarkedString::String(s) => s,
        tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
    }
}
