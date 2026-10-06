use varn_checker::Checker;
use varn_core::ast::{AstArena, Program};

use crate::PipelineError;
use varn_core::term::chalk::chalk;
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, PipelineError>;

pub struct CheckResult {
    pub checker_result: varn_checker::CheckResult,
}










pub fn report_diagnostics(
    diagnostics: &varn_core::DiagnosticBag,
    filename: &str,
    source: &str,
) -> PipelineResult<()> {
    if diagnostics.is_empty() {
        return Ok(());
    }
    let error_count = diagnostics.iter().filter(|d| d.is_error()).count();
    let msgs: Vec<String> = diagnostics
        .iter()
        .map(|d| crate::fmt::format_diagnostic(d, source))
        .collect();

    if error_count == 0 {
        for m in msgs {
            varn_core::term::terminal::log(m);
        }
        return Ok(());
    }

    let footer = format!(
        "\n{}: could not compile `{}` due to {} previous error{}",
        chalk("error").red().bold(),
        filename,
        error_count,
        if error_count > 1 { "s" } else { "" }
    );
    Err(PipelineError::new(
        3,
        format!("{}\n{}", msgs.join("\n"), footer),
    ))
}

pub fn check(
    program: &Program,
    ast_arena: &AstArena,
    interner: varn_core::AtomInterner,
    source: &str,
    debug: &DebugFlags,
) -> PipelineResult<CheckResult> {
    let options = varn_checker::CheckOptions::compile();
    let check_result = crate::resolver::with_resolver(|r| {
        Checker::check_with(program, ast_arena, interner, r, options)
    });
    report_diagnostics(&check_result.diagnostics, &program.filename, source)?;

    if debug.symbols {
        varn_debug::symbols::debug_symbols(&check_result, &program.filename, debug);
    }

    
    
    
    if debug.check_types {
        varn_debug::expr::debug_check_types(program, source, &check_result);
    }

    Ok(CheckResult {
        checker_result: check_result,
    })
}
