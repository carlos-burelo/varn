use vn_checker::Checker;
use vn_core::ast::{AstArena, Program};
use vn_core::{AtomInterner, DiagnosticBag};

use crate::{DiagnosticReport, Failure};

pub(crate) fn check_program(
    program: &Program,
    arena: &AstArena,
    interner: AtomInterner,
    source: &str,
    resolver: &crate::ModuleResolver,
) -> Result<vn_sem::output::CheckResult, Failure> {
    let result = Checker::check_with(
        program,
        arena,
        interner,
        resolver,
        vn_sem::output::CheckOptions::compile(),
    );
    if result.diagnostics.has_errors() {
        let mut reports = vec![DiagnosticReport::new(
            &program.filename,
            source,
            result.diagnostics.clone(),
        )];
        reports.extend(dependency_syntax_reports(&result.bind.deps, resolver));
        return Err(Failure::diagnostics_across(&program.filename, reports));
    }
    Ok(result)
}

fn dependency_syntax_reports(
    direct: &[std::sync::Arc<str>],
    resolver: &crate::ModuleResolver,
) -> Vec<DiagnosticReport> {
    use vn_sem::resolver::ImportResolver;
    let mut pending: Vec<String> = direct.iter().map(|d| d.to_string()).collect();
    let mut visited = std::collections::BTreeSet::new();
    let mut reports = Vec::new();
    while let Some(path) = pending.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        if let Some(failure) = resolver.module_syntax_failure(&path) {
            let mut bag = DiagnosticBag::new();
            for diagnostic in &failure.diagnostics {
                bag.push(diagnostic.clone());
            }
            reports.push(DiagnosticReport::new(&path, &failure.source, bag));
        } else if let Some(bind) = resolver.module_bind(&path) {
            pending.extend(bind.deps.iter().map(|d| d.to_string()));
        }
    }
    reports.sort_by(|a, b| a.file.cmp(&b.file));
    reports
}

pub(crate) fn check_tolerant(
    program: &Program,
    arena: &AstArena,
    interner: AtomInterner,
    resolver: &crate::ModuleResolver,
) -> vn_sem::output::CheckResult {
    Checker::check_with(
        program,
        arena,
        interner,
        resolver,
        vn_sem::output::CheckOptions::tooling(),
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
    Ok(Some(DiagnosticReport::new(
        filename,
        source,
        diagnostics.clone(),
    )))
}
