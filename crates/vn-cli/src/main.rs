#[cfg(not(feature = "dhat-heap"))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

mod bench;
mod bootstrap;
mod cli;
mod code_store;
mod commands;
mod cpu_freq;
mod debug_print;
mod discover;
mod dispatch;
mod doctor_impl;
mod error;
mod exec;
mod exit;
mod formatter;
mod ice;
mod peak_memory;
mod pipeline;
mod policy;
mod source_name;
mod standalone;
mod std_source;
mod tester;

use clap::Parser;
use cli::Cli;

use std::process;
use vn_core::term::terminal;
use vn_lexer as _;

#[cfg(feature = "dhat-heap")]
use mimalloc as _;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    ice::install();
    vn_vm::configure_process(vn_core::env::install_from_process_or_exit());

    match std::thread::Builder::new()
        .stack_size(vn_vm::VM_STACK_BYTES)
        .spawn(run_cli)
    {
        Ok(handle) => {
            if handle.join().is_err() {
                process::exit(exit::ExitCode::Internal.code());
            }
        }
        Err(e) => {
            eprintln!("failed to spawn the VM thread: {e}");
            std::process::exit(1);
        }
    }
}

fn run_cli() {
    match standalone::detect() {
        Ok(Some(embedded)) => process::exit(standalone::run(embedded)),
        Ok(None) => {}
        Err(reason) => {
            eprintln!("standalone executable is corrupt: {reason}");
            process::exit(1);
        }
    }

    let cli = Cli::parse_from(cli::with_implicit_run(std::env::args().collect()));

    let project = match dispatch::project_for(&cli.command) {
        Ok(project) => project,
        Err(e) => {
            terminal::error(&e);
            process::exit(e.exit_code().code());
        }
    };
    bootstrap::init(&project);
    let session = std::sync::Arc::new(pipeline::Session::of(project));

    if let Err(e) = bootstrap::require_std(&cli.command) {
        terminal::error(&e);
        process::exit(e.exit_code().code());
    }

    let result = dispatch::run(cli.command, &session);
    vn_vm::alloc_profile::report();

    if let Err(e) = result {
        match e.exit_code() {
            exit::ExitCode::Diagnostics | exit::ExitCode::Uncaught => eprintln!("{e}"),
            exit::ExitCode::Fatal | exit::ExitCode::Usage | exit::ExitCode::Internal => {
                terminal::error(&e)
            }
        }
        process::exit(e.exit_code().code());
    }
}
