use varn_checker::{CheckResult, Checker};
use varn_core::ast::Program;

pub fn check_semantics(program: &Program) -> CheckResult {
    Checker::check_for_lsp(program)
}
