use std::alloc::{alloc, dealloc, Layout};

use crate::scanner::Scanner;

#[no_mangle]
pub unsafe extern "C" fn varn_scan(
    source_ptr: *const u8,
    source_len: u32,
    _filename_ptr: *const u8,
    _filename_len: u32,
    out_len: *mut u32,
) -> *mut u8 {
    let src = unsafe { std::slice::from_raw_parts(source_ptr, source_len as usize) };

    let mut scanner = Scanner::new(src);
    let (tokens, lexemes) = scanner.scan_all();

    let token_count = tokens.len() as u32;
    let lexeme_len = lexemes.len() as u32;

    const HEADER: usize = 8;
    const PER_TOKEN: usize = 9 * 4;

    let total = HEADER + tokens.len() * PER_TOKEN + lexemes.len();

    let layout = match Layout::from_size_align(total, 4) {
        Ok(l) => l,
        Err(_) => {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        }
    };

    let buf = unsafe { alloc(layout) };
    if buf.is_null() {
        unsafe { *out_len = 0 };
        return std::ptr::null_mut();
    }

    unsafe {
        let u32_ptr = buf as *mut u32;
        *u32_ptr = token_count;
        *u32_ptr.add(1) = lexeme_len;

        let tok_ptr = u32_ptr.add(2);
        for (i, t) in tokens.iter().enumerate() {
            let b = tok_ptr.add(i * 9);
            *b = t.kind;
            *b.add(1) = t.start_line;
            *b.add(2) = t.start_col;
            *b.add(3) = t.start_offset;
            *b.add(4) = t.end_line;
            *b.add(5) = t.end_col;
            *b.add(6) = t.end_offset;
            *b.add(7) = t.lex_offset;
            *b.add(8) = t.lex_len;
        }

        let lex_dst = buf.add(HEADER + tokens.len() * PER_TOKEN);
        std::ptr::copy_nonoverlapping(lexemes.as_ptr(), lex_dst, lexemes.len());

        *out_len = total as u32;
    }

    buf
}

#[no_mangle]
pub unsafe extern "C" fn varn_free(ptr: *mut u8, len: u32) {
    if !ptr.is_null() && len > 0 {
        let layout = Layout::from_size_align(len as usize, 4)
            .expect("varn_free: invalid layout — was len tampered with?");
        unsafe { dealloc(ptr, layout) };
    }
}

pub fn scan(source: &str, filename: &str) -> (Vec<varn_core::Token>, Vec<varn_core::Diagnostic>) {
    scan_with_config(source, filename, crate::scanner::LexerConfig::default())
}

fn parse_num_lexeme(kind: varn_core::TokenKind, lexeme: &str) -> Option<varn_core::ParsedNumber> {
    use varn_core::{ParsedNumber, TokenKind};
    let cleaned = lexeme.replace('_', "");
    match kind {
        TokenKind::IntegerLiteral => cleaned.parse::<i64>().ok().map(ParsedNumber::Int),
        TokenKind::BinaryLiteral => {
            let digits = cleaned.strip_prefix("0b").or_else(|| cleaned.strip_prefix("0B"))?;
            i64::from_str_radix(digits, 2).ok().map(ParsedNumber::Int)
        }
        TokenKind::OctalLiteral => {
            let digits = cleaned.strip_prefix("0o").or_else(|| cleaned.strip_prefix("0O"))?;
            i64::from_str_radix(digits, 8).ok().map(ParsedNumber::Int)
        }
        TokenKind::HexLiteral => {
            let digits = cleaned.strip_prefix("0x").or_else(|| cleaned.strip_prefix("0X"))?;
            i64::from_str_radix(digits, 16).ok().map(ParsedNumber::Int)
        }
        TokenKind::FloatLiteral => cleaned.parse::<f64>().ok().map(ParsedNumber::Float),
        _ => None,
    }
}

pub fn scan_with_config(
    source: &str,
    _filename: &str,
    config: crate::scanner::LexerConfig,
) -> (Vec<varn_core::Token>, Vec<varn_core::Diagnostic>) {
    use varn_core::{SourceLocation, SourceRange, Token, TokenKind};

    let src_bytes = source.as_bytes();

    let mut scanner = Scanner::with_config(src_bytes, config);
    let (records, lexeme_buf) = scanner.scan_all();
    let diagnostics = scanner.diagnostics.take();

    let tokens: Vec<varn_core::Token> = records
        .into_iter()
        .map(|r| {
            let lexeme_bytes =
                &lexeme_buf[r.lex_offset as usize..(r.lex_offset + r.lex_len) as usize];
            let lexeme = String::from_utf8_lossy(lexeme_bytes).into_owned();

            let start = SourceLocation {
                line: r.start_line,
                column: r.start_col,
                offset: r.start_offset,
            };
            let end = SourceLocation {
                line: r.end_line,
                column: r.end_col,
                offset: r.end_offset,
            };
            let range = SourceRange { start, end };

            let kind = TokenKind::from_u32(r.kind);
            let parsed_num = parse_num_lexeme(kind, &lexeme);

            Token {
                kind,
                lexeme: lexeme.into(),
                range,
                parsed_num,
            }
        })
        .collect();

    (tokens, diagnostics)
}
