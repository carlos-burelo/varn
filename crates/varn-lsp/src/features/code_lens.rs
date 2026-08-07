use tower_lsp::lsp_types::{CodeLens, Command, Position, Range, Url};
use crate::document::DocumentState;

pub fn build_code_lenses(uri: &Url, analysis: &DocumentState) -> Vec<CodeLens> {
    let mut lenses = Vec::new();

    // 1. Top-level file lens: "▶ Run File"
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
            command: "varn.runFile".to_string(),
            arguments: Some(vec![serde_json::to_value(uri.to_string()).unwrap()]),
        }),
        data: None,
    });

    // 2. Function-level lenses for main() and test_*() functions
    for sym in &analysis.symbols {
        if sym.line == u32::MAX {
            continue;
        }

        if sym.name == "main" {
            lenses.push(CodeLens {
                range: Range {
                    start: Position {
                        line: sym.line,
                        character: sym.col,
                    },
                    end: Position {
                        line: sym.line,
                        character: sym.col + sym.name.len() as u32,
                    },
                },
                command: Some(Command {
                    title: "▶ Run Main".to_string(),
                    command: "varn.runMain".to_string(),
                    arguments: Some(vec![serde_json::to_value(uri.to_string()).unwrap()]),
                }),
                data: None,
            });
        } else if sym.name.starts_with("test_") {
            lenses.push(CodeLens {
                range: Range {
                    start: Position {
                        line: sym.line,
                        character: sym.col,
                    },
                    end: Position {
                        line: sym.line,
                        character: sym.col + sym.name.len() as u32,
                    },
                },
                command: Some(Command {
                    title: "▶ Run Test".to_string(),
                    command: "varn.runTest".to_string(),
                    arguments: Some(vec![
                        serde_json::to_value(uri.to_string()).unwrap(),
                        serde_json::to_value(sym.name.clone()).unwrap(),
                    ]),
                }),
                data: None,
            });
        }
    }

    lenses
}
