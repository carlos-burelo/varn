use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use varn_core::term::terminal;

use crate::cli::{with_implicit_run, Cli, Commands, TaskArgs};
use crate::error::CliError;
use crate::pipeline::Session;

pub fn execute(session: &Arc<Session>, args: TaskArgs) -> Result<(), CliError> {
    let tasks = &session.project().manifest.tasks;
    let Some(name) = args.name else {
        list(tasks);
        return Ok(());
    };
    let _cwd = WorkingDir::enter(&session.project().root)?;
    run_task(session, &name, &args.args, &mut Vec::new())
}

fn list(tasks: &BTreeMap<String, Vec<String>>) {
    if tasks.is_empty() {
        terminal::log("No tasks declared in varn.toml.");
        return;
    }
    for (name, lines) in tasks {
        terminal::log(name);
        for line in lines {
            terminal::log(format!("  {line}"));
        }
    }
}

fn run_task(
    session: &Arc<Session>,
    name: &str,
    extra_args: &[String],
    stack: &mut Vec<String>,
) -> Result<(), CliError> {
    if stack.iter().any(|running| running == name) {
        return Err(CliError::usage(format!(
            "task cycle: {} → {name}",
            stack.join(" → ")
        )));
    }
    let tasks = &session.project().manifest.tasks;
    let Some(lines) = tasks.get(name) else {
        let known: Vec<&str> = tasks.keys().map(String::as_str).collect();
        return Err(CliError::usage(format!(
            "unknown task '{name}' (declared: {})",
            if known.is_empty() {
                "none".to_owned()
            } else {
                known.join(", ")
            }
        )));
    };

    stack.push(name.to_owned());
    for (index, line) in lines.iter().enumerate() {
        let mut tokens = shlex::split(line).ok_or_else(|| {
            CliError::usage(format!("task '{name}': unbalanced quotes in `{line}`"))
        })?;
        if index + 1 == lines.len() {
            tokens.extend(extra_args.iter().cloned());
        }
        run_tokens(session, tokens, stack)?;
    }
    stack.pop();
    Ok(())
}

fn run_tokens(
    session: &Arc<Session>,
    tokens: Vec<String>,
    stack: &mut Vec<String>,
) -> Result<(), CliError> {
    let argv = with_implicit_run(std::iter::once("vn".to_owned()).chain(tokens).collect());
    let cli = Cli::try_parse_from(argv).map_err(|e| CliError::usage(e.to_string()))?;
    match cli.command {
        Commands::Task(inner) => match inner.name {
            Some(name) => run_task(session, &name, &inner.args, stack),
            None => {
                list(&session.project().manifest.tasks);
                Ok(())
            }
        },
        other => crate::dispatch::run(other, Some(session)),
    }
}

struct WorkingDir(Option<PathBuf>);

impl WorkingDir {
    fn enter(dir: &Path) -> Result<WorkingDir, CliError> {
        let previous = std::env::current_dir().ok();
        if previous.as_deref() == Some(dir) {
            return Ok(WorkingDir(None));
        }
        std::env::set_current_dir(dir).map_err(|e| {
            CliError::fatal(format!(
                "cannot enter project root '{}': {e}",
                dir.display()
            ))
        })?;
        Ok(WorkingDir(previous))
    }
}

impl Drop for WorkingDir {
    fn drop(&mut self) {
        if let Some(previous) = &self.0 {
            let _ = std::env::set_current_dir(previous);
        }
    }
}
