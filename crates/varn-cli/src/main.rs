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
mod dispatch;
mod doctor_impl;
mod error;
mod exec;
mod exit;
mod formatter;
mod inspect_lsp;
mod pipeline;
mod policy;
mod source_name;
mod standalone;
mod std_source;
mod tester;

use clap::Parser;
use cli::Cli;

use std::process;
use varn_core::term::terminal;
use varn_lexer as _;

#[cfg(feature = "dhat-heap")]
use mimalloc as _;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    match std::thread::Builder::new()
        .stack_size(varn_vm::VM_STACK_BYTES)
        .spawn(run_cli)
    {
        Ok(handle) => {
            if let Err(panic) = handle.join() {
                std::panic::resume_unwind(panic);
            }
        }
        Err(e) => {
            eprintln!("failed to spawn the VM thread: {e}");
            std::process::exit(1);
        }
    }
}

fn run_cli() {
    bootstrap::init();

    if let Some(code) = standalone::try_run() {
        process::exit(code);
    }

    let cli = Cli::parse_from(cli::with_implicit_run(std::env::args().collect()));

    if let Err(e) = bootstrap::require_std(&cli.command) {
        terminal::error(&e);
        process::exit(e.exit_code().code());
    }

    varn_vm::alloc_profile::init();
    varn_vm::gc_trace::init();
    let result = dispatch::dispatch(cli.command);
    varn_vm::alloc_profile::report();

    if let Err(e) = result {
        match e.exit_code() {
            exit::ExitCode::Diagnostics => eprintln!("{e}"),
            exit::ExitCode::Fatal | exit::ExitCode::Usage => terminal::error(&e),
        }
        process::exit(e.exit_code().code());
    }
}
