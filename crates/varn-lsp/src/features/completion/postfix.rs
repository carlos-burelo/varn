use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, Position, Range, TextEdit,
};

use crate::document::DocumentState;

pub struct PostfixTemplate {
    pub name: &'static str,
    pub detail: &'static str,
    pub description: &'static str,
    pub snippet_format: fn(&str) -> String,
}

pub static POSTFIX_TEMPLATES: &[PostfixTemplate] = &[
    PostfixTemplate {
        name: "let",
        detail: "Postfix let",
        description: "Bind expression to a mutable variable:\n```varn\nlet name = expr;\n```",
        snippet_format: |expr| format!("let ${{1:name}} = {expr};$0"),
    },
    PostfixTemplate {
        name: "const",
        detail: "Postfix const",
        description: "Bind expression to a constant:\n```varn\nconst name = expr;\n```",
        snippet_format: |expr| format!("const ${{1:name}} = {expr};$0"),
    },
    PostfixTemplate {
        name: "if",
        detail: "Postfix if",
        description: "Wrap expression in an if condition:\n```varn\nif expr {\n    $0\n}\n```",
        snippet_format: |expr| format!("if {expr} {{\n    $0\n}}"),
    },
    PostfixTemplate {
        name: "while",
        detail: "Postfix while",
        description: "Wrap expression in a while condition:\n```varn\nwhile expr {\n    $0\n}\n```",
        snippet_format: |expr| format!("while {expr} {{\n    $0\n}}"),
    },
    PostfixTemplate {
        name: "for",
        detail: "Postfix for-in",
        description: "Iterate over collection:\n```varn\nfor item in expr {\n    $0\n}\n```",
        snippet_format: |expr| format!("for ${{1:item}} in {expr} {{\n    $0\n}}"),
    },
    PostfixTemplate {
        name: "match",
        detail: "Postfix match",
        description: "Pattern match on expression:\n```varn\nmatch expr {\n    $0\n}\n```",
        snippet_format: |expr| format!("match {expr} {{\n    $0\n}}"),
    },
    PostfixTemplate {
        name: "return",
        detail: "Postfix return",
        description: "Return expression:\n```varn\nreturn expr;\n```",
        snippet_format: |expr| format!("return {expr};$0"),
    },
    PostfixTemplate {
        name: "pipe",
        detail: "Postfix pipeline",
        description: "Chain expression into pipeline:\n```varn\nexpr |> $0\n```",
        snippet_format: |expr| format!("{expr} |> $0"),
    },
    PostfixTemplate {
        name: "dbg",
        detail: "Postfix println (debug)",
        description: "Print expression to console:\n```varn\nprintln(expr);\n```",
        snippet_format: |expr| format!("println({expr});$0"),
    },
    PostfixTemplate {
        name: "log",
        detail: "Postfix println",
        description: "Print expression to console:\n```varn\nprintln(expr);\n```",
        snippet_format: |expr| format!("println({expr});$0"),
    },
    PostfixTemplate {
        name: "not",
        detail: "Postfix negate",
        description: "Negate boolean expression:\n```varn\n!expr\n```",
        snippet_format: |expr| format!("!{expr}$0"),
    },
    PostfixTemplate {
        name: "assert",
        detail: "Postfix assert",
        description: "Assert expression truthiness:\n```varn\nassert(expr);\n```",
        snippet_format: |expr| format!("assert({expr});$0"),
    },
];

pub fn build_postfix_completions(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Vec<CompletionItem> {
    let line_str = match state.source.lines().nth(line as usize) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let col_idx = (col as usize).min(line_str.len());
    let prefix = &line_str[..col_idx];

    let dot_pos = match prefix.rfind('.') {
        Some(pos) => pos,
        None => return Vec::new(),
    };

    let expr_str = prefix[..dot_pos].trim();
    if expr_str.is_empty() {
        return Vec::new();
    }

    // Find the start column of the expression on this line
    let expr_start_col = find_expr_start(prefix, dot_pos);
    let target_expr = prefix[expr_start_col..dot_pos].trim();
    if target_expr.is_empty() {
        return Vec::new();
    }

    let replace_range = Range {
        start: Position {
            line,
            character: expr_start_col as u32,
        },
        end: Position {
            line,
            character: col,
        },
    };

    let mut items = Vec::with_capacity(POSTFIX_TEMPLATES.len());

    for (idx, tmpl) in POSTFIX_TEMPLATES.iter().enumerate() {
        let snippet = (tmpl.snippet_format)(target_expr);
        items.push(CompletionItem {
            label: tmpl.name.to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(format!("{} (postfix)", tmpl.detail)),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: tmpl.description.to_string(),
            })),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text: snippet,
            })),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("zz_{:02}_{}", idx, tmpl.name)),
            ..Default::default()
        });
    }

    items
}

fn find_expr_start(prefix: &str, dot_pos: usize) -> usize {
    let bytes = prefix.as_bytes();
    let mut i = dot_pos;
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;

    while i > 0 {
        i -= 1;
        let b = bytes[i];
        match b {
            b')' => depth_paren += 1,
            b'(' => {
                if depth_paren > 0 {
                    depth_paren -= 1;
                } else {
                    return i + 1;
                }
            }
            b']' => depth_bracket += 1,
            b'[' => {
                if depth_bracket > 0 {
                    depth_bracket -= 1;
                } else {
                    return i + 1;
                }
            }
            b'}' => depth_brace += 1,
            b'{' => {
                if depth_brace > 0 {
                    depth_brace -= 1;
                } else {
                    return i + 1;
                }
            }
            b';' | b',' | b'=' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return i + 1;
            }
            _ => {}
        }
    }

    // Skip leading whitespace
    while i < dot_pos && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}
