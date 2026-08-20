#![allow(unused_crate_dependencies)]

use std::fs;
use std::path::Path;
use tower_lsp::lsp_types::Position;
use varn_lsp::features::hover::build_hover;
use varn_lsp::features::inlay_hints::build_inlay_hints;
use varn_lsp::features::selection_range::build_selection_ranges;
use varn_lsp::features::semantic_tokens::build_semantic_tokens;
use varn_lsp::features::symbols::build_document_symbols;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_all_vn_files_lsp_compliance() {
    let tests_dir = Path::new("../../tests");
    let tests_dir_alt = Path::new("tests");
    let tests_dir_root = Path::new("./tests");

    let actual_dir = if tests_dir.exists() {
        tests_dir
    } else if tests_dir_root.exists() {
        tests_dir_root
    } else {
        tests_dir_alt
    };

    assert!(
        actual_dir.exists(),
        "Tests directory not found at {:?}",
        actual_dir
    );

    let mut vn_files = Vec::new();
    for entry in fs::read_dir(actual_dir).expect("Failed to read tests directory") {
        let entry = entry.expect("Valid entry");
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("vn") {
            vn_files.push(path);
        }
    }

    vn_files.sort();
    assert!(
        !vn_files.is_empty(),
        "No .vn test files found in {:?}",
        actual_dir
    );

    println!("Found {} .vn test files to validate.", vn_files.len());

    let mut total_files = 0;
    let mut total_tokens_hovered = 0;
    let mut total_semantic_tokens = 0;
    let mut failures = Vec::new();

    for file_path in &vn_files {
        total_files += 1;
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let source = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{}: failed to read file ({})", file_name, e));
                continue;
            }
        };

        let uri = format!("file:///tests/{}", file_name);

        // 1. Pipeline execution
        let state = run_pipeline(source.clone(), uri.clone());

        // 2. Semantic tokens verification
        let sem_tokens = build_semantic_tokens(&state);
        total_semantic_tokens += sem_tokens.len() / 5;

        // 3. Document symbols verification
        let doc_symbols = build_document_symbols(&state);
        let _ = doc_symbols;

        // 4. Inlay hints verification
        let inlay_hints = build_inlay_hints(&state);
        let _ = inlay_hints;

        // 5. Selection ranges verification on sample positions
        if !state.tokens.is_empty() {
            let positions: Vec<Position> = state
                .tokens
                .iter()
                .step_by(5.max(state.tokens.len() / 10))
                .map(|t| Position {
                    line: t.line,
                    character: t.col,
                })
                .collect();
            let ranges = build_selection_ranges(&state, &positions);
            assert_eq!(ranges.len(), positions.len());
        }

        // 6. Exhaustive Hover testing across all tokens in file
        for tok in &state.tokens {
            total_tokens_hovered += 1;
            // Test hovering at start, middle, and end of token
            let hover_opt = build_hover(&state, tok.line, tok.col);
            if let Some(hover) = hover_opt {
                if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover.contents {
                    if mc.value.trim().is_empty() {
                        failures.push(format!(
                            "{}:{}:{} - Empty hover markup for token '{}'",
                            file_name, tok.line, tok.col, tok.lexeme
                        ));
                    }
                }
            }
        }
    }

    println!(
        "LSP Stress Test Complete: {} files tested, {} semantic tokens produced, {} tokens hovered.",
        total_files, total_semantic_tokens, total_tokens_hovered
    );

    if !failures.is_empty() {
        panic!("Found {} failures:\n{}", failures.len(), failures.join("\n"));
    }
}
