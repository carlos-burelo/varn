use varn_core::Token;
use varn_term::chalk::chalk;
use varn_term::terminal::Section;

pub fn debug_tokens(tokens: &[Token], lexeme_buf: &[u8]) {
    Section::new("tokens").subtitle("").color(|c| c.magenta()).print();

    varn_term::terminal::log(format!(
        "  {}",
        chalk(format!("{:<5} │ {:<10} │ {:<20} │ Lexeme", "Idx", "Loc", "Kind")).bold()
    ));
    varn_term::terminal::log(format!("  {}", "─".repeat(70)));

    for (i, tok) in tokens.iter().enumerate() {
        let loc = format!("{}:{}", tok.range.start.line, tok.range.start.column);
        let kind = format!("{:?}", tok.kind);
        let lex = tok.get_lexeme(lexeme_buf);

        varn_term::terminal::log(format!(
            "  {} │ {} │ {} │ {:?}",
            chalk(format!("{:<5}", i)).dim(),
            chalk(format!("{:<10}", loc)).yellow(),
            chalk(format!("{:<20}", kind)).blue(),
            lex
        ));
    }

    varn_term::terminal::log(chalk(format!("── {} tokens scanned ──", tokens.len())).dim());
}
