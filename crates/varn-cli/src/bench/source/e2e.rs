use super::*;

pub(super) fn measure_e2e(
    session: &varn_pipeline::Session,
    source: &varn_pipeline::Source,
    runs: usize,
    factory: &VmFactory,
) -> Result<Vec<Duration>, CliError> {
    use varn_pipeline::{CachePolicy, Request, Stage};

    let once = || -> Result<Duration, String> {
        let cold = session.isolated();
        let start = Instant::now();
        let compilation = cold.compile(source.clone(), Request::executable(CachePolicy::Bypass));
        let link: Duration = compilation
            .timings
            .iter()
            .filter(|(stage, _)| *stage == Stage::Link)
            .map(|(_, took)| *took)
            .sum();
        let executable = compilation.into_executable().map_err(|e| e.to_string())?;

        varn_builtins::reset_testing_counters();

        let mut machine = factory.build();
        run_vm_to_completion(&mut machine, Rc::new(executable.entry_proto))?;
        Ok(start.elapsed().saturating_sub(link))
    };

    once().map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;
    (0..runs)
        .map(|_| once().map_err(|e| CliError::fatal(format!("bench run failed: {e}"))))
        .collect()
}
