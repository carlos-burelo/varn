use varn_core::Token;
use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_term::terminal::Section;

pub fn debug_tokens(tokens: &[Token], lexeme_buf: &[u8], filename: &str) {
    Section::new("tokens")
        .subtitle(filename)
        .color(|c| c.magenta())
        .print();

    terminal::log(format!(
        "  {}",
        chalk(format!(
            "{:<5} │ {:<10} │ {:<20} │ Lexeme",
            "Idx", "Loc", "Kind"
        ))
        .dim()
    ));
    terminal::log(format!("  {}", "─".repeat(70)));

    for (i, tok) in tokens.iter().enumerate() {
        let loc = format!("{}:{}", tok.range.start.line + 1, tok.range.start.column);
        let lex = tok.get_lexeme(lexeme_buf);
        terminal::log(format!(
            "  {} │ {:<10} │ {} │ {}",
            chalk(format!("{:<5}", i)).dim(),
            loc,
            chalk(format!("{:<20}", format!("{:?}", tok.kind))).magenta(),
            chalk(format!("{:?}", lex)).yellow()
        ));
    }

    Section::new("tokens")
        .subtitle(format!("{} tokens scanned", tokens.len()))
        .close();
}
