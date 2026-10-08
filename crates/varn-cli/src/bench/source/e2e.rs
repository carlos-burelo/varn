use super::*;

pub(super) fn measure_e2e(
    runs: usize,
    eval: Option<&str>,
    path: &str,
    debug_flags: &varn_debug::flags::DebugFlags,
    factory: &VmFactory,
    session: &varn_pipeline::resolver::Session,
) -> Result<Vec<Duration>, CliError> {
    time_n(runs, || {
        let source = match eval {
            Some(code) => code.to_owned(),
            None => crate::pipeline::read_source_file(path).map_err(|e| e.message.clone())?,
        };

        let (tokens, lexeme_buf) = crate::pipeline::phase_lex(
            &source,
            path,
            false,
            &debug_flags,
            &varn_pipeline::NullSink,
        )
        .map_err(|e| e.message)?;

        let (program, _, interner, arena) =
            parse_shared(tokens, lexeme_buf, path).map_err(|errs| {
                let msgs: Vec<String> = errs
                    .iter()
                    .map(|e| {
                        format!(
                            "{}:{}:{}: {}",
                            path, e.range.start.line, e.range.start.column, e.message
                        )
                    })
                    .collect();
                format!("parse errors:\n{}", msgs.join("\n"))
            })?;

        let check_result = Checker::check_with(
            &program,
            &arena,
            interner,
            session.resolver(),
            varn_checker::CheckOptions::compile(),
        );

        let proto = varn_pipeline::emit_and_compile(
            &program,
            &arena,
            &check_result,
            export_names_of(&program.filename, session),
            &source,
        )
        .map_err(|e| format!("compile failed: {}", e))?;

        varn_builtins::reset_testing_counters();

        let mut machine = factory.build();
        let closure = Rc::new(proto);
        run_vm_to_completion(&mut machine, closure)
    })
}
