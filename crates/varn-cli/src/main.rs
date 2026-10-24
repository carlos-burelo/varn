#[cfg(not(feature = "dhat-heap"))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

mod bench;
mod cli;
mod code_store;
mod commands;
mod cpu_freq;
mod debug_print;
mod doctor_impl;
mod error;
mod exec;
mod exit;
mod formatter;
#[cfg(feature = "lsp")]
mod inspect_lsp;
mod pipeline;
mod source_name;
mod std_source;
mod tester;

use clap::Parser;
use cli::{Cli, Commands};
use exit::ExitCode;
use pipeline::{Anchor, Session};

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
    varn_project::apply_from_cwd();

    std_source::register();
    code_store::register();
    varn_builtins::register_provider();

    try_run_standalone_executable();

    if std::env::var("VARN_DEBUG_OPS").is_ok() {
        println!(
            "DEBUG std resolved: {:?}",
            varn_modules::std_root::resolve()
        );
        println!("--- CLI NATIVE OPERATIONS ---");
        for entry in varn_builtins::dispatch::all_native_ops() {
            println!(
                "id: {} | module: {} | ns: {} | symbol: {} | capability: {} | func_ptr: {:?}",
                varn_builtins::native_op_id(
                    entry.module_id(),
                    entry.namespace_path(),
                    entry.symbol_name()
                )
                .map_or("-".to_owned(), |id| id.to_string()),
                entry.module_id(),
                entry.namespace_path(),
                entry.symbol_name(),
                entry.capability().unwrap_or("-"),
                entry.func_ptr
            );
        }
        println!("-------------------------");
    }

    let raw: Vec<String> = std::env::args().collect();
    let effective = implicit_run(raw);

    let cli = Cli::parse_from(effective);

    if !matches!(cli.command, Commands::Lsp(_) | Commands::Doctor)
        && let Some(reason) = varn_builtins::std_load_error()
    {
        let e = error::CliError::fatal(reason);
        terminal::error(&e);
        process::exit(e.exit_code().code());
    }

    varn_vm::alloc_profile::init();
    varn_vm::gc_trace::init();
    let result = dispatch(cli.command);
    varn_vm::alloc_profile::report();

    if let Err(e) = result {
        match e.exit_code() {
            ExitCode::Diagnostics => eprintln!("{e}"),
            ExitCode::Fatal | ExitCode::Usage => terminal::error(&e),
        }
        process::exit(e.exit_code().code());
    }
}

fn anchor<'a>(file: Option<&'a str>, eval: Option<&str>) -> Anchor<'a> {
    match (file, eval) {
        (_, Some(_)) | (None, None) => Anchor::Cwd,
        (Some(file), None) => Anchor::File(std::path::Path::new(file)),
    }
}

fn open_session(anchor: Anchor<'_>) -> Result<Session, error::CliError> {
    Session::open(anchor).map_err(error::CliError::from)
}

fn dispatch(cmd: Commands) -> Result<(), error::CliError> {
    match cmd {
        Commands::Run(args) => {
            let session = open_session(anchor(args.file.as_deref(), args.eval.as_deref()))?;
            commands::run::execute(&session, args)
        }
        Commands::Check(args) => {
            let session = open_session(Anchor::File(std::path::Path::new(&args.file)))?;
            commands::check::execute(&session, args)
        }
        Commands::Eval(args) => {
            let session = open_session(Anchor::Cwd)?;
            commands::eval::execute(&session, args)
        }
        Commands::Repl(args) => {
            let session = open_session(Anchor::Cwd)?;
            commands::repl::execute(&session, args)
        }
        Commands::Bench(args) => {
            let session = open_session(anchor(args.file.as_deref(), args.eval.as_deref()))?;
            commands::bench::execute(&session, args)
        }
        Commands::Debug(args) => {
            let session = open_session(anchor(args.file.as_deref(), args.eval.as_deref()))?;
            commands::debug::execute(&session, args)
        }
        Commands::Build(args) => {
            let session = open_session(Anchor::File(std::path::Path::new(&args.file)))?;
            commands::build::execute(&session, args)
        }
        Commands::Test(args) => {
            let session = std::sync::Arc::new(open_session(Anchor::Cwd)?);
            commands::test::execute(session, args)
        }
        Commands::Fmt(args) => commands::fmt::execute(args),
        Commands::Doctor => commands::doctor::execute(),
        Commands::Cache(sub) => commands::cache::execute(sub),
        #[cfg(feature = "lsp")]
        Commands::Lsp(args) => commands::lsp::execute(args),
        #[cfg(not(feature = "lsp"))]
        Commands::Lsp(_) => Err(error::CliError::fatal(
            "vn was built without the \"lsp\" feature (disabled during the Fase 1 compiler refactor)"
                .to_string(),
        )),
        Commands::Init(args) => commands::init::execute(args),
        Commands::Completions(args) => commands::completions::execute(args),
        #[cfg(feature = "dap")]
        Commands::Dap => commands::dap::execute(),
        #[cfg(not(feature = "dap"))]
        Commands::Dap => Err(error::CliError::fatal(
            "vn was built without the \"dap\" feature".to_string(),
        )),
        #[cfg(feature = "pm")]
        Commands::Pkg(sub) => commands::pkg::execute(sub),
        #[cfg(not(feature = "pm"))]
        Commands::Pkg(_) => Err(error::CliError::fatal(
            "vn was built without the \"pm\" feature".to_string(),
        )),
        Commands::StdBundle(args) => {
            let session = open_session(Anchor::Cwd)?;
            commands::std_bundle::execute(&session, args)
        }
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
        "fmt",
        "pkg",
        "init",
        "doctor",
        "cache",
        "lsp",
        "completions",
        "dap",
        "std-bundle",
        "help",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    if let Some(first) = args.get(1)
        && !SUBCOMMANDS.contains(&first.as_str())
        && !first.starts_with('-')
    {
        args.insert(1, "run".to_owned());
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

    let Ok(inner_payload) = varn_modules::artifact::read_artifact(
        varn_modules::artifact::ArtifactKind::ModuleGraph,
        &envelope,
    ) else {
        return;
    };

    let Ok(artifact) = postcard::from_bytes::<varn_types::ModuleGraphArtifact>(inner_payload)
    else {
        return;
    };

    let Ok(executable) = pipeline::Executable::from_graph(artifact) else {
        return;
    };

    if let Err(e) = pipeline::execute(
        &Session::detached(),
        executable,
        &Default::default(),
        pipeline::CapabilitySet::allow_all(),
        false,
    ) {
        eprintln!("Standalone runtime error: {e}");
        process::exit(1);
    }
    process::exit(0);
}
