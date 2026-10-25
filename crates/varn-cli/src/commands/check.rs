use std::path::{Path, PathBuf};

use crate::discover::{vn_files, Depth};
use crate::exit::ExitCode;
use crate::{cli::CheckArgs, debug_print, error::CliError, pipeline};
use varn_core::term::terminal;
use varn_project::FileFilter;

pub fn execute(session: &pipeline::Session, args: CheckArgs) -> Result<(), CliError> {
    let files = targets(session, &args.files)?;
    let single = files.len() == 1;
    let mut diagnostics = 0usize;
    let mut fatal = 0usize;
    for file in &files {
        match check_file(session, file, args.verbose) {
            Ok(()) => {}
            Err(error) if single => return Err(error),
            Err(error) => match error.exit_code() {
                ExitCode::Diagnostics => {
                    eprintln!("{error}");
                    diagnostics += 1;
                }
                ExitCode::Fatal | ExitCode::Usage => {
                    terminal::error(&error);
                    fatal += 1;
                }
            },
        }
    }
    if !single {
        terminal::tagged("Varn", format_args!("Checked {} files", files.len()));
    }
    let failed = diagnostics + fatal;
    if failed == 0 {
        return Ok(());
    }
    let exit = if fatal == 0 {
        ExitCode::Diagnostics
    } else {
        ExitCode::Fatal
    };
    Err(CliError::Summary {
        message: format!("{failed} of {} files failed the check", files.len()),
        exit,
    })
}

fn targets(session: &pipeline::Session, requested: &[String]) -> Result<Vec<PathBuf>, CliError> {
    let project = session.project();
    if requested.is_empty() {
        let main =
            project.manifest.main.as_deref().ok_or_else(|| {
                CliError::usage("no files given and varn.toml declares no 'main'")
            })?;
        return Ok(vec![project.root.join(main)]);
    }
    let mut files = Vec::new();
    for request in requested {
        let path = Path::new(request);
        if path.is_dir() {
            files.extend(vn_files(
                path,
                Depth::Recursive,
                &FileFilter::default(),
                &project.root,
            )?);
        } else {
            files.push(path.to_path_buf());
        }
    }
    if files.is_empty() {
        return Err(CliError::usage("no .vn files found to check"));
    }
    Ok(files)
}

fn check_file(session: &pipeline::Session, file: &Path, verbose: bool) -> Result<(), CliError> {
    let name = file.to_string_lossy();
    if pipeline::portable::is_portable(&name) {
        return crate::exec::load(session, &name, &Default::default(), verbose).map(|_| ());
    }
    let source = pipeline::Source::file(&name)?;
    let compilation = session.compile(source, pipeline::Request::checked());
    debug_print::report(&compilation, &Default::default(), verbose);
    match compilation.failure {
        Some(failure) => Err(CliError::from(failure)),
        None => Ok(()),
    }
}
