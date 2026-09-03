//! `xtask` — Cargo task automation for Varn.
//!
//! Provides the native Rust benchmark comparison harness (`cargo xtask compare` / `cargo xtask bench`).
//! Measures process wall-clock time with sub-millisecond precision, calibrates startup, isolates
//! bytecode caches, interleaves executions round-robin to mitigate thermal drift, and validates output
//! integrity across runtimes.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Benchmark definition mapping canonical benchmark names to runtime-specific source files.
#[derive(Debug, Clone)]
struct BenchDef {
    name: &'static str,
    vn: &'static str,
    ts: &'static str,
    py: Option<&'static str>,
}

const ALL_BENCHMARKS: &[BenchDef] = &[
    BenchDef {
        name: "fib",
        vn: "bench_fib.vn",
        ts: "bench_fib.ts",
        py: Some("py/fib.py"),
    },
    BenchDef {
        name: "gc_alloc",
        vn: "bench_gc_alloc.vn",
        ts: "bench_gc_alloc.ts",
        py: Some("py/gc_alloc.py"),
    },
    BenchDef {
        name: "dto",
        vn: "bench_dto_local.vn",
        ts: "bench_dto.ts",
        py: Some("py/dto.py"),
    },
    BenchDef {
        name: "matrix",
        vn: "bench_matrix.vn",
        ts: "bench_matrix.ts",
        py: Some("py/matrix.py"),
    },
    BenchDef {
        name: "str_ops",
        vn: "bench_str_ops.vn",
        ts: "bench_str_ops.ts",
        py: None,
    },
    BenchDef {
        name: "json_native",
        vn: "bench_json.vn",
        ts: "bench_json.ts",
        py: None,
    },
    BenchDef {
        name: "json_pure",
        vn: "bench_json_pure.vn",
        ts: "bench_json_pure.ts",
        py: None,
    },
    BenchDef {
        name: "csv_pipeline",
        vn: "bench_csv_pipeline.vn",
        ts: "bench_csv_pipeline.ts",
        py: None,
    },
    BenchDef {
        name: "collection_pipeline",
        vn: "bench_collection_pipeline.vn",
        ts: "bench_collection_pipeline.ts",
        py: None,
    },
    BenchDef {
        name: "http_routing",
        vn: "bench_http_routing.vn",
        ts: "bench_http_routing.ts",
        py: None,
    },
    BenchDef {
        name: "csv_etl",
        vn: "bench_csv_etl.vn",
        ts: "bench_csv_etl.ts",
        py: None,
    },
    BenchDef {
        name: "json_api_payloads",
        vn: "bench_json_api_payloads.vn",
        ts: "bench_json_api_payloads.ts",
        py: None,
    },
];

