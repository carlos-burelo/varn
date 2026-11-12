use super::*;

pub(super) struct StageSamples {
    pub lex: Vec<Duration>,
    pub parse: Vec<Duration>,
    pub check: Vec<Duration>,
    pub lower: Vec<Duration>,
    pub optimize: Vec<Duration>,
    pub link_cold: Duration,
}

fn stage(compilation: &vn_pipeline::Compilation, wanted: vn_pipeline::Stage) -> Duration {
    compilation
        .timings
        .iter()
        .filter(|(stage, _)| *stage == wanted)
        .map(|(_, took)| *took)
        .sum()
}

pub(super) fn sample_stages(
    session: &vn_pipeline::Session,
    source: &vn_pipeline::Source,
    runs: usize,
) -> Result<(StageSamples, vn_pipeline::Compilation), CliError> {
    use vn_pipeline::{CachePolicy, Request, Stage};

    let compile = || {
        let compilation = session.compile(
            source.clone(),
            Request::executable(CachePolicy::Bypass).measured(),
        );
        match &compilation.failure {
            Some(failure) => Err(CliError::fatal(format!("bench compile failed: {failure}"))),
            None => Ok(compilation),
        }
    };

    let cold = compile()?;
    let link_cold = stage(&cold, Stage::Link);

    let mut samples = StageSamples {
        lex: Vec::with_capacity(runs),
        parse: Vec::with_capacity(runs),
        check: Vec::with_capacity(runs),
        lower: Vec::with_capacity(runs),
        optimize: Vec::with_capacity(runs),
        link_cold,
    };
    let mut last = cold;
    for _ in 0..runs {
        let compilation = compile()?;
        samples.lex.push(stage(&compilation, Stage::Lex));
        samples.parse.push(stage(&compilation, Stage::Parse));
        samples.check.push(stage(&compilation, Stage::Check));
        samples.lower.push(stage(&compilation, Stage::Lower));
        samples.optimize.push(stage(&compilation, Stage::Optimize));
        last = compilation;
    }
    Ok((samples, last))
}

pub(super) fn profile_inputs<'a>(
    compilation: &'a vn_pipeline::Compilation,
    path: &str,
) -> Result<(vn_parser::ParseProfile, &'a vn_sem::output::CheckResult), CliError> {
    let tokens = compilation
        .tokens
        .as_ref()
        .ok_or_else(|| CliError::fatal("the compilation kept no tokens"))?;
    let checked = compilation
        .checked
        .as_ref()
        .ok_or_else(|| CliError::fatal("the compilation kept no check result"))?;
    let (_, profile, _, _) = parse_shared(tokens.tokens.clone(), tokens.lexeme_buf.clone(), path)
        .map_err(|errs| {
        CliError::fatal(format!(
            "parse errors while profiling: {} found",
            errs.len()
        ))
    })?;
    Ok((profile, checked))
}

pub(super) fn parse_shared(
    tokens: Vec<vn_core::Token>,
    lexeme_buf: std::sync::Arc<[u8]>,
    path: &str,
) -> Result<
    (
        vn_core::ast::Program,
        vn_parser::ParseProfile,
        vn_core::AtomInterner,
        vn_core::ast::AstArena,
    ),
    vn_core::DiagnosticBag,
> {
    vn_parser::parse_with_profile(tokens, lexeme_buf, path, vn_core::AtomInterner::new())
}
