use crate::cli::FmtArgs;
use crate::error::CliError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run_fmt(args: FmtArgs) -> Result<(), CliError> {
    let start_time = Instant::now();
    let target = args.path.as_deref().unwrap_or(".");
    let files = discover_vn_files(Path::new(target))?;

    if files.is_empty() {
        if args.verbose {
            println!("No .vn files found to format in '{target}'");
        }
        return Ok(());
    }

    let mut changed = 0;
    let mut unformatted = Vec::new();

    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                return Err(CliError::fatal(format!(
                    "Failed to read {}: {e}",
                    file.display()
                )));
            }
        };

        let formatted = format_source(&content);

        if content != formatted {
            changed += 1;
            if args.check {
                unformatted.push(file.clone());
            } else {
                if let Err(e) = fs::write(file, &formatted) {
                    return Err(CliError::fatal(format!(
                        "Failed to write {}: {e}",
                        file.display()
                    )));
                }
                if args.verbose {
                    println!("  \x1b[32mformatted\x1b[0m {}", file.display());
                }
            }
        }
    }

    let elapsed = start_time.elapsed();

    if args.check {
        if !unformatted.is_empty() {
            eprintln!(
                "\n\x1b[31merror\x1b[0m: {} file{} not properly formatted:\n",
                unformatted.len(),
                if unformatted.len() == 1 { "" } else { "s" }
            );
            for f in &unformatted {
                eprintln!("  - {}", f.display());
            }
            eprintln!("\nRun \x1b[1mvn fmt\x1b[0m to format these files.\n");
            return Err(CliError::usage(format!(
                "{} unformatted files found",
                unformatted.len()
            )));
        } else {
            println!(
                "\n  \x1b[1;32m✓\x1b[0m All {} .vn file{} correctly formatted ({:.2?})\n",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                elapsed
            );
        }
    } else {
        println!(
            "\n  \x1b[1;36mvarn fmt\x1b[0m · {} file{} checked, {} formatted ({:.2?})\n",
            files.len(),
            if files.len() == 1 { "" } else { "s" },
            changed,
            elapsed
        );
    }

    Ok(())
}

fn discover_vn_files(path: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut files = Vec::new();
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("vn") {
            files.push(path.to_path_buf());
        }
        return Ok(files);
    }

    if path.is_dir() {
        collect_vn_recursive(path, &mut files)?;
        files.sort();
    }

    Ok(files)
}

fn collect_vn_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CliError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_vn_recursive(&p, out)?;
        } else if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("vn") {
            out.push(p);
        }
    }
    Ok(())
}

pub fn format_source(source: &str) -> String {
    varn_fmt::format_source(source)
}
