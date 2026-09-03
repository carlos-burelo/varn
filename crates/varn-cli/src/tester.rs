use crate::cli::TestArgs;
use crate::error::CliError;
use crate::pipeline;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use varn_pipeline::RunOpts;

#[derive(Clone)]
pub struct TestResult {
    pub idx: usize,
    pub display_name: String,
    pub passed: bool,
    pub duration: Duration,
    pub output: String,
}

pub fn run_tests(args: TestArgs) -> Result<(), CliError> {
    let start_time = Instant::now();

    let test_files = discover_test_files(args.path.as_deref())?;
    if test_files.is_empty() {
        return Err(CliError::usage(
            "No test files (.vn) found matching the specified path",
        ));
    }

    let filtered_files: Vec<PathBuf> = if let Some(ref filter) = args.filter {
        let filter_lc = filter.to_lowercase();
        test_files
            .into_iter()
            .filter(|p| p.to_string_lossy().to_lowercase().contains(&filter_lc))
            .collect()
    } else {
        test_files
    };

    if filtered_files.is_empty() {
        return Err(CliError::usage(format!(
            "No test files matched filter '{}'",
            args.filter.as_deref().unwrap_or("")
        )));
    }

    let total_suites = filtered_files.len();
    let num_workers = args.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });

    println!(
        "\n  \x1b[1;36mvarn test\x1b[0m · {} suite{} · {} worker{}\n",
        total_suites,
        if total_suites == 1 { "" } else { "s" },
        num_workers,
        if num_workers == 1 { "" } else { "s" }
    );

    let file_queue: std::sync::Mutex<Vec<(usize, PathBuf)>> =
        std::sync::Mutex::new(filtered_files.into_iter().enumerate().collect());
    let file_queue = Arc::new(file_queue);

    let has_failure = Arc::new(AtomicBool::new(false));
    let suites_passed = Arc::new(AtomicUsize::new(0));
    let suites_failed = Arc::new(AtomicUsize::new(0));
    let results: Arc<std::sync::Mutex<Vec<TestResult>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(total_suites)));

    let mut handles = Vec::with_capacity(num_workers);

    for _ in 0..num_workers {
        let queue = Arc::clone(&file_queue);
        let has_failure_flag = Arc::clone(&has_failure);
        let suites_passed_cnt = Arc::clone(&suites_passed);
        let suites_failed_cnt = Arc::clone(&suites_failed);
        let results_store = Arc::clone(&results);
        let fail_fast = args.fail_fast;

        handles.push(std::thread::spawn(move || loop {
            if fail_fast && has_failure_flag.load(Ordering::SeqCst) {
                break;
            }

            let item = {
                let mut guard = queue.lock().unwrap();
                guard.pop()
            };
            let Some((idx, path)) = item else { break };

            let display_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());

            let t0 = Instant::now();
            let run_res = pipeline::run(&RunOpts {
                file_path: path.to_string_lossy().to_string(),
                eval: None,
                verbose: false,
                no_run: false,
                debug: Default::default(),
                trace: false,
                strict: false,
                capabilities: Default::default(),
            });
            let elapsed = t0.elapsed();

            let passed = run_res.is_ok();
            let output = run_res.err().map(|e| format!("{e}")).unwrap_or_default();

            if passed {
                suites_passed_cnt.fetch_add(1, Ordering::SeqCst);
            } else {
                suites_failed_cnt.fetch_add(1, Ordering::SeqCst);
                has_failure_flag.store(true, Ordering::SeqCst);
            }

            results_store.lock().unwrap().push(TestResult {
                idx,
                display_name,
                passed,
                duration: elapsed,
                output,
            });
        }));
    }

    // Progress bar thread — polls completed count and overwrites a single line.
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_flag);
    let results_clone = Arc::clone(&results);

    let progress_thread = std::thread::spawn(move || {
        const BAR: usize = 40;
        loop {
            let n = results_clone.lock().unwrap().len();
            let filled = (n * BAR / total_suites.max(1)).min(BAR);
            let bar = format!(
                "\x1b[36m{}\x1b[2m{}\x1b[0m",
                "█".repeat(filled),
                "░".repeat(BAR - filled)
            );
            print!("\r  [{bar}]  {n}/{total_suites}  ");
            let _ = std::io::stdout().flush();

            if done_clone.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        // Erase the progress line.
        print!("\r\x1b[2K");
        let _ = std::io::stdout().flush();
    });

    for h in handles {
        let _ = h.join();
    }
    done_flag.store(true, Ordering::Relaxed);
    let _ = progress_thread.join();

    let total_elapsed = start_time.elapsed();
    let passed_count = suites_passed.load(Ordering::SeqCst);
    let failed_count = suites_failed.load(Ordering::SeqCst);

    // Sort results by original file order for a stable grid.
    let mut all_results = results.lock().unwrap().clone();
    all_results.sort_by_key(|r| r.idx);

    // ── Block grid ────────────────────────────────────────────────────────
    println!();
    print_block_grid(&all_results, 66);
    println!();

    // ── Summary line ─────────────────────────────────────────────────────
    if failed_count == 0 {
        println!(
            "  \x1b[32m{passed_count} passed\x1b[0m  ·  \x1b[2m0 failed\x1b[0m  ·  \
             {total_suites} suites  ·  {total_elapsed:.2?}"
        );
    } else {
        println!(
            "  \x1b[32m{passed_count} passed\x1b[0m  ·  \x1b[31m{failed_count} failed\x1b[0m  \
             ·  {total_suites} suites  ·  {total_elapsed:.2?}"
        );
        println!();
        for r in all_results.iter().filter(|r| !r.passed) {
            println!(
                "  \x1b[31m✖\x1b[0m  {}  \x1b[2m({:.1?})\x1b[0m",
                r.display_name, r.duration
            );
            if !r.output.is_empty() {
                let first = r.output.lines().next().unwrap_or(&r.output);
                println!("     \x1b[2m{first}\x1b[0m");
            }
        }
    }
    println!();

    if failed_count > 0 {
        Err(CliError::fatal("Some test suites failed"))
    } else {
        Ok(())
    }
}

fn print_block_grid(results: &[TestResult], cols: usize) {
    let mut col = 0;
    print!("  ");
    for r in results {
        if r.passed {
            print!("\x1b[32m█\x1b[0m");
        } else {
            print!("\x1b[31m█\x1b[0m");
        }
        col += 1;
        if col >= cols {
            println!();
            print!("  ");
            col = 0;
        }
    }
    if col > 0 {
        println!();
    }
}

fn discover_test_files(path: Option<&str>) -> Result<Vec<PathBuf>, CliError> {
    let base = path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests"));

    if !base.exists() {
        return Err(CliError::usage(format!(
            "Path '{}' does not exist",
            base.display()
        )));
    }

    if base.is_file() {
        return Ok(vec![base]);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&base)
        .map_err(|e| {
            CliError::fatal(format!(
                "Failed to read directory '{}': {e}",
                base.display()
            ))
        })?
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "vn").unwrap_or(false))
        .collect();

    files.sort();
    Ok(files)
}
