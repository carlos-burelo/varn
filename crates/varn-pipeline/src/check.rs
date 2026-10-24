use varn_checker::Checker;
use varn_core::ast::{AstArena, Program};
use varn_core::{AtomInterner, DiagnosticBag};

use crate::{DiagnosticReport, Failure};

pub(crate) fn check_program(
    program: &Program,
    arena: &AstArena,
    interner: AtomInterner,
    source: &str,
    resolver: &crate::ModuleResolver,
) -> Result<varn_sem::output::CheckResult, Failure> {
    let result = Checker::check_with(
        program,
        arena,
        interner,
        resolver,
        varn_sem::output::CheckOptions::compile(),
    );
    reject_errors(&result.diagnostics, &program.filename, source)?;
    Ok(result)
}

pub(crate) fn check_tolerant(
    program: &Program,
    arena: &AstArena,
    interner: AtomInterner,
    resolver: &crate::ModuleResolver,
) -> varn_sem::output::CheckResult {
    Checker::check_with(
        program,
        arena,
        interner,
        resolver,
        varn_sem::output::CheckOptions::tooling(),
    )
}

pub(crate) fn reject_errors(
    diagnostics: &DiagnosticBag,
    filename: &str,
    source: &str,
) -> Result<(), Failure> {
    if diagnostics.has_errors() {
        return Err(Failure::diagnostics(filename, source, diagnostics.clone()));
    }
    Ok(())
}

pub(crate) fn warnings_of(
    diagnostics: &DiagnosticBag,
    filename: &str,
    source: &str,
) -> Result<Option<DiagnosticReport>, Failure> {
    reject_errors(diagnostics, filename, source)?;
    if diagnostics.is_empty() {
        return Ok(None);
    }
    Ok(Some(DiagnosticReport {
        file: filename.into(),
        source: source.into(),
        diagnostics: diagnostics.clone(),
    }))
}
