use tower_lsp::lsp_types::{CodeLens, Command, Position, Range, Url};
use varn_checker::SymbolKind;

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn build_code_lenses(
    uri: &Url,
    analysis: &DocumentState,
    workspace: Option<&Workspace>,
) -> Vec<CodeLens> {
    let mut lenses = Vec::new();
    let uri_str = uri.to_string();

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
            arguments: Some(vec![serde_json::to_value(uri_str.clone()).unwrap()]),
        }),
        data: None,
    });

    // Top-level View Bytecode / SSA lenses
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
            command: "varn.showBytecode".to_string(),
            arguments: Some(vec![serde_json::to_value(uri_str.clone()).unwrap()]),
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
            command: "varn.showSSA".to_string(),
            arguments: Some(vec![serde_json::to_value(uri_str.clone()).unwrap()]),
        }),
        data: None,
    });

    // 2. Symbol-level lenses
    for sym in &analysis.symbols {
        if sym.line == u32::MAX || sym.is_from_stdlib {
            continue;
        }

        let sym_range = Range {
            start: Position {
                line: sym.line,
                character: sym.col,
            },
            end: Position {
                line: sym.line,
                character: sym.col + sym.name.len() as u32,
            },
        };

        if sym.name == "main" {
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "▶ Run Main".to_string(),
                    command: "varn.runMain".to_string(),
                    arguments: Some(vec![serde_json::to_value(uri_str.clone()).unwrap()]),
                }),
                data: None,
            });
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "⏱️ Benchmark".to_string(),
                    command: "varn.runBenchmark".to_string(),
                    arguments: Some(vec![serde_json::to_value(uri_str.clone()).unwrap()]),
                }),
                data: None,
            });
        } else if sym.name.starts_with("test_") {
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "▶ Run Test".to_string(),
                    command: "varn.runTest".to_string(),
                    arguments: Some(vec![
                        serde_json::to_value(uri_str.clone()).unwrap(),
                        serde_json::to_value(sym.name.clone()).unwrap(),
                    ]),
                }),
                data: None,
            });
        } else if sym.name.starts_with("bench_") {
            lenses.push(CodeLens {
                range: sym_range,
                command: Some(Command {
                    title: "⏱️ Benchmark".to_string(),
                    command: "varn.runBenchmark".to_string(),
                    arguments: Some(vec![
                        serde_json::to_value(uri_str.clone()).unwrap(),
                        serde_json::to_value(sym.name.clone()).unwrap(),
                    ]),
                }),
                data: None,
            });
        }

        // Reference count lens for top-level functions and classes
        if matches!(
            sym.kind,
            SymbolKind::Function | SymbolKind::Class | SymbolKind::Interface
        ) {
            if let Some(ws) = workspace {
                let ref_count = count_references(analysis, ws, sym.line, sym.col);
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
                            command: "varn.findReferences".to_string(),
                            arguments: Some(vec![
                                serde_json::to_value(uri_str.clone()).unwrap(),
                                serde_json::to_value(sym.line).unwrap(),
                                serde_json::to_value(sym.col).unwrap(),
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

fn count_references(
    state: &DocumentState,
    workspace: &Workspace,
    line: u32,
    col: u32,
) -> usize {
    crate::features::references::build_references(state, workspace, line, col)
        .map(|locs| locs.len())
        .unwrap_or(0)
}
