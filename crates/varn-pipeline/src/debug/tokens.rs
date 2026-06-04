use varn_core::Token;
use varn_utilities::chalk::chalk;
use varn_utilities::terminal::Section;

pub fn debug_tokens(tokens: &[Token], lexeme_buf: &[u8]) {
    Section::new("tokens").subtitle("").color(|c| c.magenta()).print();

    varn_utilities::terminal::log(format!(
        "  {}",
        chalk(format!("{:<5} │ {:<10} │ {:<20} │ Lexeme", "Idx", "Loc", "Kind")).bold()
    ));
    varn_utilities::terminal::log(format!("  {}", "─".repeat(70)));

    for (i, tok) in tokens.iter().enumerate() {
        let loc = format!("{}:{}", tok.range.start.line, tok.range.start.column);
        let kind = format!("{:?}", tok.kind);
        let lex = tok.get_lexeme(lexeme_buf);

        varn_utilities::terminal::log(format!(
            "  {} │ {} │ {} │ {:?}",
            chalk(format!("{:<5}", i)).dim(),
            chalk(format!("{:<10}", loc)).yellow(),
            chalk(format!("{:<20}", kind)).blue(),
            lex
        ));
    }

    varn_utilities::terminal::log(chalk(format!("── {} tokens scanned ──", tokens.len())).dim());
}
