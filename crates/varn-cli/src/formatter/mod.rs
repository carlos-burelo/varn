use crate::cli::FmtArgs;
use crate::error::CliError;
use std::fs;
use std::path::Path;
use std::time::Instant;
use varn_core::term::chalk;

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

pub fn run_fmt(project: &varn_project::Project, args: FmtArgs) -> Result<(), CliError> {
    let start_time = Instant::now();
    let target = args.path.as_deref().unwrap_or(".");
    let files = crate::discover::vn_files(
        Path::new(target),
        crate::discover::Depth::Recursive,
        &project.manifest.fmt,
        &project.root,
    )?;

    if files.is_empty() {
        if args.verbose {
            println!("No .vn files found to format in '{target}'");
        }
        return Ok(());
    }

    let mut changed = 0;
    let mut unformatted = Vec::new();

    for file in &files {
        let content = fs::read_to_string(file)
            .map_err(|e| CliError::fatal(format!("Failed to read {}: {e}", file.display())))?;

        let formatted = format_source(&content);

        if content != formatted {
            changed += 1;
            if args.check {
                unformatted.push(file.clone());
            } else {
                fs::write(file, &formatted).map_err(|e| {
                    CliError::fatal(format!("Failed to write {}: {e}", file.display()))
                })?;
                if args.verbose {
                    println!("  {} {}", chalk("formatted").green(), file.display());
                }
            }
        }
    }

    let elapsed = start_time.elapsed();

    if args.check {
        if !unformatted.is_empty() {
            eprintln!(
                "\n{}: {} file{} not properly formatted:\n",
                chalk("error").red(),
                unformatted.len(),
                plural(unformatted.len())
            );
            for f in &unformatted {
                eprintln!("  - {}", f.display());
            }
            eprintln!("\nRun {} to format these files.\n", chalk("vn fmt").bold());
            return Err(CliError::usage(format!(
                "{} unformatted files found",
                unformatted.len()
            )));
        }
        println!(
            "\n  {} All {} .vn file{} correctly formatted ({:.2?})\n",
            chalk("✓").green().bold(),
            files.len(),
            plural(files.len()),
            elapsed
        );
    } else {
        println!(
            "\n  {} · {} file{} checked, {} formatted ({:.2?})\n",
            chalk("varn fmt").cyan().bold(),
            files.len(),
            plural(files.len()),
            changed,
            elapsed
        );
    }

    Ok(())
}

pub fn format_source(source: &str) -> String {
    varn_core::fmt::format_source(source)
}
