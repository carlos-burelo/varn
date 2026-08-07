use tower_lsp::lsp_types::FormattingOptions;
use varn_lsp::features::formatting::build_formatting;

#[test]
fn test_document_formatting() {
    let unformatted = "function calculate(n: int): int {\nlet x = 42\n  if (x > 0) {\nprint(x)\n}\nreturn x + n\n}";
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        ..Default::default()
    };

    let edits = build_formatting(unformatted, options);
    assert!(edits.is_some());
    let new_text = &edits.unwrap()[0].new_text;

    assert!(new_text.contains("    let x = 42"));
    assert!(new_text.contains("        print(x)"));
    assert!(new_text.contains("    return x + n"));
}
