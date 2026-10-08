use super::*;

pub(super) fn export_names_of(
    filename: &str,
    session: &varn_pipeline::resolver::Session,
) -> Vec<Arc<str>> {
    let exports = session.resolver().module_exports(filename, &mut vec![]);
    let mut names: Vec<Arc<str>> = exports.keys().map(|k| Arc::from(k.as_str())).collect();
    names.sort();
    names
}
pub(super) fn verbose_sections(
    factory: &VmFactory,
    exec_jit: &varn_vm::varn_jit::JitStatsSnapshot,
    records: &[varn_vm::varn_jit::CompileRecord],
    parse_profile: &varn_parser::ParseProfile,
    check_result: &varn_sem::output::CheckResult,
    phases: &[PhaseStats],
    opts: &BenchOpts,
) -> Result<(), CliError> {
    varn_builtins::reset_testing_counters();
    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);

    let mut profile_vm = factory.build();
    profile_vm.enable_opcode_profiling();
    profile_vm.enable_profiling();
    profile_vm.enable_hotspot_profiling();
    run_vm_to_completion(&mut profile_vm, factory.entry_proto())
        .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
    profile_vm.collect_gc();
    let opcode_counts = profile_vm.take_opcode_counts();
    let mut vm_profile = profile_vm.take_profile();
    let hotspots = profile_vm.take_hotspots();

    varn_builtins::set_print_silent(!opts.show_output);
    varn_builtins::set_testing_silent(!opts.show_output);

    let breakdown = BreakdownOpts {
        all_rows: opts.all_rows,
    };
    let phase_p50 = |name: &str| phases.iter().find(|p| p.name == name).map(|p| p.p50);

    print_breakdown(
        "Parser Breakdown",
        |c| c.green(),
        &[
            ("program_loop", parse_profile.program_loop),
            ("stmt_or_decl", parse_profile.stmt_or_decl),
            ("block", parse_profile.block),
            ("recover", parse_profile.recover),
        ],
        phase_p50("parse"),
        &breakdown,
    );

    let cp = &check_result.profile;
    print_breakdown(
        "Checker Breakdown",
        |c| c.red(),
        &[
            ("load_globals", cp.load_globals),
            ("bind", cp.bind),
            ("merge_core", cp.merge_core_members),
            ("enrich_calls", cp.enrich_call_returns),
            ("init", cp.init),
            ("check_stmts", cp.check_stmts),
            ("annotations", cp.collect_annotations),
            ("finalize", cp.finalize),
            ("cleanup", cp.cleanup),
        ],
        phase_p50("check"),
        &breakdown,
    );

    print_coverage(exec_jit, records, "programa completo");

    let interp_share = (exec_jit.total_frames() > 0).then(|| exec_jit.never_compiled_ratio());
    print_opcode_hotspots(&opcode_counts, interp_share);
    if let Some(ref mut profile) = vm_profile {
        let move_count = opcode_counts
            .iter()
            .find(|(op, _)| matches!(op, varn_core::OpCode::Move))
            .map(|(_, n)| *n)
            .unwrap_or(0);
        profile.move_opcodes = move_count;
        print_vm_profile(profile, interp_share);
    }
    if let Some(ref hs) = hotspots {
        print_hotspots(hs);
    }
    terminal::blank();
    Ok(())
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
