use super::args::Opts;
use super::benches::BenchDef;
use super::run::invoke_once;
use super::stats::round2;
use super::stats::{RowResult, RuntimeInfo, SampleStats};
use super::term::Term;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(super) fn calibrate_runtimes(
    term: &Term,
    opts: &Opts,
    runtimes: &[RuntimeInfo],
    temp_root: &Path,
    cache_dirs: &HashMap<String, PathBuf>,
) -> Result<HashMap<String, SampleStats>, Box<dyn std::error::Error>> {
    let startup_dir = temp_root.join("startup");
    std::fs::create_dir_all(&startup_dir)?;

    let mut startup_files = HashMap::new();
    for rt in runtimes {
        let probe = startup_dir.join(format!("startup_{}{}", rt.name, rt.empty_ext));
        std::fs::write(&probe, rt.empty_body)?;
        startup_files.insert(rt.name.clone(), probe);
    }

    let mut startup_samples: HashMap<String, Vec<f64>> = HashMap::new();
    for rt in runtimes {
        startup_samples.insert(rt.name.clone(), Vec::with_capacity(opts.runs));
    }

    for i in 0..opts.runs {
        for rt in runtimes {
            let file = &startup_files[&rt.name];
            let cdir = &cache_dirs[&rt.name];
            let res = invoke_once(rt, file, cdir)?;
            if i >= opts.warmup {
                startup_samples.get_mut(&rt.name).unwrap().push(res.ms);
            }
        }
    }

    let mut startup_stats: HashMap<String, SampleStats> = HashMap::new();
    for rt in runtimes {
        let stats = SampleStats::compute(&startup_samples[&rt.name]);
        startup_stats.insert(rt.name.clone(), stats);
    }

    if !opts.json {
        println!(
            "  {}",
            term.bold_cyan("🚀 Startup Latency (empty program):")
        );
        let max_su = startup_stats
            .values()
            .map(|s| s.median)
            .fold(0.0f64, f64::max);

        for rt in runtimes {
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

    Ok(startup_stats)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn measure_benchmarks(
    opts: &Opts,
    runtimes: &[RuntimeInfo],
    benchmarks: &[&BenchDef],
    bench_dir: &Path,
    cache_dirs: &HashMap<String, PathBuf>,
    startup_stats: &HashMap<String, SampleStats>,
) -> Result<Vec<RowResult>, Box<dyn std::error::Error>> {
    let mut results: Vec<RowResult> = Vec::new();

    for bench in benchmarks {
        if !opts.json {
            print!("\r  ⏳ Benchmarking: {:<20} ...", bench.name);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }

        let mut bench_files: HashMap<String, PathBuf> = HashMap::new();
        for rt in runtimes {
            let rel = match rt.name.as_str() {
                "varn" | "varn-base" => Some(bench.vn),
                "python" => bench.py,
                _ => Some(bench.ts),
            };
            if let Some(r) = rel {
                let full = match rt.name.as_str() {
                    "varn" | "varn-base" => bench_dir.join("vn").join(r),
                    "python" => bench_dir.join(r),
                    _ => bench_dir.join("ts").join(r),
                };
                if full.is_file() {
                    bench_files.insert(rt.name.clone(), full);
                }
            }
        }

        let present_rts: Vec<&RuntimeInfo> = runtimes
            .iter()
            .filter(|r| bench_files.contains_key(&r.name))
            .collect();

        if present_rts.is_empty() {
            results.push(RowResult {
                bench_name: bench.name.to_string(),
                output_ok: false,
                outputs_by_rt: HashMap::new(),
                total_stats: HashMap::new(),
                work_stats: HashMap::new(),
                best_rival: None,
                rival_work: None,
                work_ratio: None,
                resolved: false,
            });
            continue;
        }

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
        let mut empty_sig_rt = Vec::new();
        for rt in &present_rts {
            if let Some(sig) = last_sigs.get(&rt.name) {
                if !sig.is_empty() {
                    unique_sigs.insert(sig.clone());
                } else {
                    empty_sig_rt.push(rt.name.clone());
                }
            }
        }
        let output_ok = unique_sigs.len() <= 1 && empty_sig_rt.is_empty();

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
            (Some(vw), Some(rw)) if vw >= 0.05 && rw >= 0.05 => Some(round2(rw / vw)),
            _ => None,
        };

        let resolved = if let (Some(rw_name), Some(_rw)) = (&best_rival, best_rival_work) {
            match (work_stats.get("varn"), work_stats.get(rw_name)) {
                (Some(varn_w), Some(rival_w)) => {
                    let non_overlapping = (varn_w.max < rival_w.min) || (rival_w.max < varn_w.min);
                    let significant_diff =
                        (varn_w.median - rival_w.median).abs() / rival_w.median.max(0.1) > 0.05;
                    non_overlapping || significant_diff
                }
                _ => false,
            }
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

    Ok(results)
}
