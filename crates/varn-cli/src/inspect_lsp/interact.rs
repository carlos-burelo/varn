use varn_core::term::terminal;
use varn_core::TokenKind;
use varn_debug::colors::{footer, header, BOLD, C_TYPES, DIM, R};
use varn_debug::flags::{DebugFlags, Step, Verb};

use super::expect;
use super::queries::{self, Ctx};

const MAX_CURSORS: usize = 64;

pub fn debug_interact(path: &str, source: &str, flags: &DebugFlags) {
    header(C_TYPES, "lsp editor session", path);

    let uri = varn_modules::resolver::path_to_uri(path);
    let analysis = varn_lsp::pipeline::run_pipeline(source.to_string(), uri.clone());
    let mut index = varn_lsp::index::ProjectIndex::new();
    index.update_file(&uri, &analysis);

    let steps: Vec<Step> = if flags.lsp_cursors.is_empty() {
        auto_cursors(&analysis)
    } else {
        flags.lsp_cursors.clone()
    };
    terminal::log(format!(
        "  {DIM}{}{RESET} step{S} ({})",
        if flags.lsp_cursors.is_empty() {
            "auto positions"
        } else {
            "given positions"
        },
        steps.len(),
        S = if steps.len() == 1 { "" } else { "s" },
        DIM = DIM,
        RESET = R
    ));

    let mut ok = 0u32;
    let mut fail = 0u32;
    for step in steps.into_iter().take(MAX_CURSORS) {
        if run_step(&analysis, &index, source, &uri, &step) {
            ok += 1;
        } else {
            fail += 1;
        }
    }
    terminal::blank();
    footer(
        C_TYPES,
        &format!("session: {ok} ok, {fail} FAIL (live editor code paths)"),
    );
}

fn auto_cursors(analysis: &varn_lsp::document::DocumentAnalysis) -> Vec<Step> {
    analysis
        .tokens
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TokenKind::Dot | TokenKind::ColonColon | TokenKind::LParen | TokenKind::Comma
            )
        })
        .map(|t| Step {
            line: t.line,
            col: t.col + t.length,
            typed: None,
            ask: Verb::all(),
            expects: Vec::new(),
        })
        .collect()
}

fn run_step(
    analysis: &varn_lsp::document::DocumentAnalysis,
    index: &varn_lsp::index::ProjectIndex,
    source: &str,
    uri: &str,
    step: &Step,
) -> bool {
    let stale = build_stale(source, uri, step.line, step.col, step.typed.as_deref());
    let (analysis, index, stale_mark): (
        &varn_lsp::document::DocumentAnalysis,
        &varn_lsp::index::ProjectIndex,
        &str,
    ) = match &stale {
        Some((a, i)) => (a, i, " [stale replay]"),
        None => (analysis, index, ""),
    };
    let type_mark = step
        .typed
        .as_deref()
        .map(|t| format!(" type={t:?}"))
        .unwrap_or_default();
    let line_txt: String = source
        .lines()
        .nth(step.line as usize)
        .unwrap_or("")
        .trim()
        .chars()
        .take(72)
        .collect();
    terminal::log(format!(
        "  {BOLD}@ {}:{}{}{RESET} {DIM}{line_txt:?}{RESET}",
        step.line + 1,
        step.col + 1,
        type_mark,
        BOLD = BOLD,
        RESET = R,
        DIM = DIM
    ));

    let prev: Option<char> = match step.typed.as_deref() {
        Some(t) => t.chars().last(),
        None => source
            .lines()
            .nth(step.line as usize)
            .and_then(|l| l.chars().nth(step.col.saturating_sub(1) as usize)),
    };
    let (trig_char, trig_kind) = derive_trigger(prev);
    let ctx = Ctx {
        analysis,
        index,
        line: step.line,
        col: step.col,
        trig_char,
        trig_kind,
        stale_mark,
    };
    let mut outcomes = Vec::with_capacity(step.ask.len());
    for verb in &step.ask {
        let (lines, outcome) = queries::run_query(*verb, &ctx);
        for line in lines {
            terminal::log(line);
        }
        outcomes.push((*verb, outcome));
    }
    expect::eval(&outcomes, &step.expects)
}

fn derive_trigger(prev: Option<char>) -> (Option<&'static str>, &'static str) {
    match prev {
        Some('.') => (Some("."), "TriggerCharacter"),
        Some(':') => (Some(":"), "TriggerCharacter"),
        Some('(') => (None, "TriggerCharacter"),
        None | Some(_) => (None, "Invoked"),
    }
}

fn build_stale(
    source: &str,
    uri: &str,
    line: u32,
    col: u32,
    typed: Option<&str>,
) -> Option<(
    varn_lsp::document::DocumentAnalysis,
    varn_lsp::index::ProjectIndex,
)> {
    let typed = typed?;
    let stale_source = remove_typed(source, line, col, typed)?;
    let analysis = varn_lsp::pipeline::run_pipeline(stale_source, uri.to_owned());
    let mut index = varn_lsp::index::ProjectIndex::new();
    index.update_file(uri, &analysis);
    Some((analysis, index))
}

fn remove_typed(source: &str, line: u32, col: u32, typed: &str) -> Option<String> {
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    let chars: Vec<char> = lines.get(line as usize)?.chars().collect();
    let typed_chars: Vec<char> = typed.chars().collect();
    let end = (col as usize).min(chars.len());
    let start = end.checked_sub(typed_chars.len())?;
    if chars[start..end] != typed_chars[..] {
        return None;
    }
    let mut stale: Vec<char> = chars[..start].to_vec();
    stale.extend_from_slice(&chars[end..]);
    lines[line as usize] = stale.into_iter().collect();
    Some(lines.join("\n"))
}
