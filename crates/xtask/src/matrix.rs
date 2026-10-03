use super::args::Opts;
use super::report::{JsonBenchRow, JsonReport};
use super::stats::{RowResult, RuntimeInfo};
use super::term::Term;
use super::verdict::{get_verdict_badge, get_verdict_badge_len, get_verdict_text};

pub(super) fn print_results(
    term: &Term,
    opts: &Opts,
    runtimes: &[RuntimeInfo],
    results: &[RowResult],
    cpu: &str,
    rt_names: &[String],
    startup_stats: &std::collections::HashMap<String, super::stats::SampleStats>,
) -> Result<(), Box<dyn std::error::Error>> {
    if opts.json {
        let json_report = JsonReport {
            cpu: cpu.to_string(),
            runtimes: rt_names.to_vec(),
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
        print_markdown(runtimes, results);
    } else if opts.compact {
        print_compact(term, results);
    } else {
        print_cards(term, runtimes, results);
    }
    Ok(())
}

fn print_markdown(runtimes: &[RuntimeInfo], results: &[RowResult]) {
    let mut hdr = String::from("| Benchmark");
    for rt in runtimes {
        hdr.push_str(&format!(" | {} work", rt.name));
    }
    hdr.push_str(" | verdict (work) |");
    println!("{hdr}");

    let mut sep = String::from("|---");
    for _ in runtimes {
        sep.push_str("|---");
    }
    sep.push_str("|---|");
    println!("{sep}");

    for r in results {
        let mut line = format!("| {}", r.bench_name);
        for rt in runtimes {
            if let Some(w) = r.work_stats.get(&rt.name) {
                line.push_str(&format!(" | {:.1} ms", w.median));
            } else {
                line.push_str(" | --");
            }
        }
        line.push_str(&format!(" | {} |", get_verdict_text(r)));
        println!("{line}");
    }
}

fn print_compact(term: &Term, results: &[RowResult]) {
    let hdr = format!(
        "  {:<20} {:>10} {:>10} {:>10}   {:<22}   {}",
        "Benchmark", "Varn", "Bun", "Node", "Relative (Varn vs Bun)", "Verdict"
    );
    println!("{}", term.bold_cyan(&hdr));
    println!("  {}", term.gray(&"─".repeat(hdr.len() - 2)));

    for r in results {
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

        let badge = get_verdict_badge(r, term);
        println!(
            "  {:<20} {} {} {}   {}   {}",
            r.bench_name, v_str, b_str, n_str, rel_bar, badge
        );
    }
}

fn print_cards(term: &Term, runtimes: &[RuntimeInfo], results: &[RowResult]) {
    let card_width: usize = 72;
    for r in results {
        let badge = get_verdict_badge(r, term);
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

        for rt in runtimes {
            if let Some(wrk) = r.work_stats.get(&rt.name) {
                let (cols, tag) = if wrk.median > max_normal * 3.5 && rt.name == "python" {
                    (
                        bar_max_cols,
                        format!(" {}", term.gray(&format!("[+{:.0}ms]", wrk.median))),
                    )
                } else {
                    let c = ((wrk.median / max_normal) * (bar_max_cols as f64)).round() as usize;
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
