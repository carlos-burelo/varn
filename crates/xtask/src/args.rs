use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(super) struct Opts {
    pub(super) runs: usize,
    pub(super) warmup: usize,
    pub(super) only: Option<Vec<String>>,
    pub(super) baseline: Option<PathBuf>,
    pub(super) skip_python: bool,
    pub(super) compact: bool,
    pub(super) markdown: bool,
    pub(super) json: bool,
    pub(super) detailed: bool,
    pub(super) no_color: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            runs: 7,
            warmup: 2,
            only: None,
            baseline: None,
            skip_python: false,
            compact: false,
            markdown: false,
            json: false,
            detailed: false,
            no_color: false,
        }
    }
}

pub(super) fn parse_args() -> Result<Opts, String> {
    let mut opts = Opts::default();
    let mut args = std::env::args().skip(1);

    if let Some(first) = args.next() {
        if first != "compare" && first != "bench" {
            if first == "--help" || first == "-h" {
                print_help();
                std::process::exit(0);
            }
            if first.starts_with('-') {
                parse_option(&mut opts, &first, &mut args)?;
            } else {
                return Err(format!(
                    "Unknown command: '{first}'. Usage: cargo xtask compare [options]"
                ));
            }
        }
    }

    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            print_help();
            std::process::exit(0);
        }
        parse_option(&mut opts, &arg, &mut args)?;
    }

    if opts.runs < 3 {
        return Err("--runs must be at least 3 (two are warmup)".into());
    }
    if opts.warmup >= opts.runs {
        return Err("--warmup must be less than --runs".into());
    }

    Ok(opts)
}

fn parse_option(
    opts: &mut Opts,
    flag: &str,
    args: &mut impl Iterator<Item = String>,
) -> Result<(), String> {
    match flag {
        "--runs" | "-r" => {
            let val = args
                .next()
                .ok_or_else(|| "--runs requires an integer value".to_string())?;
            opts.runs = val
                .parse::<usize>()
                .map_err(|_| format!("Invalid runs value: '{val}'"))?;
        }
        "--warmup" | "-w" => {
            let val = args
                .next()
                .ok_or_else(|| "--warmup requires an integer value".to_string())?;
            opts.warmup = val
                .parse::<usize>()
                .map_err(|_| format!("Invalid warmup value: '{val}'"))?;
        }
        "--only" | "-o" => {
            let val = args.next().ok_or_else(|| {
                "--only requires a benchmark name or comma-separated list".to_string()
            })?;
            let items: Vec<String> = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(ref mut existing) = opts.only {
                existing.extend(items);
            } else {
                opts.only = Some(items);
            }
        }
        "--baseline" | "-b" => {
            let val = args
                .next()
                .ok_or_else(|| "--baseline requires a path to vn binary".to_string())?;
            let path = PathBuf::from(&val);
            if !path.exists() {
                return Err(format!("Baseline binary not found at: {}", path.display()));
            }
            opts.baseline = Some(path);
        }
        "--skip-python" | "-sp" => {
            opts.skip_python = true;
        }
        "--compact" | "-c" => {
            opts.compact = true;
        }
        "--markdown" | "-m" => {
            opts.markdown = true;
        }
        "--json" | "-j" => {
            opts.json = true;
        }
        "--detailed" | "-d" => {
            opts.detailed = true;
        }
        "--no-color" => {
            opts.no_color = true;
        }
        other => {
            return Err(format!(
                "Unknown option: '{other}'. Use --help to see available flags."
            ));
        }
    }
    Ok(())
}

pub(super) fn print_help() {
    println!(
        r#"cargo xtask compare — High-precision runtime benchmark comparison harness
cargo xtask std-scaffold <mod> [--class Name] [--dry-run] — New stdlib module scaffold

USAGE:
    cargo xtask compare [OPTIONS]
    cargo xtask bench [OPTIONS]

OPTIONS:
    -r, --runs <N>         Total runs per benchmark per runtime (default: 7, min: 3)
    -w, --warmup <N>       Warmup runs discarded before sampling (default: 2)
    -o, --only <NAMES>     Comma-separated list of benchmarks to run (e.g. fib,matrix,str_ops)
    -b, --baseline <PATH>  Path to baseline vn executable to compare against as 'varn-base'
    -sp, --skip-python     Skip Python execution even if installed
    -c, --compact          Compact visual table with relative progress bars
    -m, --markdown         Print results as a Markdown table
    -j, --json             Output detailed results as JSON
    -d, --detailed         Include extended statistics (mean, stddev, P95)
        --no-color         Disable ANSI color output
    -h, --help             Show this help message
"#
    );
}
