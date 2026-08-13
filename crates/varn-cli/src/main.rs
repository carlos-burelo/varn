mod bench;
mod cli;
mod commands;
mod cpu_freq;
mod doctor_impl;
mod error;
mod inspect_lsp;
mod pipeline;
mod tester;

use clap::Parser;
use cli::{Cli, Commands};
// `varn-lexer` belongs to the crate for the `debug_binder` bin, not to `vn`.
// The anchor keeps `unused_crate_dependencies` honest for this target.
use varn_lexer as _;
use std::process;
use varn_term::terminal;

fn main() {
    const STDLIB_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/std.vnb"));
    varn_builtins::register_embedded_stdlib(STDLIB_BYTES);
    varn_builtins::register_provider();

    try_run_standalone_executable();

    if std::env::var("VARN_DEBUG_OPS").is_ok() {
        println!(
            "DEBUG std resolved: {:?}",
            varn_modules::std_root::resolve()
        );
        println!("--- CLI NATIVE OPERATIONS ---");
        for entry in varn_builtins::dispatch::iter_native_ops() {
            println!(
                "module: {} | ns: {} | symbol: {} | func_ptr: {:?}",
                entry.module_id(),
                entry.namespace_path(),
                entry.symbol_name(),
                entry.func_ptr
            );
        }
        println!("-------------------------");
    }

    let raw: Vec<String> = std::env::args().collect();
    let effective = implicit_run(raw);

    let cli = Cli::parse_from(effective);

    // Resolving the std is lazy; force it here so an unusable one fails at
    // startup with a readable message instead of mid-compile (spec §3: never
    // a silent fallback to the embedded registry).
    //
    // Two exceptions, both decided after parsing. `lsp`: an editor opened on
    // a half-recompiled checkout is routine, and exiting there gives the
    // client an EPIPE instead of a diagnostic — the server reports the same
    // reason through `window/showMessage` and keeps serving. `doctor`: its
    // whole job is explaining a broken install, so it must survive one.
    if !matches!(cli.command, Commands::Lsp(_) | Commands::Doctor) {
        if let Some(reason) = varn_builtins::std_load_error() {
            let e = error::CliError::fatal(reason);
            terminal::error(&e);
            process::exit(e.exit_code);
        }
    }

    let result = dispatch(cli.command);

    if let Err(e) = result {
        terminal::error(&e);
        process::exit(e.exit_code);
    }
}

fn dispatch(cmd: Commands) -> Result<(), error::CliError> {
    match cmd {
        Commands::Run(args) => commands::run::execute(args),
        Commands::Check(args) => commands::check::execute(args),
        Commands::Eval(args) => commands::eval::execute(args),
        Commands::Repl(args) => commands::repl::execute(args),
        Commands::Bench(args) => commands::bench::execute(args),
        Commands::Debug(args) => commands::debug::execute(args),
        Commands::Build(args) => commands::build::execute(args),
        Commands::Test(args) => commands::test::execute(args),
        Commands::Pkg(sub) => commands::pkg::execute(sub),
        Commands::Doctor => commands::doctor::execute(),
        Commands::Cache(sub) => commands::cache::execute(sub),
        Commands::Lsp(args) => commands::lsp::execute(args),
        Commands::Init(args) => commands::init::execute(args),
        Commands::Completions(args) => commands::completions::execute(args),
    }
}

fn implicit_run(mut args: Vec<String>) -> Vec<String> {
    const SUBCOMMANDS: &[&str] = &[
        "run",
        "check",
        "eval",
        "repl",
        "bench",
        "debug",
        "build",
        "test",
        "pkg",
        "init",
        "doctor",
        "cache",
        "lsp",
        "completions",
        "help",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    if let Some(first) = args.get(1) {
        if !SUBCOMMANDS.contains(&first.as_str()) && !first.starts_with('-') {
            args.insert(1, "run".to_owned());
        }
    }
    args
}

fn try_run_standalone_executable() {
    let Ok(exe_path) = std::env::current_exe() else {
        return;
    };
    let Ok(file) = std::fs::File::open(&exe_path) else {
        return;
    };
    let Ok(meta) = file.metadata() else {
        return;
    };
    let len = meta.len();
    if len < 12 {
        return;
    }
    use std::io::{Read, Seek, SeekFrom};
    let mut reader = std::io::BufReader::new(file);

    let mut trailer = [0u8; 12];
    if reader.seek(SeekFrom::End(-12)).is_err() {
        return;
    }
    if reader.read_exact(&mut trailer).is_err() {
        return;
    }

    if &trailer[8..12] != varn_modules::artifact::MAGIC_VEXE {
        return;
    }

    let payload_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap());
    if len < 12 + payload_len {
        return;
    }

    let payload_offset = len - 12 - payload_len;
    if reader.seek(SeekFrom::Start(payload_offset)).is_err() {
        return;
    }
    let mut envelope = vec![0u8; payload_len as usize];
    if reader.read_exact(&mut envelope).is_err() {
        return;
    }

    let Ok(inner_payload) = varn_modules::artifact::read_envelope(
        varn_modules::artifact::MAGIC_WRC,
        pipeline::CACHE_FORMAT_VERSION,
        &envelope,
    ) else {
        return;
    };

    let Ok(artifact) = postcard::from_bytes::<varn_types::ModuleGraphArtifact>(inner_payload)
    else {
        return;
    };

    let Ok(compiled) = pipeline::cache::compile_output_from_graph(artifact) else {
        return;
    };

    let exe_str = exe_path.to_string_lossy();
    if let Err(e) = pipeline::execute(
        compiled.entry_proto,
        compiled.precompiled,
        "",
        &exe_str,
        &Default::default(),
    ) {
        eprintln!("Standalone runtime error: {e}");
        process::exit(1);
    }
    process::exit(0);
}
