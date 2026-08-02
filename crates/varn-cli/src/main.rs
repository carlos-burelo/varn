mod bench;
mod cli;
mod commands;
mod cpu_freq;
mod doctor_impl;
mod error;
mod pipeline;

use clap::Parser;
use cli::{Cli, Commands};
use std::process;
use varn_utilities::terminal;

/// Varn is allocation-bound in the shapes that matter (object construction,
/// string concatenation, array growth), and the Windows MSVC system heap
/// charges far more per small allocation than a modern thread-caching
/// allocator does. Swapping it out is the one change that reaches every
/// allocation site at once.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    const STDLIB_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/std.vnb"));
    varn_builtins::register_embedded_stdlib(STDLIB_BYTES);
    varn_builtins::register_provider();

    // Resolving the std is lazy; force it here so an unusable one fails at
    // startup with a readable message instead of mid-compile (spec §3: never
    // a silent fallback to the embedded registry).
    if let Some(reason) = varn_builtins::std_load_error() {
        let e = error::CliError::fatal(reason);
        terminal::error(&e);
        process::exit(e.exit_code);
    }

    if std::env::var("VARN_DEBUG_OPS").is_ok() {
        println!("DEBUG std resolved: {:?}", varn_modules::std_root::resolve());
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
        Commands::Pkg(sub) => commands::pkg::execute(sub),
        Commands::Doctor => commands::doctor::execute(),
        Commands::Cache(sub) => commands::cache::execute(sub),
        Commands::Lsp => commands::lsp::execute(),
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
