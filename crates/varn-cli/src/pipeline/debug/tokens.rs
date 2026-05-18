use varn_core::Token;
use crate::pipeline::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_TOKENS};

pub fn debug_tokens(tokens: &[Token]) {
    use super::super::colors::{footer, header};
    header(C_TOKENS, "tokens", "");

    eprintln!(
        "  {BOLD}{:<5} │ {:<10} │ {:<20} │ Lexeme{RESET}",
        "Idx", "Loc", "Kind"
    );
    eprintln!("  {}", "─".repeat(70));

    for (i, tok) in tokens.iter().enumerate() {
        let loc = format!("{}:{}", tok.range.start.line, tok.range.start.column);
        let kind = format!("{:?}", tok.kind);
        
        eprintln!(
            "  {DIM}{:<5}{RESET} │ {YELLOW}{:<10}{RESET} │ {BLUE}{:<20}{RESET} │ {:?}",
            i,
            loc,
            kind,
            tok.lexeme
        );
    }

    footer(C_TOKENS, &format!("{} tokens scanned", tokens.len()));
}
