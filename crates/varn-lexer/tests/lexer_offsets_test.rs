use std::fs;
use std::path::Path;
use varn_core::TokenKind;

fn verify_source_lexing(filename: &str, source: &str) {
    let (tokens, _lexeme_buf, diags) = varn_lexer::scan(source, filename);

    assert!(
        diags.is_empty(),
        "Lexer produced diagnostics on valid file {filename}: {:?}",
        diags
    );

    let src_bytes = source.as_bytes();
    let mut prev_end_offset = 0u32;

    for (i, tok) in tokens.iter().enumerate() {
        let start = tok.range.start.offset as usize;
        let end = tok.range.end.offset as usize;

        // 1. Range bounds sanity
        assert!(
            tok.range.start.offset <= tok.range.end.offset,
            "[{filename}] Token #{i} ({:?}) has start offset ({}) > end offset ({})",
            tok.kind,
            tok.range.start.offset,
            tok.range.end.offset
        );

        assert!(
            end <= src_bytes.len(),
            "[{filename}] Token #{i} ({:?}) end offset ({}) exceeds source length ({})",
            tok.kind,
            end,
            src_bytes.len()
        );

        // 2. Monotonic non-decreasing order
        assert!(
            tok.range.start.offset >= prev_end_offset,
            "[{filename}] Token #{i} ({:?}) start offset ({}) < previous end offset ({})",
            tok.kind,
            tok.range.start.offset,
            prev_end_offset
        );
        prev_end_offset = tok.range.end.offset;

        // 3. Line number validation
        let expected_start_line = (src_bytes[..start].iter().filter(|&&b| b == b'\n').count() + 1) as u32;
        assert_eq!(
            tok.range.start.line, expected_start_line,
            "[{filename}] Token #{i} ({:?}) start line mismatch: got {}, expected {}",
            tok.kind, tok.range.start.line, expected_start_line
        );

        let expected_end_line = (src_bytes[..end].iter().filter(|&&b| b == b'\n').count() + 1) as u32;
        assert_eq!(
            tok.range.end.line, expected_end_line,
            "[{filename}] Token #{i} ({:?}) end line mismatch: got {}, expected {}",
            tok.kind, tok.range.end.line, expected_end_line
        );

        // 4. Column validation (start of line offset to start offset)
        let line_start_offset = src_bytes[..start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let expected_start_col = (start - line_start_offset) as u32;
        assert_eq!(
            tok.range.start.column, expected_start_col,
            "[{filename}] Token #{i} ({:?}) start col mismatch: got {}, expected {}",
            tok.kind, tok.range.start.column, expected_start_col
        );

        // 5. Lexeme content verification
        let slice = std::str::from_utf8(&src_bytes[start..end])
            .unwrap_or_else(|_| panic!("[{filename}] Token #{i} ({:?}) is not valid UTF-8", tok.kind));

        match tok.kind {
            TokenKind::EOF => {
                assert_eq!(start, end);
            }
            TokenKind::Identifier => {
                assert!(!slice.is_empty(), "[{filename}] Identifier slice is empty");
            }
            TokenKind::IntegerLiteral => {
                assert!(slice.chars().next().unwrap().is_ascii_digit());
            }
            TokenKind::Str => {
                assert!(slice.starts_with('"') || slice.starts_with('\''), "[{filename}] String literal didn't start with quote: {slice}");
            }
            _ => {}
        }
    }
}

#[test]
fn test_all_vn_files_lexer_offset_invariants() {
    let tests_dir = Path::new("../../tests").canonicalize().or_else(|_| Path::new("tests").canonicalize()).unwrap();
    let mut count = 0;

    for entry in fs::read_dir(tests_dir).expect("failed to read tests directory") {
        let entry = entry.expect("valid entry");
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "vn") {
            let filename = path.file_name().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&path).expect("read file");
            verify_source_lexing(&filename, &source);
            count += 1;
        }
    }

    assert!(count >= 80, "Expected to test >= 80 files, tested {count}");
    println!("Verified {count} .vn files for strict lexer offset/position invariants.");
}

#[test]
fn test_synthetic_unicode_and_tricky_syntax() {
    let synthetic = r#"
// Testing unicode identifiers and comments: año, pi_π, emojis 🚀
const año: int = 2026;
const π_valor: float = 3.14159;

/* Multiline comment with
   newlines and unicode: Español, 日本語, 한국어 */
function calcular(base: int, extra: int = 10): int {
    const template = `Resultado para ${año}: ${base + extra}`;
    const hex = 0xFF_AA;
    const bin = 0b1010_0101;
    const oct = 0o755;
    const big = 1234567890123456789n;
    const dec = 123.456m;
    const regex = /^[a-z]+_\d+$/i;
    return base + extra;
}
"#;
    verify_source_lexing("synthetic_test.vn", synthetic);
}
