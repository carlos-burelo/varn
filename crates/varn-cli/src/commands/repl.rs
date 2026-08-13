use crate::{cli::ReplArgs, error::CliError};
use std::io::{self, Write};
use varn_pipeline::{DebugFlags, RunOpts};
use varn_term::terminal;

pub fn execute(_args: ReplArgs) -> Result<(), CliError> {
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
            run_snippet(&source);
        }
    }

    println!("\nsaliendo.");
    Ok(())
}

fn run_snippet(source: &str) {
    let opts = RunOpts {
        file_path: "(repl)".to_owned(),
        eval: Some(source.to_owned()),
        verbose: false,
        no_run: false,
        debug: DebugFlags::default(),
        trace: false,
        strict: false,
    };
    if let Err(e) = crate::pipeline::run(&opts) {
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
