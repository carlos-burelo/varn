use tower_lsp_f::lsp_types::{CodeLens, Command, Position, Range, Uri};
use varn_checker::SymbolKind;

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

        if sym.name() == "main" {
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "▶ Run Main".to_string(),
                    tooltip: None,
                    command: "varn.runMain".to_string(),
                    arguments: Some(vec![uri_arg()]),
                }),
                data: None,
            });
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "⏱️ Benchmark".to_string(),
                    tooltip: None,
                    command: "varn.runBenchmark".to_string(),
                    arguments: Some(vec![uri_arg()]),
                }),
                data: None,
            });
        } else if sym.name().starts_with("test_") {
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
        } else if sym.name().starts_with("bench_") {
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "⏱️ Benchmark".to_string(),
                    tooltip: None,
                    command: "varn.runBenchmark".to_string(),
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
        ) {
            if let Some(ws) = workspace {
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
    }

    lenses
}

fn count_references(state: &DocumentState, workspace: &Workspace, line: u32, col: u32) -> usize {
    crate::features::references::build_references(state, workspace, line, col)
        .map(|locs| locs.len())
        .unwrap_or(0)
}
