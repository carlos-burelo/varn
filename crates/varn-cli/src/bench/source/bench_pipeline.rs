use super::*;

pub(super) struct StageSamples {
    pub lex: Vec<Duration>,
    pub parse: Vec<Duration>,
    pub check: Vec<Duration>,
    pub lower: Vec<Duration>,
    pub optimize: Vec<Duration>,
    pub link_cold: Duration,
}

fn stage(compilation: &varn_pipeline::Compilation, wanted: varn_pipeline::Stage) -> Duration {
    compilation
        .timings
        .iter()
        .filter(|(stage, _)| *stage == wanted)
        .map(|(_, took)| *took)
        .sum()
}

pub(super) fn sample_stages(
    session: &varn_pipeline::Session,
    source: &varn_pipeline::Source,
    runs: usize,
) -> Result<(StageSamples, varn_pipeline::Compilation), CliError> {
    use varn_pipeline::{CachePolicy, Request, Stage};

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
    compilation: &'a varn_pipeline::Compilation,
    path: &str,
) -> Result<(varn_parser::ParseProfile, &'a varn_sem::output::CheckResult), CliError> {
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
    tokens: Vec<varn_core::Token>,
    lexeme_buf: std::sync::Arc<[u8]>,
    path: &str,
) -> Result<
    (
        varn_core::ast::Program,
        varn_parser::ParseProfile,
        varn_core::AtomInterner,
        varn_core::ast::AstArena,
    ),
    varn_core::DiagnosticBag,
> {
    varn_parser::parse_with_profile(tokens, lexeme_buf, path, varn_core::AtomInterner::new())
}
