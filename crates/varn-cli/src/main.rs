mod bench_impl;
mod bench_output;
mod cli;
mod commands;
mod doctor_impl;
mod error;
mod module_precompile;
mod opts;
mod pipeline;

use clap::Parser;
use cli::{Cli, Commands};
use std::process;

fn main() {
    varn_builtins::register_provider();

    let raw: Vec<String> = std::env::args().collect();
    let effective = implicit_run(raw);

    let cli = Cli::parse_from(effective);

    let result = dispatch(cli.command);

    if let Err(e) = result {
        eprintln!("{e}");
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
