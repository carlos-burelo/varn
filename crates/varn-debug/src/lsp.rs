use crate::colors::{BLUE, BOLD, C_TYPES, DIM, GREEN, R, RESET, YELLOW};
use crate::flags::DebugFlags;
use tower_lsp::lsp_types::HoverContents;

pub fn debug_lsp(path: &str, source: &str, flags: &DebugFlags) {
    use crate::colors::{footer, header};
    header(C_TYPES, "lsp analysis dashboard", path);

    let uri = if cfg!(windows) {
        format!("file:///{}", path.replace('\\', "/"))
    } else {
        format!("file://{}", path)
    };

    let analysis = varn_lsp::pipeline::run_pipeline(source.to_string(), uri);

    // 1. SYMBOLS
    if flags.lsp_symbols {
        eprintln!(
            "  {BOLD}{BLUE}Symbols:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        for sym in &analysis.symbols {
            if sym.line == u32::MAX {
                continue;
            }
            let kind_str = format!("{:?}", sym.kind);
            let tag = if sym.is_from_stdlib {
                if sym.line == u32::MAX || sym.origin.as_deref() == Some("builtin") {
                    format!("{DIM}[core]{RESET} ")
                } else {
                    format!("{DIM}[std]{RESET} ")
                }
            } else {
                format!("{DIM}[usr]{RESET} ")
            };
            eprintln!(
                "    {DIM}{:<12}{RESET} {BOLD}{tag}{:<20}{RESET} : {YELLOW}{:<20}{RESET} {DIM}(ln:{}, col:{}){RESET}",
                kind_str, sym.name, sym.type_str, sym.line + 1, sym.col + 1,
                tag = tag, DIM=DIM, RESET=R, BOLD=BOLD, YELLOW=YELLOW
            );
        }
        eprintln!();
    }

    // 2. HOVERS
    if flags.lsp_hovers {
        eprintln!(
            "  {BOLD}{BLUE}Simulated Hovers:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        for tok in &analysis.tokens {
            if tok.kind.can_be_identifier() {
                if let Some(hover) =
                    varn_lsp::features::hover::build_hover(&analysis, tok.line, tok.col)
                {
                    let content = match hover.contents {
                        HoverContents::Scalar(c) => format_marked_string(c),
                        HoverContents::Array(arr) => arr
                            .into_iter()
                            .map(format_marked_string)
                            .collect::<Vec<_>>()
                            .join(" | "),
                        HoverContents::Markup(m) => m.value,
                    };
                    let content = compact_debug_hover(content);
                    eprintln!(
                        "    {DIM}({:>2}:{:>2}){RESET} {YELLOW}{:<15}{RESET} → {BOLD}{}{RESET}",
                        tok.line + 1,
                        tok.col + 1,
                        tok.lexeme,
                        content.replace('\n', " "),
                        DIM = DIM,
                        RESET = R,
                        YELLOW = YELLOW,
                        BOLD = BOLD
                    );
                }
            }
        }
        eprintln!();
    }

    // 3. COMPLETIONS
    if flags.lsp_completions {
        eprintln!(
            "  {BOLD}{BLUE}Simulated Completions:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        for tok in &analysis.tokens {
            if matches!(
                tok.kind,
                varn_core::TokenKind::Dot
                    | varn_core::TokenKind::LParen
                    | varn_core::TokenKind::Comma
            ) {
                let trigger = if tok.kind == varn_core::TokenKind::Dot {
                    "."
                } else {
                    ""
                };
                let (resp, _) = varn_lsp::features::completion::build_completion_response(
                    &analysis,
                    tok.line,
                    tok.col + 1,
                    Some(trigger),
                    "Invoked".to_string(),
                    None,
                );
                if let Some(tower_lsp::lsp_types::CompletionResponse::Array(items)) = resp {
                    let labels: Vec<_> = items.iter().map(|it| it.label.clone()).collect();
                    if !labels.is_empty() {
                        eprintln!(
                            "    {DIM}({:>2}:{:>2}){RESET} {BOLD}{}{RESET} → [{}]",
                            tok.line + 1,
                            tok.col + 1,
                            tok.lexeme,
                            labels.join(", "),
                            DIM = DIM,
                            RESET = R,
                            BOLD = BOLD
                        );
                    }
                }
            }
        }
        eprintln!();
    }

    // 4. EXPRESSION TYPES
    if flags.lsp_types && !analysis.db.expr_types.is_empty() {
        eprintln!(
            "  {BOLD}{BLUE}Expression Types (Detailed Mapping):{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        let mut sorted: Vec<_> = analysis.db.expr_types.iter().collect();
        sorted.sort_by_key(|(off, _)| *off);
        for (off, info) in sorted {
            eprintln!(
                "    {DIM}offset {:>4}{RESET} : {YELLOW}{}{RESET}",
                off,
                info.ty,
                DIM = DIM,
                RESET = R,
                YELLOW = YELLOW
            );
        }
        eprintln!();
    }

    // 5. SEMANTIC TOKENS
    let sem_tokens = varn_lsp::features::semantic_tokens::build_semantic_tokens(&analysis);
    if flags.lsp_semantic {
        eprintln!(
            "  {BOLD}{BLUE}Semantic Tokens Mapping:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        let legend = &varn_lsp::features::semantic_tokens::LEGEND;
        let mut curr_line = 0;
        let mut curr_col = 0;
        for chunk in sem_tokens.chunks_exact(5) {
            curr_line += chunk[0];
            if chunk[0] == 0 {
                curr_col += chunk[1];
            } else {
                curr_col = chunk[1];
            }
            let type_name = legend
                .token_types
                .get(chunk[3] as usize)
                .map(|t| t.as_str())
                .unwrap_or("dynamic");
            let mods = chunk[4];
            let mut mod_names = Vec::new();
            for (bit, m) in legend.token_modifiers.iter().enumerate() {
                if (mods & (1 << bit)) != 0 {
                    mod_names.push(m.as_str());
                }
            }
            let mods_str = if mod_names.is_empty() {
                String::new()
            } else {
                format!(" [{}]", mod_names.join(", "))
            };
            let lexeme = find_lexeme(&analysis, curr_line, curr_col, chunk[2]);
            eprintln!(
                "    {DIM}({:>2}:{:>2}){RESET} {YELLOW}{:<15}{RESET} → {GREEN}{}{RESET}{BOLD}{BLUE}{}{RESET}",
                curr_line + 1, curr_col + 1, lexeme, type_name, mods_str,
                DIM=DIM, RESET=R, YELLOW=YELLOW, GREEN=GREEN, BOLD=BOLD, BLUE=BLUE
            );
        }
        eprintln!();
    }

    // 6. COLORIZE (Contextual Tagging View)
    if flags.lsp_colorize {
        eprintln!(
            "  {BOLD}{BLUE}Semantic Tagging View:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        print_tagged_source(source, &sem_tokens);
        eprintln!();
    }

    if !analysis.diagnostics.is_empty() {
        eprintln!(
            "  {BOLD}{BLUE}LSP Diagnostics:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        for diag in &analysis.diagnostics {
            eprintln!(
                "    {BOLD}[line {}]{RESET} {}",
                diag.line + 1,
                diag.message,
                BOLD = BOLD,
                RESET = R
            );
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

fn find_lexeme(
    analysis: &varn_lsp::document::DocumentAnalysis,
    line: u32,
    col: u32,
    length: u32,
) -> String {
    analysis
        .tokens
        .iter()
        .find(|t| t.line == line && t.col == col)
        .map(|t| t.lexeme[..(length as usize).min(t.lexeme.len())].to_string())
        .or_else(|| {
            analysis
                .tokens
                .iter()
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

fn compact_debug_hover(content: String) -> String {
    const PRIMS: [&str; 7] = ["str", "int", "float", "bool", "char", "decimal", "bigint"];
    for p in PRIMS {
        let prefix = format!("class {p} {{");
        if content.starts_with(&prefix) {
            return format!("class {p}");
        }
    }
    content
}

fn print_tagged_source(source: &str, sem_tokens: &[u32]) {
    use crate::colors::{BLUE, BOLD, DIM, RESET, YELLOW};
    let legend = &varn_lsp::features::semantic_tokens::LEGEND;

    let lines: Vec<&str> = source.lines().collect();
    let mut curr_line = 0;
    let mut curr_col = 0;
    let mut tokens_iter = sem_tokens.chunks_exact(5).peekable();

    for (i, line) in lines.iter().enumerate() {
        let line_num = i as u32;
        let mut last_col = 0;
        eprint!("    {DIM}{:>3} | {RESET}", line_num + 1);

        while let Some(chunk) = tokens_iter.peek() {
            let delta_line = chunk[0];
            let delta_start = chunk[1];
            let length = chunk[2];
            let type_idx = chunk[3] as usize;

            if curr_line + delta_line > line_num {
                break;
            }

            tokens_iter.next();
            curr_line += delta_line;
            if delta_line == 0 {
                curr_col += delta_start;
            } else {
                curr_col = delta_start;
            }

            // Print uncolored text before the token
            if curr_col > last_col {
                let start = last_col as usize;
                let end = (curr_col as usize).min(line.len());
                eprint!("{}", &line[start..end]);
            }

            // Print token with its tag
            let type_name = legend
                .token_types
                .get(type_idx)
                .map(|t| t.as_str())
                .unwrap_or("dynamic");
            let start = curr_col as usize;
            let end = ((curr_col + length) as usize).min(line.len());
            let lexeme = &line[start..end];

            eprint!(
                "{BOLD}{lexeme}{RESET}{BLUE}[{YELLOW}{type_name}{BLUE}]{RESET}",
                BOLD = BOLD,
                RESET = RESET,
                BLUE = BLUE,
                YELLOW = YELLOW,
                lexeme = lexeme,
                type_name = type_name
            );

            last_col = curr_col + length;
        }

        // Print remaining text in line
        if (last_col as usize) < line.len() {
            eprint!("{}", &line[last_col as usize..]);
        }
        eprintln!();
    }
}
