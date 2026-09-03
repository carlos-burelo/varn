use crate::PipelineError;
use std::rc::Rc;
use varn_core::Token;
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn lex(
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<(Vec<Token>, Rc<[u8]>)> {
    let (tokens, lexeme_buf, errors) = varn_lexer::scan(source, path);

    if !errors.is_empty() {
        let msgs: Vec<String> = errors
            .iter()
            .map(|e| varn_core::diagnostics::format_diagnostic(e, source))
            .collect();
        let error_count = errors.len();
        let footer = format!(
            "\n{}: could not compile `{}` due to {} previous error{}",
            varn_core::term::chalk::chalk("error").red().bold(),
            path,
            error_count,
            if error_count > 1 { "s" } else { "" }
        );
        return Err(PipelineError::new(
            3,
            format!("{}\n{}", msgs.join("\n"), footer),
        ));
    }

    if verbose {
        varn_core::term::terminal::tagged("Varn", format_args!("scanned {} tokens", tokens.len()));
    }

    if debug.tokens {
        varn_debug::tokens::debug_tokens(&tokens, &lexeme_buf, path);
    }

    Ok((tokens, lexeme_buf))
}
