use crate::cli::Commands;
use crate::commands;
use crate::error::CliError;
use crate::pipeline::{Anchor, Session};
use std::path::Path;
use std::sync::Arc;
use varn_project::Project;

fn run_anchor<'a>(file: Option<&'a str>, eval: Option<&str>) -> Anchor<'a> {
    match (file, eval) {
        (_, Some(_)) | (None, None) => Anchor::Cwd,
        (Some(file), None) => Anchor::File(Path::new(file)),
    }
}

fn path_anchor(path: Option<&str>) -> Anchor<'_> {
    match path.map(Path::new) {
        Some(path) if path.is_dir() => Anchor::Dir(path),
        Some(path) => Anchor::File(path),
        None => Anchor::Cwd,
    }
}

fn anchor_of(cmd: &Commands) -> Option<Anchor<'_>> {
    match cmd {
        Commands::Run(args) => Some(run_anchor(args.file.as_deref(), args.eval.as_deref())),
        Commands::Bench(args) => Some(run_anchor(args.file.as_deref(), args.eval.as_deref())),
        Commands::Debug(args) => Some(run_anchor(args.file.as_deref(), args.eval.as_deref())),
        Commands::Build(args) => Some(path_anchor(Some(&args.file))),
        Commands::Check(args) => Some(path_anchor(args.files.first().map(String::as_str))),
        Commands::Test(args) => Some(path_anchor(args.path.as_deref())),
        Commands::Fmt(args) => Some(path_anchor(args.path.as_deref())),
        Commands::Eval(_)
        | Commands::Repl(_)
        | Commands::Task(_)
        | Commands::Pkg(_)
        | Commands::Doctor
        | Commands::Lsp(_)
        | Commands::Dap
        | Commands::StdBundle(_) => Some(Anchor::Cwd),
        Commands::Init(_) | Commands::Cache(_) | Commands::Completions(_) => None,
    }
}

pub fn project_for(cmd: &Commands) -> Result<Project, CliError> {
    match anchor_of(cmd) {
        Some(anchor) => Project::discover(anchor).map_err(CliError::from),
        None => Ok(Project::detached()),
    }
}

pub fn run(cmd: Commands, session: &Arc<Session>) -> Result<(), CliError> {
    match cmd {
        Commands::Run(args) => commands::run::execute(session, args),
        Commands::Check(args) => commands::check::execute(session, args),
        Commands::Eval(args) => commands::eval::execute(session, args),
        Commands::Repl(args) => commands::repl::execute(session, args),
        Commands::Bench(args) => commands::bench::execute(session, args),
        Commands::Debug(args) => commands::debug::execute(session, args),
        Commands::Build(args) => commands::build::execute(session, args),
        Commands::Test(args) => commands::test::execute(Arc::clone(session), args),
        Commands::Task(args) => commands::task::execute(session, args),
        Commands::Fmt(args) => commands::fmt::execute(session, args),
        Commands::Doctor => commands::doctor::execute(),
        Commands::Cache(sub) => commands::cache::execute(sub),
        Commands::Lsp(args) => commands::lsp::execute(args),
        Commands::Init(args) => commands::init::execute(args),
        Commands::Completions(args) => commands::completions::execute(args),
        Commands::Dap => commands::dap::execute(),
        Commands::Pkg(sub) => {
            let project = varn_project::Project::owning(varn_project::Anchor::Cwd)?;
            commands::pkg::execute(&project, sub)
        }
        Commands::StdBundle(args) => commands::std_bundle::execute(session, args),
    }
}
