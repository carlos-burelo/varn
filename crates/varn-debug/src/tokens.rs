use crate::colors::{C_TOKENS, DIM, MAGENTA, RESET, YELLOW};
use varn_core::Token;

pub fn debug_tokens(tokens: &[Token], lexeme_buf: &[u8], filename: &str) {
    use crate::colors::{footer, header};
    header(C_TOKENS, "tokens", filename);

    eprintln!(
        "  {DIM}{:<5} │ {:<10} │ {:<20} │ Lexeme{RESET}",
        "Idx", "Loc", "Kind"
    );
    eprintln!("  {}", "─".repeat(70));

    for (i, tok) in tokens.iter().enumerate() {
        let loc = format!("{}:{}", tok.range.start.line + 1, tok.range.start.column);
        let lex = tok.get_lexeme(lexeme_buf);
        eprintln!(
            "  {DIM}{:<5} │ {:<10} │ {MAGENTA}{:<20}{RESET} │ {YELLOW}{:?}{RESET}",
            i,
            loc,
            format!("{:?}", tok.kind),
            lex,
            DIM = DIM,
            MAGENTA = MAGENTA,
            RESET = RESET,
            YELLOW = YELLOW
        );
    }

    footer(C_TOKENS, &format!("{} tokens scanned", tokens.len()));
}
