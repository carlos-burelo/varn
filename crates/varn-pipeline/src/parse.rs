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
            .map(|e| {
                crate::fmt::format_error_with_context(
                    source,
                    path,
                    e.range.start.line,
                    e.range.start.column,
                    "parse",
                    &e.message,
                )
            })
            .collect();
        PipelineError::fatal(msgs.join("\n"))
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
    if debug.expr {
        varn_debug::expr::debug_expr(&program, debug.expr_range);
    }
    if debug.graph {}

    Ok(program)
}
