#![cfg(feature = "lsp-debug")]

use tower_lsp::lsp_types::HoverContents;
use varn_debug::colors::{BLUE, BOLD, C_TYPES, DIM, GREEN, R, YELLOW};
use varn_debug::flags::DebugFlags;

pub fn debug_lsp(path: &str, source: &str, flags: &DebugFlags) {
    use varn_debug::colors::{footer, header};
    header(C_TYPES, "lsp analysis dashboard", path);

    let uri = varn_modules::resolver::path_to_uri(path);

    let analysis = varn_lsp::pipeline::run_pipeline(source.to_string(), uri);

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
            eprintln!(
                "    {DIM}{:<12}{RESET} {BOLD}{:<20}{RESET} : {YELLOW}{:<20}{RESET} {DIM}(ln:{}){RESET}",
                kind_str,
                sym.name,
                sym.type_str,
                sym.line + 1,
                DIM = DIM,
                RESET = R,
                BOLD = BOLD,
                YELLOW = YELLOW
            );
        }
        eprintln!();
    }

    if flags.lsp_hovers {
        eprintln!(
            "  {BOLD}{BLUE}Simulated Hovers:{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        for tok in &analysis.tokens {
            if tok.kind == varn_core::TokenKind::Identifier || tok.kind.can_be_identifier() {
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

    if flags.lsp_types && !analysis.db.expr_types.is_empty() {
        eprintln!(
            "  {BOLD}{BLUE}Expression Types (Detailed Mapping):{RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            RESET = R
        );
        let mut sorted: Vec<_> = analysis.db.expr_types.iter().collect();
        sorted.sort_by_key(|(off, _)| *off);
        for (off, ty) in sorted {
            eprintln!(
                "    {DIM}offset {:>4}{RESET} : {YELLOW}{}{RESET}",
                off,
                ty,
                DIM = DIM,
                RESET = R,
                YELLOW = YELLOW
            );
        }
        eprintln!();
    }

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
            let lexeme = find_lexeme(&analysis, curr_line, curr_col, chunk[2]);
            eprintln!(
                "    {DIM}({:>2}:{:>2}){RESET} {YELLOW}{:<15}{RESET} → {GREEN}{}{RESET}",
                curr_line + 1,
                curr_col + 1,
                lexeme,
                type_name,
                DIM = DIM,
                RESET = R,
                YELLOW = YELLOW,
                GREEN = GREEN
            );
        }
        eprintln!();
    }

    if flags.lsp_colorize {
        eprintln!(
            "  {BOLD}{BLUE}Annotated Source:{RESET}  {DIM}(token#tag  tags: kw=keyword  ty=type  fn=function  cls=class  var=variable  param  prop=property  num=number  str=string  em=enum  ns=namespace  iface  tp=type_param){RESET}",
            BOLD = BOLD,
            BLUE = BLUE,
            DIM = DIM,
            RESET = R
        );

        let mut abs_spans: Vec<(u32, u32, u32, u32)> = Vec::new();
        {
            let mut curr_line = 0u32;
            let mut curr_col = 0u32;
            for chunk in sem_tokens.chunks_exact(5) {
                curr_line += chunk[0];
                curr_col = if chunk[0] == 0 { curr_col + chunk[1] } else { chunk[1] };
                abs_spans.push((curr_line, curr_col, chunk[2], chunk[3]));
            }
        }

        let mut line_spans: std::collections::HashMap<u32, Vec<(u32, u32, u32)>> =
            std::collections::HashMap::new();
        for (line, col, len, tt) in &abs_spans {
            line_spans.entry(*line).or_default().push((*col, *len, *tt));
        }

        let source_lines: Vec<&str> = source.lines().collect();
        let gutter = source_lines.len().to_string().len();

        for (line_idx, line_text) in source_lines.iter().enumerate() {
            let line_num = line_idx as u32;
            let empty = vec![];
            let mut spans = line_spans.get(&line_num).unwrap_or(&empty).clone();
            spans.sort_by_key(|(col, _, _)| *col);
            let colored = annotate_line(line_text, &spans);
            eprintln!(
                "  {DIM}{:>gutter$} │{RESET} {}",
                line_idx + 1,
                colored,
                DIM = DIM,
                RESET = R,
                gutter = gutter
            );
        }
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

fn annotate_line(line: &str, spans: &[(u32, u32, u32)]) -> String {
    if spans.is_empty() {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    let char_len = chars.len();
    let mut result = String::new();
    let mut pos = 0usize;
    for &(col, len, tt) in spans {
        let col = col as usize;
        let end = (col + len as usize).min(char_len);
        if col > char_len {
            break;
        }
        if col > pos {
            result.extend(&chars[pos..col]);
        }
        let token_text: String = chars[col..end].iter().collect();
        result.push_str(&token_text);
        result.push_str("\x1b[34m#");
        result.push_str(tt_to_tag(tt));
        result.push_str("\x1b[0m");
        pos = end;
    }
    if pos < char_len {
        result.extend(&chars[pos..]);
    }
    result
}

fn tt_to_tag(tt: u32) -> &'static str {
    match tt {
        0 => "kw",
        1 => "ty",
        2 => "var",
        3 => "fn",
        4 => "cls",
        5 => "param",
        6 => "prop",
        7 => "num",
        8 => "str",
        9 => "em",
        10 => "ns",
        11 => "iface",
        12 => "tp",
        _ => "?",
    }
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
