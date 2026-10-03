use crate::cli::TestArgs;
use crate::error::CliError;
use crate::pipeline;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use varn_core::term::{chalk, log, Section};
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

    Section::new("varn test")
        .subtitle(format!(
            "{total_suites} suite{} · {num_workers} worker{}",
            if total_suites == 1 { "" } else { "s" },
            if num_workers == 1 { "" } else { "s" }
        ))
        .color(|c| c.cyan())
        .print();

    let file_queue: Mutex<Vec<(usize, PathBuf)>> =
        Mutex::new(filtered_files.into_iter().enumerate().collect());
    let file_queue = Arc::new(file_queue);

    let has_failure = Arc::new(AtomicBool::new(false));
    let suites_passed = Arc::new(AtomicUsize::new(0));
    let suites_failed = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<Vec<TestResult>>> =
        Arc::new(Mutex::new(Vec::with_capacity(total_suites)));
    let print_lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));

    let quiet = !args.verbose;
    if quiet {
        varn_builtins::set_print_silent(true);
        varn_builtins::set_testing_silent(true);
    }

    let mut handles = Vec::with_capacity(num_workers);

    for _ in 0..num_workers {
        let queue = Arc::clone(&file_queue);
        let has_failure_flag = Arc::clone(&has_failure);
        let suites_passed_cnt = Arc::clone(&suites_passed);
        let suites_failed_cnt = Arc::clone(&suites_failed);
        let results_store = Arc::clone(&results);
        let out_lock = Arc::clone(&print_lock);
        let fail_fast = args.fail_fast;
        let verbose = args.verbose;

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
                display_name: display_name.clone(),
                passed,
                duration: elapsed,
                output: output.clone(),
            });

            let ms = elapsed.as_millis();
            let _guard = out_lock.lock().unwrap();
            if passed {
                log(format!(
                    "  {} {} {}",
                    chalk("ok").green(),
                    display_name,
                    chalk(format_args!("({ms}ms)")).dim()
                ));
            } else {
                log(format!(
                    "  {} {} {}",
                    chalk("FAILED").red(),
                    display_name,
                    chalk(format_args!("({ms}ms)")).dim()
                ));
                if verbose && !output.is_empty() {
                    for line in output.lines() {
                        log(format!("    {}", chalk(line).dim()));
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    if quiet {
        varn_builtins::set_print_silent(false);
        varn_builtins::set_testing_silent(false);
    }

    let total_elapsed = start_time.elapsed();
    let passed_count = suites_passed.load(Ordering::SeqCst);
    let failed_count = suites_failed.load(Ordering::SeqCst);

    let mut all_results = results.lock().unwrap().clone();
    all_results.sort_by_key(|r| r.idx);

    if failed_count == 0 {
        Section::new("test result")
            .subtitle(format!(
                "{passed_count} passed · 0 failed · {total_suites} suites · {total_elapsed:.2?}"
            ))
            .color(|c| c.green())
            .print();
    } else {
        Section::new("test result")
            .subtitle(format!(
                "{passed_count} passed · {failed_count} failed · {total_suites} suites · {total_elapsed:.2?}"
            ))
            .color(|c| c.red())
            .print();
        for r in all_results.iter().filter(|r| !r.passed) {
            log(format!(
                "  {} {} {}",
                chalk("✖").red(),
                r.display_name,
                chalk(format_args!("({:.1?})", r.duration)).dim()
            ));
            if !r.output.is_empty() {
                let first = r.output.lines().next().unwrap_or(&r.output);
                log(format!("    {}", chalk(first).dim()));
            }
        }
    }

    if failed_count > 0 {
        Err(CliError::fatal("Some test suites failed"))
    } else {
        Ok(())
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
