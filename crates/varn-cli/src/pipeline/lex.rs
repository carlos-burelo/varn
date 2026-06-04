use std::rc::Rc;
use varn_core::Token;
use varn_debug::flags::DebugFlags;
use varn_utilities::terminal;

pub fn lex(source: &str, path: &str, verbose: bool, debug: &DebugFlags) -> (Vec<Token>, Rc<[u8]>) {
    let (tokens, lexeme_buf, errors) = varn_lexer::scan(source, path);

    for e in &errors {
        terminal::log(crate::fmt::format_error_with_context(
            source,
            path,
            e.range.start.line,
            e.range.start.column,
            "lex",
            &e.message,
        ));
    }

    if verbose {
        terminal::tagged("Varn", format!("scanned {} tokens", tokens.len()));
    }

    if debug.tokens {
        varn_debug::tokens::debug_tokens(&tokens, &lexeme_buf, path);
    }

    (tokens, lexeme_buf)
}
