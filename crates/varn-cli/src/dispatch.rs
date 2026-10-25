use crate::cli::Commands;
use crate::commands;
use crate::error::CliError;
use crate::pipeline::{Anchor, Session};
use std::sync::Arc;

fn anchor<'a>(file: Option<&'a str>, eval: Option<&str>) -> Anchor<'a> {
    match (file, eval) {
        (_, Some(_)) | (None, None) => Anchor::Cwd,
        (Some(file), None) => Anchor::File(std::path::Path::new(file)),
    }
}

fn open_session(anchor: Anchor<'_>) -> Result<Session, CliError> {
    Session::open(anchor).map_err(CliError::from)
}

pub fn dispatch(cmd: Commands) -> Result<(), CliError> {
    run(cmd, None)
}

pub fn run(cmd: Commands, shared: Option<&Arc<Session>>) -> Result<(), CliError> {
    let session = |anchor: Anchor<'_>| -> Result<Arc<Session>, CliError> {
        match shared {
            Some(session) => Ok(Arc::clone(session)),
            None => open_session(anchor).map(Arc::new),
        }
    };
    match cmd {
        Commands::Run(args) => {
            let session = session(anchor(args.file.as_deref(), args.eval.as_deref()))?;
            commands::run::execute(&session, args)
        }
        Commands::Check(args) => {
            let session = session(Anchor::Cwd)?;
            commands::check::execute(&session, args)
        }
        Commands::Eval(args) => {
            let session = session(Anchor::Cwd)?;
            commands::eval::execute(&session, args)
        }
        Commands::Repl(args) => {
            let session = session(Anchor::Cwd)?;
            commands::repl::execute(&session, args)
        }
        Commands::Bench(args) => {
            let session = session(anchor(args.file.as_deref(), args.eval.as_deref()))?;
            commands::bench::execute(&session, args)
        }
        Commands::Debug(args) => {
            let session = session(anchor(args.file.as_deref(), args.eval.as_deref()))?;
            commands::debug::execute(&session, args)
        }
        Commands::Build(args) => {
            let session = session(Anchor::File(std::path::Path::new(&args.file)))?;
            commands::build::execute(&session, args)
        }
        Commands::Test(args) => {
            let session = session(Anchor::Cwd)?;
            commands::test::execute(session, args)
        }
        Commands::Task(args) => {
            let session = session(Anchor::Cwd)?;
            commands::task::execute(&session, args)
        }
        Commands::Fmt(args) => {
            let session = session(Anchor::Cwd)?;
            commands::fmt::execute(&session, args)
        }
        Commands::Doctor => commands::doctor::execute(),
        Commands::Cache(sub) => commands::cache::execute(sub),
        Commands::Lsp(args) => commands::lsp::execute(args),
        Commands::Init(args) => commands::init::execute(args),
        Commands::Completions(args) => commands::completions::execute(args),
        Commands::Dap => commands::dap::execute(),
        Commands::Pkg(sub) => commands::pkg::execute(sub),
        Commands::StdBundle(args) => {
            let session = session(Anchor::Cwd)?;
            commands::std_bundle::execute(&session, args)
        }
    }
}
