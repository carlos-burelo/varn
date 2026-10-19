use tower_lsp_f::lsp_types::{CodeLens, Command, Position, Range, Uri};
use varn_sem::symbol::SymbolKind;

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn build_code_lenses(
    uri: &Uri,
    analysis: &DocumentState,
    workspace: Option<&Workspace>,
) -> Vec<CodeLens> {
    let mut lenses = Vec::new();
    let uri_str = uri.to_string();
    let uri_arg = || serde_json::Value::String(uri_str.clone());

    lenses.push(CodeLens {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
        command: Some(Command {
            title: "▶ Run File".to_string(),
            tooltip: None,
            command: "varn.runFile".to_string(),
            arguments: Some(vec![uri_arg()]),
        }),
        data: None,
    });

    lenses.push(CodeLens {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
        command: Some(Command {
            title: "🔍 View Bytecode".to_string(),
            tooltip: None,
            command: "varn.showBytecode".to_string(),
            arguments: Some(vec![uri_arg()]),
        }),
        data: None,
    });

    lenses.push(CodeLens {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
        command: Some(Command {
            title: "🔍 View SSA".to_string(),
            tooltip: None,
            command: "varn.showSSA".to_string(),
            arguments: Some(vec![uri_arg()]),
        }),
        data: None,
    });

    for range in entry_guards(analysis) {
        lenses.push(CodeLens {
            range,
            command: Some(Command {
                title: "▶ Run as Entry".to_string(),
                tooltip: None,
                command: "varn.runMain".to_string(),
                arguments: Some(vec![uri_arg()]),
            }),
            data: None,
        });
        lenses.push(CodeLens {
            range,
            command: Some(Command {
                title: "⏱️ Benchmark".to_string(),
                tooltip: None,
                command: "varn.runBenchmark".to_string(),
                arguments: Some(vec![uri_arg()]),
            }),
            data: None,
        });
    }

    for sym in analysis.symbols() {
        if sym.line() == u32::MAX || sym.is_from_stdlib() {
            continue;
        }

        let sym_range = Range {
            start: Position {
                line: sym.line(),
                character: sym.col(),
            },
            end: Position {
                line: sym.line(),
                character: sym.col() + sym.name().len() as u32,
            },
        };

        if sym.is_test() {
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "▶ Run Test".to_string(),
                    tooltip: None,
                    command: "varn.runTest".to_string(),
                    arguments: Some(vec![
                        uri_arg(),
                        serde_json::Value::String(sym.name().to_owned()),
                    ]),
                }),
                data: None,
            });
        }

        if matches!(
            sym.kind(),
            SymbolKind::Function | SymbolKind::Class | SymbolKind::Interface
        ) && let Some(ws) = workspace
        {
            let ref_count = count_references(analysis, ws, sym.line(), sym.col());
            if ref_count > 0 {
                let title = if ref_count == 1 {
                    "1 reference".to_string()
                } else {
                    format!("{} references", ref_count)
                };
                lenses.push(CodeLens {
                    range: sym_range,
                    command: Some(Command {
                        title,
                        tooltip: None,
                        command: "varn.findReferences".to_string(),
                        arguments: Some(vec![
                            uri_arg(),
                            serde_json::Value::from(sym.line()),
                            serde_json::Value::from(sym.col()),
                        ]),
                    }),
                    data: None,
                });
            }
        }
    }

    lenses
}

fn count_references(state: &DocumentState, workspace: &Workspace, line: u32, col: u32) -> usize {
    crate::features::references::build_references(state, workspace, line, col)
        .map(|locs| locs.len())
        .unwrap_or(0)
}

fn entry_guards(state: &DocumentState) -> Vec<Range> {
    use varn_core::ast::MetaProperty;
    use varn_core::{ContextualKeyword, TokenKind};
    let tokens = &state.tokens;
    let text = |i: usize| crate::document::token_lexeme(&state.source, &tokens[i]);
    tokens
        .windows(5)
        .enumerate()
        .filter(|(i, w)| {
            w[0].kind == TokenKind::Import
                && w[1].kind == TokenKind::Dot
                && ContextualKeyword::of(text(i + 2)) == Some(ContextualKeyword::Meta)
                && w[3].kind == TokenKind::Dot
                && MetaProperty::of(text(i + 4)) == Some(MetaProperty::Main)
        })
        .map(|(_, w)| Range {
            start: Position {
                line: w[0].line,
                character: w[0].col,
            },
            end: Position {
                line: w[4].line,
                character: w[4].col + w[4].length,
            },
        })
        .collect()
}
