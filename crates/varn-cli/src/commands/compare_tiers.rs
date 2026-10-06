use std::path::Path;
use std::process::Command;

use varn_core::term::chalk::chalk;

use crate::error::CliError;

struct Tier {
    label: &'static str,
    env: &'static [(&'static str, &'static str)],
}

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
        chalk(format!(
            "- {}",
            base.get(split).copied().unwrap_or("<no more output>")
        ))
        .green()
    );
    println!(
        "      {} {}",
        chalk("     ").dim(),
        chalk(format!(
            "+ {}",
            other.get(split).copied().unwrap_or("<no more output>")
        ))
        .red()
    );
    let extra = other.len().saturating_sub(split + 1);
    if extra > 0 {
        println!(
            "      {}",
            chalk(format!("… and {extra} more line(s)")).dim()
        );
    }
}

fn describe_status(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(0) => "ok".to_owned(),
        Some(c) => format!("exit {c}"),

        None => "killed".to_owned(),
    }
}
