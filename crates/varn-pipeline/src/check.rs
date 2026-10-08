use varn_checker::Checker;
use varn_core::ast::{AstArena, Program};

use crate::PipelineError;
use varn_core::term::chalk::chalk;
use varn_debug_flags::DebugFlags;

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
    session: &crate::resolver::Session,
    sink: &dyn crate::debug_sink::DebugSink,
) -> PipelineResult<CheckResult> {
    let options = varn_checker::CheckOptions::compile();
    let check_result =
        Checker::check_with(program, ast_arena, interner, session.resolver(), options);
    report_diagnostics(&check_result.diagnostics, &program.filename, source)?;

    sink.check(program, source, &check_result, debug);

    Ok(CheckResult {
        checker_result: check_result,
    })
}

pub fn collect_test_targets(
    path: &str,
    source: &str,
    session: &crate::resolver::Session,
) -> PipelineResult<Vec<(String, bool)>> {
    let debug = DebugFlags::default();
    let sink = crate::debug_sink::NullSink;
    let (tokens, lexeme_buf) = crate::lex::lex(source, path, false, &debug, &sink)?;
    let (program, arena, interner) =
        crate::parse::parse(tokens, lexeme_buf, source, path, false, &debug, &sink)?;
    let options = varn_checker::CheckOptions::compile();
    let checked = Checker::check_with(&program, &arena, interner, session.resolver(), options);
    if checked.diagnostics.has_errors() {
        report_diagnostics(&checked.diagnostics, &program.filename, source)?;
    }
    Ok(checked
        .test_targets
        .iter()
        .map(|t| (t.name.to_string(), t.is_async))
        .collect())
}
