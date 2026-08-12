use crate::cli::TestArgs;
use crate::error::CliError;
use crate::pipeline;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use varn_pipeline::RunOpts;

pub struct TestResult {
    pub display_name: String,
    pub passed: bool,
    pub duration: Duration,
    pub output: String,
}

pub fn run_tests(args: TestArgs) -> Result<(), CliError> {
    let start_time = Instant::now();

    // 1. Resolve test target files
    let test_files = discover_test_files(args.path.as_deref())?;

    if test_files.is_empty() {
        return Err(CliError::usage(
            "No test files (.vn) found matching the specified path",
        ));
    }

    // 2. Filter test files if --filter is provided
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
        "\n  \x1b[1;36mvarn test\x1b[0m · {} test suite{} found · {} worker{}\n",
        total_suites,
        if total_suites == 1 { "" } else { "s" },
        num_workers,
        if num_workers == 1 { "" } else { "s" }
    );

    // 3. Prepare parallel worker queue
    let file_queue: std::sync::Mutex<Vec<(usize, PathBuf)>> =
        std::sync::Mutex::new(filtered_files.into_iter().enumerate().collect());

    let has_failure = Arc::new(AtomicBool::new(false));
    let suites_passed = Arc::new(AtomicUsize::new(0));
    let suites_failed = Arc::new(AtomicUsize::new(0));
    let total_assertions_passed = Arc::new(AtomicUsize::new(0));
    let total_assertions_failed = Arc::new(AtomicUsize::new(0));

    let results: Arc<std::sync::Mutex<Vec<TestResult>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(total_suites)));

    let mut handles = Vec::with_capacity(num_workers);

    // Process files with parallel thread pool
    let file_queue = Arc::new(file_queue);
    for _ in 0..num_workers {
        let queue = Arc::clone(&file_queue);
        let has_failure_flag = Arc::clone(&has_failure);
        let suites_passed_cnt = Arc::clone(&suites_passed);
        let suites_failed_cnt = Arc::clone(&suites_failed);
        let _assert_passed_cnt = Arc::clone(&total_assertions_passed);
        let _assert_failed_cnt = Arc::clone(&total_assertions_failed);
        let results_store = Arc::clone(&results);
        let fail_fast = args.fail_fast;
        let verbose = args.verbose;

        handles.push(std::thread::spawn(move || {
            loop {
                if fail_fast && has_failure_flag.load(Ordering::SeqCst) {
                    break;
                }

                let item = {
                    let mut guard = queue.lock().unwrap();
                    guard.pop()
                };

                let Some((_idx, path)) = item else {
                    break;
                };

                let display_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                let file_str = path.to_string_lossy().to_string();

                let t0 = Instant::now();
                let run_res = pipeline::run(&RunOpts {
                    file_path: file_str,
                    eval: None,
                    verbose: false,
                    no_run: false,
                    debug: Default::default(),
                    trace: false,
                    strict: false,
                });
                let elapsed = t0.elapsed();

                let mut passed = run_res.is_ok();
                let mut output = String::new();

                if let Err(ref e) = run_res {
                    output = format!("{e}");
                    passed = false;
                }

                // If output contains standard test summaries (PASSED: X, FAILED: Y)
                if passed {
                    suites_passed_cnt.fetch_add(1, Ordering::SeqCst);
                } else {
                    suites_failed_cnt.fetch_add(1, Ordering::SeqCst);
                    has_failure_flag.store(true, Ordering::SeqCst);
                }

                let res = TestResult {
                    display_name,
                    passed,
                    duration: elapsed,
                    output,
                };

                if verbose || !res.passed {
                    let icon = if res.passed {
                        "\x1b[32m✔ [PASSED]\x1b[0m"
                    } else {
                        "\x1b[31m✖ [FAILED]\x1b[0m"
                    };
                    println!(
                        "    {} {:<35} ({:.1?})",
                        icon, res.display_name, res.duration
                    );
                    if !res.passed && !res.output.is_empty() {
                        println!("      \x1b[31mError: {}\x1b[0m", res.output);
                    }
                } else {
                    println!(
                        "    \x1b[32m✔ [PASSED]\x1b[0m {:<35} ({:.1?})",
                        res.display_name, res.duration
                    );
                }

                results_store.lock().unwrap().push(res);
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    let total_elapsed = start_time.elapsed();
    let passed_count = suites_passed.load(Ordering::SeqCst);
    let failed_count = suites_failed.load(Ordering::SeqCst);

    println!("\n  \x1b[1m════════════════════════════════════════\x1b[0m");
    println!(
        "  \x1b[1mTest Suites:\x1b[0m \x1b[32m{} passed\x1b[0m, {} total",
        passed_count, total_suites
    );
    if failed_count > 0 {
        println!(
            "  \x1b[1mStatus:\x1b[0m      \x1b[31m{} failed\x1b[0m",
            failed_count
        );
    } else {
        println!("  \x1b[1mStatus:\x1b[0m      \x1b[32mALL TEST SUITES PASSED\x1b[0m");
    }
    println!("  \x1b[1mTotal Time:\x1b[0m  {:.2?}\n", total_elapsed);

    if failed_count > 0 {
        Err(CliError::fatal("Some test suites failed"))
    } else {
        Ok(())
    }
}

fn discover_test_files(target_path: Option<&str>) -> Result<Vec<PathBuf>, CliError> {
    let mut files = Vec::new();

    let root = match target_path {
        Some(p) => PathBuf::from(p),
        None => {
            if Path::new("./tests").is_dir() {
                PathBuf::from("./tests")
            } else {
                PathBuf::from(".")
            }
        }
    };

    if root.is_file() {
        files.push(root);
        return Ok(files);
    }

    if root.is_dir() {
        collect_vn_files_recursively(&root, &mut files)?;
        files.sort();
        return Ok(files);
    }

    Err(CliError::usage(format!(
        "Path '{}' does not exist",
        root.display()
    )))
}

fn collect_vn_files_recursively(dir: &Path, acc: &mut Vec<PathBuf>) -> Result<(), CliError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| CliError::fatal(format!("Failed to read directory {dir:?}: {e}")))?;

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            // Ignore hidden directories like .git, node_modules, target
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.')
                    || name == "target"
                    || name == "node_modules"
                    || name == "errors"
                {
                    continue;
                }
            }
            collect_vn_files_recursively(&p, acc)?;
        } else if p.is_file() {
            if let Some(ext) = p.extension() {
                if ext == "vn" {
                    // Filter out umbrella suite entry file (main.vn) and benchmark files when scanning directory
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        if stem == "main" || stem == "main-bench" {
                            continue;
                        }
                    }
                    acc.push(p);
                }
            }
        }
    }

    Ok(())
}
