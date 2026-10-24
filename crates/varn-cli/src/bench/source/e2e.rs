use super::*;

pub(super) fn measure_e2e(
    runs: usize,
    eval: Option<&str>,
    path: &str,
    debug_flags: &varn_debug::flags::DebugFlags,
    factory: &VmFactory,
    session: &varn_pipeline::ModuleResolver,
) -> Result<Vec<Duration>, CliError> {
    time_n(runs, || {
        let source = match eval {
            Some(code) => code.to_owned(),
            None => crate::pipeline::Source::file(path)
                .map_err(|e| e.to_string())?
                .text()
                .to_owned(),
        };

        let (tokens, lexeme_buf) = crate::pipeline::phase_lex(
            &source,
            path,
            false,
            &debug_flags,
            &varn_pipeline::NullSink,
        )
        .map_err(|e| e.to_string())?;

        let (program, _, interner, arena) =
            parse_shared(tokens, lexeme_buf, path).map_err(|errs| {
                let msgs: Vec<String> = errs
                    .iter()
                    .map(|e| {
                        format!(
                            "{}:{}:{}: {}",
                            path, e.range.start.line, e.range.start.column, e.to_string()
                        )
                    })
                    .collect();
                format!("parse errors:\n{}", msgs.join("\n"))
            })?;

        let check_result = Checker::check_with(
            &program,
            &arena,
            interner,
            session,
            varn_sem::output::CheckOptions::compile(),
        );

        let mut e2e_tir = varn_pipeline::emit_tir(&program, &arena, &check_result);
        varn_pipeline::inject_star_reexports(&mut e2e_tir, &exports_of(&program.filename, session));
        varn_pipeline::resolve_import_slots(&mut e2e_tir, session, &program.filename);
        let (e2e_result, _) = varn_pipeline::compile_tir(
            &e2e_tir,
            export_names_of(&program.filename, session),
            &source,
            false,
        );
        let proto = e2e_result.map_err(|e| format!("compile failed: {}", e))?;

        varn_builtins::reset_testing_counters();

        let mut machine = factory.build();
        let closure = Rc::new(proto);
        run_vm_to_completion(&mut machine, closure)
    })
}
