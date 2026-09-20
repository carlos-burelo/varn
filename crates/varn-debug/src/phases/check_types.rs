//! `-p check:types`: deterministic, diffable checker type dump
//! (DEBUG_PLAN §4.5). `Plain` and `Text` are the same bytes (this phase was
//! always colorless and line-oriented).

use std::io::Write;

use varn_checker::CheckResult;
use varn_core::ast::Program;

use crate::fmt::Format;
use crate::report::Report;

pub fn collect(program: &Program, source: &str, check: &CheckResult) -> Report {
    Report::Text(crate::expr::render_check_types(program, source, check))
}

pub fn render(rep: &Report, fmt: Format, w: &mut dyn Write) -> std::io::Result<()> {
    let Report::Text(text) = rep else {
        return Ok(());
    };
    let _ = fmt;
    write!(w, "{text}")
}

pub fn debug_check_types(program: &Program, source: &str, check: &CheckResult) {
    let rep = collect(program, source, check);
    let _ = render(&rep, Format::Plain, &mut std::io::stderr());
}
