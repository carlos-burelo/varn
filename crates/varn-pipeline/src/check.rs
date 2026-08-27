use varn_checker::Checker;
use varn_core::ast::Program;

use crate::PipelineError;
use varn_debug::flags::DebugFlags;
use varn_core::term::chalk::chalk;

type PipelineResult<T> = Result<T, PipelineError>;

pub struct CheckResult {
    pub checker_result: varn_checker::CheckResult,
}

/// What a checker run means for the build: errors stop it, warnings are
/// printed, silence passes.
///
/// Shared by the entry file and by every module reached through `import`
/// ([`crate::module_precompile::build_module_graph`]). It has to be one
/// function: the module graph used to call the checker for its type
/// annotations and drop `diagnostics` on the floor, so `let x: int = "s"` was
/// a hard error in the file you ran and silently fine in the file it imported.
/// A rule about validity that two call sites apply differently is not a rule.
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
    source: &str,
    debug: &DebugFlags,
    strict: bool,
) -> PipelineResult<CheckResult> {
    let mut options = varn_checker::CheckOptions::compile();
    if strict {
        options = options.strict();
    }
    let check_result = crate::resolver::with_resolver(|r| {
        Checker::check_with(program, r, options)
    });
    report_diagnostics(&check_result.diagnostics, &program.filename, source)?;

    if debug.symbols {
        varn_debug::symbols::debug_symbols(&check_result, &program.filename, debug);
    }

    // After the check, not after the parse: this dump is the checker's answers,
    // and the old `debug_expr` hook fired in `parse` where none of them exist
    // yet — which is why it could only ever print "not implemented".
    if debug.check_types {
        varn_debug::expr::debug_check_types(program, source, &check_result);
    }

    Ok(CheckResult {
        checker_result: check_result,
    })
}
