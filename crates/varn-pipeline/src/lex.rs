use crate::Failure;
use std::sync::Arc;
use varn_core::debug_flags::DebugFlags;
use varn_core::Token;

type PipelineResult<T> = Result<T, Failure>;

pub fn lex(
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
    sink: &dyn crate::debug_sink::DebugSink,
) -> PipelineResult<(Vec<Token>, Arc<[u8]>)> {
    let (tokens, lexeme_buf, errors) = varn_lexer::scan(source, path);

    if !errors.is_empty() {
        let mut bag = varn_core::DiagnosticBag::new();
        for e in errors {
            bag.push(e);
        }
        return Err(Failure::diagnostics(path, source, bag));
    }

    if verbose {
        varn_core::term::terminal::tagged("Varn", format_args!("scanned {} tokens", tokens.len()));
    }

    sink.lex(&tokens, &lexeme_buf, path, debug);

    Ok((tokens, lexeme_buf))
}
