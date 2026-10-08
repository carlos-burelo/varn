#[allow(deprecated)]
use tower_lsp_f::lsp_types::{CompletionResponse, Contents, Definition, MarkedString};
use varn_debug::colors::{DIM, GREEN, R, YELLOW};
use varn_debug::flags::Verb;

pub enum Outcome {
    Items {
        branch: &'static str,
        labels: Vec<String>,
    },
    Single {
        found: bool,
    },
}

impl Outcome {
    pub fn labels(&self) -> &[String] {
        match self {
            Outcome::Items { labels, .. } => labels,
            Outcome::Single { .. } => &[],
        }
    }
}

pub struct Ctx<'a> {
    pub analysis: &'a varn_lsp::document::DocumentAnalysis,
    pub index: &'a varn_lsp::index::ProjectIndex,
    pub line: u32,
    pub col: u32,
    pub trig_char: Option<&'static str>,
    pub trig_kind: &'static str,
    pub stale_mark: &'static str,
}

pub fn run_query(verb: Verb, ctx: &Ctx) -> (Vec<String>, Outcome) {
    match verb {
        Verb::Complete => complete(ctx),
        Verb::Hover => single(
            "hov",
            varn_lsp::features::hover::build_hover(ctx.analysis, ctx.line, ctx.col)
                .map(|h| one_line(&hover_text(&h), 100)),
        ),
        Verb::Definition => single(
            "def",
            varn_lsp::features::definition::build_goto_definition(
                ctx.analysis,
                None,
                ctx.line,
                ctx.col,
            )
            .map(definition_text),
        ),
        Verb::Declaration => single(
            "dec",
            varn_lsp::features::definition::build_goto_definition(
                ctx.analysis,
                None,
                ctx.line,
                ctx.col,
            )
            .map(definition_text),
        ),
        Verb::Highlight => {
            let hls = varn_lsp::features::document_highlight::build_document_highlights(
                ctx.analysis,
                ctx.line,
                ctx.col,
            );
            let found = !hls.is_empty();
            let text = if found {
                format!(
                    "{} [{}]",
                    hls.len(),
                    hls.iter()
                        .map(|h| format!("{:?}", h.kind))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else {
                "—".to_string()
            };
            (vec![tag("hl", text)], Outcome::Single { found })
        }
        Verb::Signature => single(
            "sig",
            varn_lsp::features::signature_help::build_signature_help(
                ctx.analysis,
                ctx.line,
                ctx.col,
            )
            .and_then(|s| {
                let idx = s.active_signature.unwrap_or(0) as usize;
                s.signatures
                    .get(idx)
                    .or_else(|| s.signatures.first())
                    .map(|info| {
                        format!(
                            "{}  ({} sigs)",
                            one_line(&info.label, 100),
                            s.signatures.len()
                        )
                    })
            }),
        ),
    }
}

fn single(tag_name: &str, value: Option<String>) -> (Vec<String>, Outcome) {
    let found = value.is_some();
    (
        vec![tag(tag_name, value.unwrap_or_else(|| "—".to_string()))],
        Outcome::Single { found },
    )
}

fn tag(name: &str, value: String) -> String {
    format!(
        "    {YELLOW}{name}{RESET} {value}",
        YELLOW = YELLOW,
        RESET = R
    )
}

fn complete(ctx: &Ctx) -> (Vec<String>, Outcome) {
    let (resp, log) = varn_lsp::features::completion::build_completion_response(
        ctx.analysis,
        ctx.line,
        ctx.col,
        ctx.trig_char,
        ctx.trig_kind.to_string(),
        Some(ctx.index),
    );
    let mut lines = Vec::new();
    let mut branch = "?";
    let mut labels = Vec::new();
    if let Some(msg) = log {
        branch = log_branch(&msg);
        lines.push(format!(
            "    {DIM}{msg}{}{RESET}",
            ctx.stale_mark,
            DIM = DIM,
            RESET = R
        ));
    }
    match &resp {
        Some(CompletionResponse::CompletionItemList(items)) => {
            labels = items.iter().map(|i| i.label.clone()).collect();
            lines.push(format!(
                "    {GREEN}show ({}){RESET} [{}]",
                items.len(),
                varn_lsp::features::completion::preview_items(items, 6),
                GREEN = GREEN,
                RESET = R
            ));
        }
        Some(CompletionResponse::CompletionList(list)) => {
            labels = list.items.iter().map(|i| i.label.clone()).collect();
            lines.push(format!(
                "    {GREEN}show-list ({}){RESET} [{}]",
                list.items.len(),
                varn_lsp::features::completion::preview_items(&list.items, 6),
                GREEN = GREEN,
                RESET = R
            ));
        }
        None => lines.push(format!(
            "    {DIM}show: suppressed{RESET}",
            DIM = DIM,
            RESET = R
        )),
    }
    (lines, Outcome::Items { branch, labels })
}

fn definition_text(d: Definition) -> String {
    match d {
        Definition::Location(l) => format!(
            "{}:{}:{}",
            short_uri(l.uri.as_str()),
            l.range.start.line + 1,
            l.range.start.character + 1
        ),
        Definition::LocationList(v) => {
            let first = v.first().map(|l| {
                format!(
                    "{}:{}:{}",
                    short_uri(l.uri.as_str()),
                    l.range.start.line + 1,
                    l.range.start.character + 1
                )
            });
            match first {
                Some(f) => format!("{f} +{} more", v.len().saturating_sub(1)),
                None => "empty".to_string(),
            }
        }
    }
}

fn log_branch(msg: &str) -> &'static str {
    if msg.contains(" ipath ") {
        "ipath"
    } else if msg.contains(" nimport ") {
        "nimport"
    } else if msg.contains("suppressed=") {
        "suppressed"
    } else if msg.contains("ccref=") {
        "ccref"
    } else if msg.contains(" dot ") {
        "dot"
    } else if msg.contains("postfix-only") {
        "postfix"
    } else if msg.contains(" pattern ") {
        "pattern"
    } else if msg.contains("callargs") {
        "callargs"
    } else if msg.contains(" general ") {
        "general"
    } else {
        "?"
    }
}

fn hover_text(h: &tower_lsp_f::lsp_types::Hover) -> String {
    match &h.contents {
        Contents::MarkedString(c) => marked_text(c),
        Contents::MarkedStringList(arr) => {
            arr.iter().map(marked_text).collect::<Vec<_>>().join(" | ")
        }
        Contents::MarkupContent(m) => m.value.clone(),
    }
}

#[allow(deprecated)]
#[allow(deprecated)]
fn marked_text(ms: &MarkedString) -> String {
    match ms {
        MarkedString::String(s) => s.clone(),
        MarkedString::MarkedStringWithLanguage(ls) => ls.value.clone(),
    }
}

fn one_line(s: &str, n: usize) -> String {
    s.replace("```varn\n", "")
        .replace("```Varn\n", "")
        .replace('\n', " ")
        .chars()
        .take(n)
        .collect()
}

fn short_uri(uri: &str) -> String {
    uri.rsplit(['/', '\\']).next().unwrap_or(uri).to_string()
}