/// Parsed CLI options for the compare command.
#[derive(Debug, Clone)]
struct Opts {
    runs: usize,
    warmup: usize,
    only: Option<Vec<String>>,
    baseline: Option<PathBuf>,
    skip_python: bool,
    compact: bool,
    markdown: bool,
    json: bool,
    detailed: bool,
    no_color: bool,
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

fn parse_args() -> Result<Opts, String> {
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

fn print_help() {
    println!(
        r#"cargo xtask compare — High-precision runtime benchmark comparison harness

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

/// Information about an available runtime launcher.
#[derive(Debug, Clone)]
struct RuntimeInfo {
    name: String,
    bin: PathBuf,
    args_prefix: Vec<String>,
    empty_ext: &'static str,
    empty_body: &'static str,
}

/// Statistics calculated over timed samples.
#[derive(Debug, Clone, Serialize)]
struct SampleStats {
    count: usize,
    median: f64,
    min: f64,
    max: f64,
    mean: f64,
    stddev: f64,
    p95: f64,
}

impl SampleStats {
    fn compute(samples: &[f64]) -> Self {
        if samples.is_empty() {
            return Self {
                count: 0,
                median: 0.0,
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                stddev: 0.0,
                p95: 0.0,
            };
        }
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len();

        let median = if count % 2 == 1 {
            sorted[count / 2]
        } else {
            (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
        };

        let min = sorted[0];
        let max = sorted[count - 1];

        let sum: f64 = sorted.iter().sum();
        let mean = sum / (count as f64);

        let variance = if count > 1 {
            sorted.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / ((count - 1) as f64)
        } else {
            0.0
        };
        let stddev = variance.sqrt();

        let p95_idx = ((count as f64 * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(count - 1);
        let p95 = sorted[p95_idx];

        Self {
            count,
            median: round1(median),
            min: round1(min),
            max: round1(max),
            mean: round1(mean),
            stddev: round1(stddev),
            p95: round1(p95),
        }
    }
}

fn round1(val: f64) -> f64 {
    (val * 10.0).round() / 10.0
}

fn round2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

/// Single timed execution result.
struct RunResult {
    ms: f64,
    signature: String,
    output: String,
}

/// Consolidated benchmark row result.
struct RowResult {
    bench_name: String,
    output_ok: bool,
    outputs_by_rt: HashMap<String, String>,
    total_stats: HashMap<String, SampleStats>,
    work_stats: HashMap<String, SampleStats>,
    best_rival: Option<String>,
    rival_work: Option<f64>,
    work_ratio: Option<f64>,
    resolved: bool,
}

/// Extracted comparable signature from raw output.
fn get_result_signature(raw: &str) -> String {
    let re_drop =
        Regex::new(r"(?i)elapsed|took|\btime\b|(?:^|[^a-z])ms\b|_ms\b|\bms\s*[=:]|\bbytes\b")
            .unwrap();
    let mut nums = Vec::new();

    for line in raw.lines() {
        if re_drop.is_match(line) {
            continue;
        }
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        while i < len {
            if bytes[i].is_ascii_digit()
                || (bytes[i] == b'-' && i + 1 < len && bytes[i + 1].is_ascii_digit())
            {
                let preceded_by_dot = i > 0 && bytes[i - 1] == b'.';
                let start = i;
                if bytes[i] == b'-' {
                    i += 1;
                }
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let followed_by_dot = i < len && bytes[i] == b'.';
                if !preceded_by_dot && !followed_by_dot {
                    nums.push(line[start..i].to_string());
                } else {
                    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                        i += 1;
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    nums.join(",")
}

fn find_binary(cmd: &str) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(cmd);
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate_exe = dir.join(format!("{cmd}.exe"));
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
                let candidate_cmd = dir.join(format!("{cmd}.cmd"));
                if candidate_cmd.is_file() {
                    return Some(candidate_cmd);
                }
            }
        }
    }
    None
}

/// Terminal styling and color helper.
struct Term {
    no_color: bool,
}

#[allow(dead_code)]
impl Term {
    fn new(no_color: bool) -> Self {
        Self { no_color }
    }

    fn color(&self, code: &str, s: &str) -> String {
        if self.no_color {
            s.to_string()
        } else {
            format!("\x1b[{code}m{s}\x1b[0m")
        }
    }

    fn cyan(&self, s: &str) -> String {
        self.color("36", s)
    }
    fn bold_cyan(&self, s: &str) -> String {
        self.color("1;36", s)
    }
    fn green(&self, s: &str) -> String {
        self.color("32", s)
    }
    fn bold_green(&self, s: &str) -> String {
        self.color("1;32", s)
    }
    fn red(&self, s: &str) -> String {
        self.color("31", s)
    }
    fn bold_red(&self, s: &str) -> String {
        self.color("1;31", s)
    }
    fn yellow(&self, s: &str) -> String {
        self.color("33", s)
    }
    fn gray(&self, s: &str) -> String {
        self.color("90", s)
    }
    fn white(&self, s: &str) -> String {
        self.color("37", s)
    }
    fn bold_white(&self, s: &str) -> String {
        self.color("1;37", s)
    }

    fn rt_color(&self, rt: &str, s: &str) -> String {
        match rt {
            "varn" => self.color("1;36", s),      // Bright Cyan
            "bun" => self.color("1;33", s),       // Bright Amber
            "node" => self.color("1;32", s),      // Bright Green
            "python" => self.color("1;34", s),    // Bright Blue
            "varn-base" => self.color("1;35", s), // Bright Magenta
            _ => self.white(s),
        }
    }

    fn bar(&self, rt: &str, count: usize) -> String {
        let block = "█".repeat(count);
        self.rt_color(rt, &block)
    }
}

/// Output data for JSON export.
#[derive(Debug, Serialize)]
struct JsonReport {
    cpu: String,
    runtimes: Vec<String>,
    runs: usize,
    warmup: usize,
    startup: HashMap<String, SampleStats>,
    benchmarks: Vec<JsonBenchRow>,
}

#[derive(Debug, Serialize)]
struct JsonBenchRow {
    name: String,
    output_ok: bool,
    verdict: String,
    work_ratio: Option<f64>,
    resolved: bool,
    rival: Option<String>,
    rival_work: Option<f64>,
    stats: HashMap<String, SampleStats>,
    work_stats: HashMap<String, SampleStats>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let term = Term::new(opts.no_color);
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or("Cannot determine workspace root")?
        .to_path_buf();

    let bench_dir = workspace_root.join("tests").join("benchmarks");
    let target_vn = workspace_root
        .join("target")
        .join("release")
        .join(if cfg!(windows) { "vn.exe" } else { "vn" });

    if !target_vn.exists() {
        eprintln!("error: vn executable not found at {}", target_vn.display());
        eprintln!("Please build release binary first: cargo build --release --bin vn");
        std::process::exit(1);
    }

    // Discover runtimes
    let mut runtimes = Vec::new();

    runtimes.push(RuntimeInfo {
        name: "varn".to_string(),
        bin: target_vn.clone(),
        args_prefix: vec!["run".to_string()],
        empty_ext: ".vn",
        empty_body: "print(1)",
    });

    if let Some(ref base_path) = opts.baseline {
        runtimes.push(RuntimeInfo {
            name: "varn-base".to_string(),
            bin: base_path.clone(),
            args_prefix: vec!["run".to_string()],
            empty_ext: ".vn",
            empty_body: "print(1)",
        });
    }

    if let Some(bun_path) = find_binary("bun") {
        runtimes.push(RuntimeInfo {
            name: "bun".to_string(),
            bin: bun_path,
            args_prefix: vec!["run".to_string()],
            empty_ext: ".ts",
            empty_body: "console.log(1)",
        });
    }

    if let Some(node_path) = find_binary("node") {
        runtimes.push(RuntimeInfo {
            name: "node".to_string(),
            bin: node_path,
            args_prefix: vec![],
            empty_ext: ".ts",
            empty_body: "console.log(1)",
        });
    }

    if !opts.skip_python {
        if let Some(py_path) = find_binary("python").or_else(|| find_binary("python3")) {
            runtimes.push(RuntimeInfo {
                name: "python".to_string(),
                bin: py_path,
                args_prefix: vec![],
                empty_ext: ".py",
                empty_body: "print(1)",
            });
        }
    }

    // Filter benchmarks if requested
    let benchmarks: Vec<&BenchDef> = if let Some(ref only_list) = opts.only {
        let filtered: Vec<&BenchDef> = ALL_BENCHMARKS
            .iter()
            .filter(|b| only_list.iter().any(|o| o.eq_ignore_ascii_case(b.name)))
            .collect();
        if filtered.is_empty() {
            eprintln!("error: no benchmark matched: {}", only_list.join(", "));
            std::process::exit(1);
        }
        filtered
    } else {
        ALL_BENCHMARKS.iter().collect()
    };

    // Isolated cache root
    let temp_root = std::env::temp_dir().join(format!("varn-bench-{}", std::process::id()));
    let cache_root = temp_root.join("caches");
    std::fs::create_dir_all(&cache_root)?;

    let mut cache_dirs: HashMap<String, PathBuf> = HashMap::new();
    for rt in &runtimes {
        let cdir = cache_root.join(&rt.name);
        std::fs::create_dir_all(&cdir)?;
        cache_dirs.insert(rt.name.clone(), cdir);
    }

    let cpu = get_cpu_info();
    let rt_names: Vec<String> = runtimes.iter().map(|r| r.name.clone()).collect();

    // Visual Header Card
    if !opts.json {
        println!();
        let box_width = 80_usize;
        println!("  {}", term.gray(&format!("┌{}┐", "─".repeat(box_width))));
        let title_plain = "⚡ VARN BENCHMARK SUITE — Comparative Performance Matrix";
        let pad_title = (box_width - 4).saturating_sub(title_plain.chars().count());
        println!(
            "  {}  {}{}{}",
            term.gray("│"),
            term.bold_cyan(title_plain),
            " ".repeat(pad_title),
            term.gray("│")
        );
        let info_line1 = format!(
            "Host: {}   •   Runs: {} ({} warmup)",
            cpu, opts.runs, opts.warmup
        );
        let pad_info1 = (box_width - 4).saturating_sub(info_line1.chars().count());
        println!(
            "  {}  {}{}{}",
            term.gray("│"),
            term.gray(&info_line1),
            " ".repeat(pad_info1),
            term.gray("│")
        );

        let info_line2 = format!("Runtimes: {}", rt_names.join(", "));
        let pad_info2 = (box_width - 4).saturating_sub(info_line2.chars().count());
        println!(
            "  {}  {}{}{}",
            term.gray("│"),
            term.cyan(&info_line2),
            " ".repeat(pad_info2),
            term.gray("│")
        );

        println!("  {}", term.gray(&format!("└{}┘", "─".repeat(box_width))));
        println!();
    }

    // -------------------------------------------------------------
    // Calibration: Startup time on empty program
    // -------------------------------------------------------------
    let startup_dir = temp_root.join("startup");
    std::fs::create_dir_all(&startup_dir)?;

    let mut startup_files = HashMap::new();
    for rt in &runtimes {
        let probe = startup_dir.join(format!("startup_{}{}", rt.name, rt.empty_ext));
        std::fs::write(&probe, rt.empty_body)?;
        startup_files.insert(rt.name.clone(), probe);
    }

    let mut startup_samples: HashMap<String, Vec<f64>> = HashMap::new();
    for rt in &runtimes {
        startup_samples.insert(rt.name.clone(), Vec::with_capacity(opts.runs));
    }

    for i in 0..opts.runs {
        for rt in &runtimes {
            let file = &startup_files[&rt.name];
            let cdir = &cache_dirs[&rt.name];
            let res = invoke_once(rt, file, cdir)?;
            if i >= opts.warmup {
                startup_samples.get_mut(&rt.name).unwrap().push(res.ms);
            }
        }
    }

    let mut startup_stats: HashMap<String, SampleStats> = HashMap::new();
    for rt in &runtimes {
        let stats = SampleStats::compute(&startup_samples[&rt.name]);
        startup_stats.insert(rt.name.clone(), stats);
    }

    // Visual Startup Gauge
    if !opts.json {
        println!(
            "  {}",
            term.bold_cyan("🚀 Startup Latency (empty program):")
        );
        let max_su = startup_stats
            .values()
            .map(|s| s.median)
            .fold(0.0f64, f64::max);

        for rt in &runtimes {
            let st = &startup_stats[&rt.name];
            let bar_len = if max_su > 0.0 {
                ((st.median / max_su) * 26.0).round() as usize
            } else {
                1
            }
            .max(1);
            let bar_str = term.bar(&rt.name, bar_len);
            let extra = if rt.name == "varn" {
                if let Some(bun_st) = startup_stats.get("bun") {
                    let factor = bun_st.median / st.median.max(0.1);
                    format!(
                        "   {}",
                        term.bold_green(&format!("[⚡ {:.1}x faster]", factor))
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let rt_pad = 7_usize.saturating_sub(rt.name.len());
            let bar_pad = 26_usize.saturating_sub(bar_len);
            let rt_colored = term.rt_color(&rt.name, &rt.name);
            let spaces = " ".repeat(rt_pad);
            println!(
                "    {rt_colored}{spaces}{:>6.1} ms  {}{}{}",
                st.median,
                bar_str,
                " ".repeat(bar_pad),
                extra
            );
        }
        println!();
    }

    // -------------------------------------------------------------
    // Benchmark Measurement Loop
    // -------------------------------------------------------------
    let mut results: Vec<RowResult> = Vec::new();

    for bench in benchmarks {
        if !opts.json {
            print!("\r  ⏳ Benchmarking: {:<20} ...", bench.name);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }

        let mut bench_files: HashMap<String, PathBuf> = HashMap::new();
        for rt in &runtimes {
            let rel = match rt.name.as_str() {
                "varn" | "varn-base" => Some(bench.vn),
                "python" => bench.py,
                _ => Some(bench.ts),
            };
            if let Some(r) = rel {
                let full = bench_dir.join(r);
                if full.is_file() {
                    bench_files.insert(rt.name.clone(), full);
                }
            }
        }

        let present_rts: Vec<&RuntimeInfo> = runtimes
            .iter()
            .filter(|r| bench_files.contains_key(&r.name))
            .collect();

        let mut samples: HashMap<String, Vec<f64>> = HashMap::new();
        let mut last_outputs: HashMap<String, String> = HashMap::new();
        let mut last_sigs: HashMap<String, String> = HashMap::new();

        for rt in &present_rts {
            samples.insert(rt.name.clone(), Vec::with_capacity(opts.runs));
        }

        for i in 0..opts.runs {
            for rt in &present_rts {
                let file = &bench_files[&rt.name];
                let cdir = &cache_dirs[&rt.name];
                let run_res = invoke_once(rt, file, cdir)?;
                if i >= opts.warmup {
                    samples.get_mut(&rt.name).unwrap().push(run_res.ms);
                }
                last_outputs.insert(rt.name.clone(), run_res.output);
                last_sigs.insert(rt.name.clone(), run_res.signature);
            }
        }

        let mut unique_sigs = std::collections::HashSet::new();
        for rt in &present_rts {
            if let Some(sig) = last_sigs.get(&rt.name) {
                if !sig.is_empty() {
                    unique_sigs.insert(sig.clone());
                }
            }
        }
        let output_ok = unique_sigs.len() <= 1;

        let mut total_stats = HashMap::new();
        let mut work_stats = HashMap::new();

        for rt in &present_rts {
            let s = &samples[&rt.name];
            let tot = SampleStats::compute(s);
            total_stats.insert(rt.name.clone(), tot);

            let su = startup_stats[&rt.name].median;
            let work_samples: Vec<f64> = s.iter().map(|&t| (t - su).max(0.0)).collect();
            let wrk = SampleStats::compute(&work_samples);
            work_stats.insert(rt.name.clone(), wrk);
        }

        let mut best_rival: Option<String> = None;
        let mut best_rival_work: Option<f64> = None;

        for rt in &present_rts {
            if rt.name != "varn" && rt.name != "varn-base" {
                if let Some(w) = work_stats.get(&rt.name) {
                    if best_rival_work.is_none() || w.median < best_rival_work.unwrap() {
                        best_rival = Some(rt.name.clone());
                        best_rival_work = Some(w.median);
                    }
                }
            }
        }

        let varn_work = work_stats.get("varn").map(|w| w.median);
        let work_ratio = match (varn_work, best_rival_work) {
            (Some(vw), Some(rw)) if vw > 0.0 => Some(round2(rw / vw)),
            _ => None,
        };

        let resolved = if let (Some(rw_name), Some(_rw)) = (&best_rival, best_rival_work) {
            let varn_w = &work_stats["varn"];
            let rival_w = &work_stats[rw_name];
            let non_overlapping = (varn_w.max < rival_w.min) || (rival_w.max < varn_w.min);
            let significant_diff =
                (varn_w.median - rival_w.median).abs() / rival_w.median.max(0.1) > 0.08;
            non_overlapping || significant_diff
        } else {
            false
        };

        results.push(RowResult {
            bench_name: bench.name.to_string(),
            output_ok,
            outputs_by_rt: last_outputs,
            total_stats,
            work_stats,
            best_rival,
            rival_work: best_rival_work,
            work_ratio,
            resolved,
        });
    }

    if !opts.json {
        print!("\r{}\r", " ".repeat(60));
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }

    // -------------------------------------------------------------
    // Reporting
    // -------------------------------------------------------------
    if opts.json {
        let json_report = JsonReport {
            cpu,
            runtimes: rt_names,
            runs: opts.runs,
            warmup: opts.warmup,
            startup: startup_stats.clone(),
            benchmarks: results
                .iter()
                .map(|r| JsonBenchRow {
                    name: r.bench_name.clone(),
                    output_ok: r.output_ok,
                    verdict: get_verdict_text(r),
                    work_ratio: r.work_ratio,
                    resolved: r.resolved,
                    rival: r.best_rival.clone(),
                    rival_work: r.rival_work,
                    stats: r.total_stats.clone(),
                    work_stats: r.work_stats.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&json_report)?);
    } else if opts.markdown {
        let mut hdr = String::from("| Benchmark");
        for rt in &runtimes {
            hdr.push_str(&format!(" | {} work", rt.name));
        }
        hdr.push_str(" | verdict (work) |");
        println!("{hdr}");

        let mut sep = String::from("|---");
        for _ in &runtimes {
            sep.push_str("|---");
        }
        sep.push_str("|---|");
        println!("{sep}");

        for r in &results {
            let mut line = format!("| {}", r.bench_name);
            for rt in &runtimes {
                if let Some(w) = r.work_stats.get(&rt.name) {
                    line.push_str(&format!(" | {:.1} ms", w.median));
                } else {
                    line.push_str(" | --");
                }
            }
            line.push_str(&format!(" | {} |", get_verdict_text(r)));
            println!("{line}");
        }
    } else if opts.compact {
        // Compact Table with inline visual relative comparison bar
        let hdr = format!(
            "  {:<20} {:>10} {:>10} {:>10}   {:<22}   {}",
            "Benchmark", "Varn", "Bun", "Node", "Relative (Varn vs Bun)", "Verdict"
        );
        println!("{}", term.bold_cyan(&hdr));
        println!("  {}", term.gray(&"─".repeat(hdr.len() - 2)));

        for r in &results {
            let varn_w = r.work_stats.get("varn").map(|w| w.median);
            let bun_w = r.work_stats.get("bun").map(|w| w.median);
            let node_w = r.work_stats.get("node").map(|w| w.median);

            let v_str = varn_w
                .map(|w| format!("{:>7.1} ms", w))
                .unwrap_or_else(|| format!("{:>10}", "--"));
            let b_str = bun_w
                .map(|w| format!("{:>7.1} ms", w))
                .unwrap_or_else(|| format!("{:>10}", "--"));
            let n_str = node_w
                .map(|w| format!("{:>7.1} ms", w))
                .unwrap_or_else(|| format!("{:>10}", "--"));

            let rel_bar = if let (Some(vw), Some(bw)) = (varn_w, bun_w) {
                let total = vw + bw;
                if total > 0.0 {
                    let v_share = ((vw / total) * 18.0).round() as usize;
                    let v_share = v_share.clamp(1, 17);
                    let b_share = 18 - v_share;
                    let v_blocks = term.rt_color("varn", &"█".repeat(v_share));
                    let b_blocks = term.rt_color("bun", &"░".repeat(b_share));
                    format!("[{v_blocks}{b_blocks}]")
                } else {
                    format!("[{}]", " ".repeat(18))
                }
            } else {
                format!(" {:<20} ", "--")
            };

            let badge = get_verdict_badge(r, &term);
            println!(
                "  {:<20} {} {} {}   {}   {}",
                r.bench_name, v_str, b_str, n_str, rel_bar, badge
            );
        }
    } else {
        // High-Impact Visual Card Mode
        let card_width: usize = 72;
        for r in &results {
            let badge = get_verdict_badge(r, &term);
            let title = if r.output_ok {
                format!("  ┌─ {} ", term.bold_white(&r.bench_name))
            } else {
                format!(
                    "  ┌─ {} {} ",
                    term.bold_white(&r.bench_name),
                    term.bold_red("[MISMATCH]")
                )
            };
            let title_len = r.bench_name.len() + 5 + if r.output_ok { 0 } else { 11 };
            let pad_dashes = card_width.saturating_sub(title_len);
            println!("{}{}", title, term.gray(&"─".repeat(pad_dashes)) + "┐");

            let mut rivals_median: Vec<f64> = r
                .work_stats
                .iter()
                .filter(|(rt, _)| *rt != "python")
                .map(|(_, st)| st.median)
                .collect();
            rivals_median.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let max_normal = rivals_median.last().copied().unwrap_or(1.0).max(0.1);
            let bar_max_cols = 36;

            for rt in &runtimes {
                if let Some(wrk) = r.work_stats.get(&rt.name) {
                    let (cols, tag) = if wrk.median > max_normal * 3.5 && rt.name == "python" {
                        (
                            bar_max_cols,
                            format!(" {}", term.gray(&format!("[+{:.0}ms]", wrk.median))),
                        )
                    } else {
                        let c =
                            ((wrk.median / max_normal) * (bar_max_cols as f64)).round() as usize;
                        let c = if wrk.median > 0.0 {
                            c.max(1).min(bar_max_cols)
                        } else {
                            0
                        };
                        (c, String::new())
                    };
                    let bar_str = term.bar(&rt.name, cols);

                    let rt_pad = 7_usize.saturating_sub(rt.name.len());
                    let rt_name_spaced = format!(
                        "{}{}",
                        term.rt_color(&rt.name, &rt.name),
                        " ".repeat(rt_pad)
                    );
                    let time_str = format!("{:>6.1} ms", wrk.median);
                    let bar_pad = bar_max_cols.saturating_sub(cols);
                    let row_visual = format!(
                        "{} {}  {}{}{}",
                        rt_name_spaced,
                        time_str,
                        bar_str,
                        " ".repeat(bar_pad),
                        tag
                    );
                    let plain_len =
                        7 + 1 + 9 + 2 + cols + bar_pad + if tag.is_empty() { 0 } else { 10 };
                    let right_pad = card_width.saturating_sub(plain_len + 4);
                    println!(
                        "  │  {}{}{}│",
                        row_visual,
                        " ".repeat(right_pad),
                        term.gray("")
                    );
                }
            }

            if !r.output_ok {
                for (rt, out) in &r.outputs_by_rt {
                    let mut snippet = out.replace(['\r', '\n'], " ");
                    if snippet.len() > 50 {
                        snippet.truncate(50);
                        snippet.push_str("...");
                    }
                    let snip_line = format!(
                        "{} {}",
                        term.bold_red(&format!("{rt}:")),
                        term.gray(&snippet)
                    );
                    let snip_pad = card_width.saturating_sub(rt.len() + 2 + snippet.len() + 4);
                    println!(
                        "  │  {}{}{}│",
                        snip_line,
                        " ".repeat(snip_pad),
                        term.gray("")
                    );
                }
            }

            let bottom_prefix = format!("  └─ {} ", badge);
            let badge_vis_len = get_verdict_badge_len(r);
            let bottom_dashes = card_width.saturating_sub(badge_vis_len + 5);
            println!(
                "{}{}",
                bottom_prefix,
                term.gray(&"─".repeat(bottom_dashes)) + "┘"
            );
            println!();
        }
    }

    // Final Visual Scoreboard
    if !opts.json {
        let mut varn_wins = 0;
        let mut rival_wins = 0;
        let mut tied = 0;
        let mut mismatches = 0;

        for r in &results {
            if !r.output_ok {
                mismatches += 1;
            } else if !r.resolved {
                tied += 1;
            } else if let Some(ratio) = r.work_ratio {
                if ratio >= 1.05 {
                    varn_wins += 1;
                } else if ratio <= 0.95 {
                    rival_wins += 1;
                } else {
                    tied += 1;
                }
            } else {
                tied += 1;
            }
        }

        let su_speedup =
            if let (Some(v), Some(b)) = (startup_stats.get("varn"), startup_stats.get("bun")) {
                b.median / v.median.max(0.1)
            } else {
                1.0
            };

        let box_width = 80_usize;
        println!("  {}", term.gray(&format!("┌{}┐", "─".repeat(box_width))));
        let title_line = format!(
            "📊 SCOREBOARD:   🏆 {} Wins   •   🤝 {} Tied   •   🔻 {} Rivals",
            varn_wins, tied, rival_wins
        );
        let title_colored = format!(
            "{}   {} {}   •   {} {}   •   {} {}",
            term.bold_white("📊 SCOREBOARD:"),
            "🏆",
            term.bold_green(&format!("{varn_wins} Wins")),
            "🤝",
            term.cyan(&format!("{tied} Tied")),
            "🔻",
            term.yellow(&format!("{rival_wins} Rivals"))
        );
        let pad_title = (box_width - 4).saturating_sub(title_line.chars().count());
        println!(
            "  {}  {}{}  {}",
            term.gray("│"),
            title_colored,
            " ".repeat(pad_title),
            term.gray("│")
        );

        let integrity = if mismatches == 0 {
            term.bold_green("100% Verified (Zero mismatches)")
        } else {
            term.bold_red(&format!("{mismatches} MISMATCHES"))
        };
        let sub_colored = format!(
            "🚀 Startup: {} faster than Bun   •   Integrity: {}",
            term.bold_green(&format!("{:.1}x", su_speedup)),
            integrity
        );
        let sub_plain = format!(
            "🚀 Startup: {:.1}x faster than Bun   •   Integrity: {}",
            su_speedup,
            if mismatches == 0 {
                "100% Verified (Zero mismatches)"
            } else {
                "MISMATCHES"
            }
        );
        let pad_sub = (box_width - 4).saturating_sub(sub_plain.chars().count());
        println!(
            "  {}  {}{}  {}",
            term.gray("│"),
            sub_colored,
            " ".repeat(pad_sub),
            term.gray("│")
        );
        println!("  {}", term.gray(&format!("└{}┘", "─".repeat(box_width))));
        println!();
    }

    let _ = std::fs::remove_dir_all(&temp_root);
    Ok(())
}

fn get_verdict_badge(r: &RowResult, term: &Term) -> String {
    if !r.output_ok {
        return term.bold_red("❌ OUTPUT MISMATCH");
    }
    let Some(ratio) = r.work_ratio else {
        return term.gray("--");
    };
    if !r.resolved {
        return term.cyan("🤝 ~tied (ranges overlap)");
    }
    if ratio >= 1.05 {
        term.bold_green(&format!("🏆 {:.2}x faster", ratio))
    } else if ratio <= 0.95 {
        term.yellow(&format!("🔻 {:.2}x slower", 1.0 / ratio))
    } else {
        term.cyan("🤝 ~tied (parity)")
    }
}

fn get_verdict_badge_len(r: &RowResult) -> usize {
    if !r.output_ok {
        return 17;
    }
    let Some(ratio) = r.work_ratio else {
        return 2;
    };
    if !r.resolved {
        return 24;
    }
    if ratio >= 1.05 || ratio <= 0.95 {
        15
    } else {
        17
    }
}

fn get_verdict_text(r: &RowResult) -> String {
    if !r.output_ok {
        return "OUTPUT MISMATCH".to_string();
    }
    let Some(ratio) = r.work_ratio else {
        return "--".to_string();
    };
    if !r.resolved {
        return "not resolved".to_string();
    }
    if ratio >= 1.0 {
        format!("{:.2}x faster", ratio)
    } else {
        format!("{:.2}x slower", 1.0 / ratio)
    }
}

fn invoke_once(
    rt: &RuntimeInfo,
    file: &Path,
    cache_dir: &Path,
) -> Result<RunResult, Box<dyn std::error::Error>> {
    let mut cmd = Command::new(&rt.bin);
    for arg in &rt.args_prefix {
        cmd.arg(arg);
    }
    cmd.arg(file);
    cmd.env("VARN_CACHE_DIR", cache_dir);

    let start = Instant::now();
    let output = cmd.output()?;
    let duration = start.elapsed();
    let ms = duration.as_secs_f64() * 1000.0;

    let mut raw = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        if !raw.is_empty() {
            raw.push('\n');
        }
        raw.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let trimmed = raw.trim();
    let signature = get_result_signature(trimmed);

    Ok(RunResult {
        ms,
        signature,
        output: trimmed.to_string(),
    })
}

fn get_cpu_info() -> String {
    #[cfg(windows)]
    {
        let out = Command::new("reg")
            .args([
                "query",
                r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
                "/v",
                "ProcessorNameString",
            ])
            .output()
            .ok();
        if let Some(o) = out {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                if let Some(pos) = line.find("REG_SZ") {
                    let name = line[pos + 6..].trim();
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
        }
        if let Ok(id) = std::env::var("PROCESSOR_IDENTIFIER") {
            return id;
        }
    }
    "unknown".to_string()
}
