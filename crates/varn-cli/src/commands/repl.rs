use crate::{cli::ReplArgs, error::CliError};
use std::io::{self, Write};
use varn_core::term::terminal;
use varn_pipeline::DebugFlags;

pub fn execute(session: &varn_pipeline::Session, args: ReplArgs) -> Result<(), CliError> {
    println!("Varn {} — REPL interactivo", env!("CARGO_PKG_VERSION"));
    println!("  .exit / .quit  → salir");
    println!("  .help          → ayuda");

    let stdin = io::stdin();
    let mut buf = String::new();
    let mut depth: i32 = 0;

    loop {
        let prompt = if buf.is_empty() { "Varn> " } else { "   > " };
        print!("{prompt}");
        io::stdout().flush().ok();

        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Err(_) => break,
            Ok(_) => {}
        }

        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        match trimmed {
            ".exit" | ".quit" => break,
            ".help" => {
                print_repl_help();
                continue;
            }
            ".clear" => {
                buf.clear();
                depth = 0;
                continue;
            }
            "" if buf.is_empty() => continue,
            _ => {}
        }

        buf.push_str(trimmed);
        buf.push('\n');
        depth += count_brace_delta(trimmed);

        if depth <= 0 {
            depth = 0;
            let source = std::mem::take(&mut buf);
            run_snippet(session, &args.permissions, &source);
        }
    }

    println!("\nsaliendo.");
    Ok(())
}

fn run_snippet(
    session: &varn_pipeline::Session,
    permissions: &crate::cli::PermissionArgs,
    source: &str,
) {
    let debug = DebugFlags::default();
    let result = crate::exec::compile(
        session,
        varn_pipeline::Source::virtual_source(crate::source_name::REPL, source),
        varn_pipeline::CachePolicy::Bypass,
        &debug,
        false,
    )
    .and_then(|executable| {
        crate::exec::run(
            session,
            executable,
            crate::exec::policy(permissions, &debug, false),
        )
    });
    if let Err(e) = result {
        terminal::error(&e);
    }
}

fn count_brace_delta(line: &str) -> i32 {
    let opens = line.chars().filter(|&c| c == '{').count() as i32;
    let closes = line.chars().filter(|&c| c == '}').count() as i32;
    opens - closes
}

fn print_repl_help() {
    println!("Comandos REPL:");
    println!("  .exit / .quit  Salir del REPL");
    println!("  .clear         Limpiar buffer actual");
    println!("  .help          Esta ayuda");
    println!("  Ctrl+D         EOF — salir");
}
