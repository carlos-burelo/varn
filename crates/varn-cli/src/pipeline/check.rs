use varn_checker::Checker;
use varn_core::ast::Program;

use crate::error::CliError;
use varn_debug::colors::{BOLD, C_ERRORS, R};
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, CliError>;

pub struct CheckResult {
    pub checker_result: varn_checker::CheckResult,
}

pub fn check(program: &Program, source: &str, debug: &DebugFlags) -> PipelineResult<CheckResult> {
    let check_result = Checker::check(program);
    if !check_result.diagnostics.is_empty() {
        let mut msgs = Vec::new();
        let error_count = check_result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .count();

        for d in &check_result.diagnostics {
            msgs.push(crate::fmt::format_diagnostic(d, source));
        }

        if error_count > 0 {
            let footer = format!(
                "\n{}{}error{}: could not compile `{}` due to {} previous error{}",
                BOLD,
                C_ERRORS,
                R,
                program.filename,
                error_count,
                if error_count > 1 { "s" } else { "" }
            );
            return Err(CliError::new(3, format!("{}\n{}", msgs.join("\n"), footer)));
        } else {
            for m in msgs {
                eprintln!("{m}");
            }
        }
    }

    if debug.symbols {
        varn_debug::symbols::debug_symbols(&check_result, &program.filename, debug);
    }

    if debug.types {
        varn_debug::types::debug_types(program, debug);
    }
    if debug.lsp {
        varn_debug::lsp::debug_lsp(program.filename.as_ref(), source, debug);
    }

    Ok(CheckResult {
        checker_result: check_result,
    })
}
