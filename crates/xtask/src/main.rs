use std::collections::HashMap;
use std::path::PathBuf;

mod args;
mod benches;
mod matrix;
mod measure;
mod report;
mod run;
mod scaffold;
mod stats;
mod term;
mod verdict;

use args::parse_args;
use term::Term;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().is_some_and(|c| c == "std-scaffold") {
        if let Err(e) = scaffold::run(&raw[1..]) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
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

    let runtimes = run::discover_runtimes(&opts, &target_vn);
    let benchmarks = benches::select_benchmarks(&opts.only)?;

    let temp_root = std::env::temp_dir().join(format!("varn-bench-{}", std::process::id()));
    let cache_root = temp_root.join("caches");
    std::fs::create_dir_all(&cache_root)?;

    let mut cache_dirs: HashMap<String, PathBuf> = HashMap::new();
    for rt in &runtimes {
        let cdir = cache_root.join(&rt.name);
        std::fs::create_dir_all(&cdir)?;
        cache_dirs.insert(rt.name.clone(), cdir);
    }

    let cpu = run::get_cpu_info();
    let rt_names: Vec<String> = runtimes.iter().map(|r| r.name.clone()).collect();

    report::print_header(&term, &opts, &cpu, &rt_names);

    let startup_stats =
        measure::calibrate_runtimes(&term, &opts, &runtimes, &temp_root, &cache_dirs)?;

    let results = measure::measure_benchmarks(
        &opts,
        &runtimes,
        &benchmarks,
        &bench_dir,
        &cache_dirs,
        &startup_stats,
    )?;

    matrix::print_results(
        &term,
        &opts,
        &runtimes,
        &results,
        &cpu,
        &rt_names,
        &startup_stats,
    )?;

    report::print_scoreboard(&term, &opts, &results, &startup_stats);

    let _ = std::fs::remove_dir_all(&temp_root);
    Ok(())
}
