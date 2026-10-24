use crate::Failure;
use std::sync::Arc;
use varn_core::debug_flags::DebugFlags;

type PipelineResult<T> = Result<T, Failure>;

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
                let mut bag = varn_core::DiagnosticBag::new();
                for e in errs {
                    bag.push(e);
                }
                Failure::diagnostics(path, source, bag)
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
