use crate::PipelineError;
use std::rc::Rc;
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn parse(
    tokens: Vec<varn_core::Token>,
    lexeme_buf: Rc<[u8]>,
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<varn_core::ast::Program> {
    let program = varn_parser::parse(tokens, lexeme_buf, path).map_err(|errs| {
        let msgs: Vec<String> = errs
            .iter()
            .map(|e| varn_core::diagnostics::format_diagnostic(e, source))
            .collect();
        let error_count = errs.len();
        let footer = format!(
            "\n{}: could not compile `{}` due to {} previous error{}",
            varn_term::chalk::chalk("error").red().bold(),
            path,
            error_count,
            if error_count > 1 { "s" } else { "" }
        );
        PipelineError::new(3, format!("{}\n{}", msgs.join("\n"), footer))
    })?;

    if verbose {
        varn_term::terminal::tagged(
            "Varn",
            format_args!("parsed {} top-level statements", program.body.len()),
        );
    }

    if debug.ast {
        varn_debug::ast::debug_ast(&program);
    }

    if debug.modules {
        varn_debug::modules::debug_modules(&program);
    }
    if debug.graph {}

    Ok(program)
}
