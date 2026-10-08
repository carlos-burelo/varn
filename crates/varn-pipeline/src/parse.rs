use crate::PipelineError;
use std::sync::Arc;
use varn_debug_flags::DebugFlags;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn parse(
    tokens: Vec<varn_core::Token>,
    lexeme_buf: Arc<[u8]>,
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
    sink: &dyn crate::debug_sink::DebugSink,
) -> PipelineResult<(
    varn_core::ast::Program,
    varn_core::ast::AstArena,
    varn_core::AtomInterner,
)> {
    let (program, arena, interner) =
        varn_parser::parse(tokens, lexeme_buf, path, varn_core::AtomInterner::new())
            .map(|(program, interner, arena)| (program, arena, interner))
            .map_err(|errs| {
                let msgs: Vec<String> = errs
                    .iter()
                    .map(|e| varn_core::diagnostics::format_diagnostic(e, source))
                    .collect();
                let error_count = errs.len();
                let footer = format!(
                    "\n{}: could not compile `{}` due to {} previous error{}",
                    varn_core::term::chalk::chalk("error").red().bold(),
                    path,
                    error_count,
                    if error_count > 1 { "s" } else { "" }
                );
                PipelineError::new(3, format!("{}\n{}", msgs.join("\n"), footer))
            })?;

    if verbose {
        varn_core::term::terminal::tagged(
            "Varn",
            format_args!("parsed {} top-level statements", program.body.len()),
        );
    }

    sink.parse(&program, &arena, &interner, debug);

    Ok((program, arena, interner))
}
