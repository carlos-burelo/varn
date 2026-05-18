use varn_core::Token;
use varn_debug::flags::DebugFlags;

pub fn lex(source: &str, path: &str, verbose: bool, debug: &DebugFlags) -> Vec<Token> {
    let (tokens, errors) = varn_lexer::scan(source, path);

    for e in &errors {
        eprintln!(
            "{}",
            crate::fmt::format_error_with_context(
                source,
                path,
                e.range.start.line,
                e.range.start.column,
                "lex",
                &e.message
            )
        );
    }

    if verbose {
        eprintln!("[Varn] scanned {} tokens", tokens.len());
    }

    if debug.tokens {
        varn_debug::tokens::debug_tokens(&tokens, path);
    }

    tokens
}
