//! `vn run --compare-tiers` — run one program on every execution tier and say
//! where they stop agreeing.
//!
//! A Varn program must produce identical output on every tier. When it does
//! not, the bug is in code generation or the JIT — the class of defect
//! `cargo test` structurally cannot see, because the two tiers compile the same
//! source and only one of them is wrong.
//!
//! Finding *which file* diverges is a loop around `vn run`. What costs the
//! hours is finding *where inside it*, and that is what this reports: the first
//! output line the tiers disagree on, with the lines that led to it.
//!
//! Each tier runs as its own process. Sharing one is not worth the risk: the
//! JIT publishes compiled entries into protos, so a second run in the same
//! process is no longer a clean interpreter run.

use std::path::Path;
use std::process::Command;

use varn_core::term::chalk::chalk;

use crate::error::CliError;

/// One execution tier, named by the environment that selects it.
struct Tier {
    label: &'static str,
    env: &'static [(&'static str, &'static str)],
}

/// The baseline is the interpreter: it is the definition of what the program
/// means, so a disagreement is always the other tier being wrong.
const TIERS: &[Tier] = &[
    Tier {
        label: "interpreter",
        env: &[("VARN_NO_CLIF", "1")],
    },
    Tier {
        label: "jit",
        env: &[],
    },
    Tier {
        label: "jit (tier 1)",
        env: &[("VARN_JIT_TIER", "1")],
    },
];

struct Outcome {
    label: &'static str,
    status: String,
    output: String,
}

/// Run `file` on every tier and report the first disagreement. Returns the
/// number of tiers that diverged from the interpreter, so a caller can use it
/// as an exit code.
pub fn execute(file: &str, script_args: &[String]) -> Result<(), CliError> {
    if !Path::new(file).exists() {
        return Err(CliError::usage(format!("no such file: {file}")));
    }
    let exe = std::env::current_exe()
        .map_err(|e| CliError::fatal(format!("cannot locate the running vn binary: {e}")))?;

    println!(
        "\n{} {}",
        chalk("COMPARE TIERS").bold().blue(),
        chalk(file).dim()
    );

    let mut outcomes = Vec::with_capacity(TIERS.len());
    for tier in TIERS {
        let mut cmd = Command::new(&exe);
        cmd.arg("run").arg(file);
        if !script_args.is_empty() {
            cmd.arg("--").args(script_args);
        }
        // A tier is selected by environment, so the child must not inherit a
        // selection the parent happens to be running under.
        cmd.env_remove("VARN_NO_CLIF").env_remove("VARN_JIT_TIER");
        for (k, v) in tier.env {
            cmd.env(k, v);
        }
        let out = cmd
            .output()
            .map_err(|e| CliError::fatal(format!("could not run the {} tier: {e}", tier.label)))?;

        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        outcomes.push(Outcome {
            label: tier.label,
            status: describe_status(&out.status),
            output: text,
        });
    }

    let (baseline, rest) = outcomes.split_first().expect("TIERS is never empty");
    println!(
        "  {:<14} {}  {}",
        baseline.label,
        chalk(&baseline.status).dim(),
        chalk("baseline").dim()
    );

    let mut diverged = 0usize;
    for tier in rest {
        let same_output = tier.output == baseline.output;
        let same_status = tier.status == baseline.status;
        if same_output && same_status {
            println!(
                "  {:<14} {}  {}",
                tier.label,
                chalk(&tier.status).dim(),
                chalk("identical").green()
            );
            continue;
        }
        diverged += 1;
        println!(
            "  {:<14} {}  {}",
            tier.label,
            chalk(&tier.status).dim(),
            chalk("DIVERGES").red().bold()
        );
        if !same_status {
            println!(
                "      {} {} against {}",
                chalk("exit").dim(),
                tier.status,
                baseline.status
            );
        }
        if !same_output {
            report_first_difference(baseline, tier);
        }
    }

    println!();
    if diverged == 0 {
        println!("  {}", chalk("every tier agrees").green().bold());
        return Ok(());
    }
    Err(CliError::fatal(format!(
        "{diverged} tier(s) diverge from the interpreter"
    )))
}

/// Print the first line the two tiers disagree on, with the lines that led up
/// to it. The lines before the split are what both tiers still agreed on, which
/// is usually what names the function to look at.
fn report_first_difference(baseline: &Outcome, tier: &Outcome) {
    const CONTEXT: usize = 3;
    let base: Vec<&str> = baseline.output.lines().collect();
    let other: Vec<&str> = tier.output.lines().collect();
    let split = base
        .iter()
        .zip(&other)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| base.len().min(other.len()));

    let from = split.saturating_sub(CONTEXT);
    for (i, line) in base.iter().enumerate().take(split).skip(from) {
        println!("      {} {line}", chalk(format!("{:>5}", i + 1)).dim());
    }
    println!(
        "      {} {}",
        chalk(format!("{:>5}", split + 1)).dim(),
        chalk(format!("- {}", base.get(split).copied().unwrap_or("<no more output>")))
            .green()
    );
    println!(
        "      {} {}",
        chalk("     ").dim(),
        chalk(format!("+ {}", other.get(split).copied().unwrap_or("<no more output>"))).red()
    );
    let extra = other.len().saturating_sub(split + 1);
    if extra > 0 {
        println!("      {}", chalk(format!("… and {extra} more line(s)")).dim());
    }
}

fn describe_status(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(0) => "ok".to_owned(),
        Some(c) => format!("exit {c}"),
        // A tier killed by a signal is the strongest divergence there is.
        None => "killed".to_owned(),
    }
}
