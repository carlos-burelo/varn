use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(super) struct RuntimeInfo {
    pub(super) name: String,
    pub(super) bin: PathBuf,
    pub(super) args_prefix: Vec<String>,
    pub(super) empty_ext: &'static str,
    pub(super) empty_body: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct SampleStats {
    pub(super) count: usize,
    pub(super) median: f64,
    pub(super) min: f64,
    pub(super) max: f64,
    pub(super) mean: f64,
    pub(super) stddev: f64,
    pub(super) p95: f64,
}

impl SampleStats {
    pub(super) fn compute(samples: &[f64]) -> Self {
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

pub(super) fn round1(val: f64) -> f64 {
    (val * 10.0).round() / 10.0
}

pub(super) fn round2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

pub(super) struct RunResult {
    pub(super) ms: f64,
    pub(super) signature: String,
    pub(super) output: String,
}

pub(super) struct RowResult {
    pub(super) bench_name: String,
    pub(super) output_ok: bool,
    pub(super) outputs_by_rt: HashMap<String, String>,
    pub(super) total_stats: HashMap<String, SampleStats>,
    pub(super) work_stats: HashMap<String, SampleStats>,
    pub(super) best_rival: Option<String>,
    pub(super) rival_work: Option<f64>,
    pub(super) work_ratio: Option<f64>,
    pub(super) resolved: bool,
}

pub(super) fn get_result_signature(raw: &str) -> String {
    let re_timing = Regex::new(r"(?i)elapsed|took|\btime\b").unwrap();
    let re_ms = Regex::new(r"(?i)[a-z_]*ms\s*=\s*-?[\d.]+\b").unwrap();
    let re_bytes = Regex::new(r"(?i)bytes\s*[=:]\s*-?[\d.]+\b|-?[\d.]+\s*bytes\b").unwrap();
    let mut nums = Vec::new();

    for line in raw.lines() {
        if re_timing.is_match(line) {
            continue;
        }
        let no_ms = re_ms.replace_all(line, " ").to_string();
        let cleaned = re_bytes.replace_all(&no_ms, " ").to_string();
        if cleaned.trim().is_empty() {
            continue;
        }
        let bytes = cleaned.as_bytes();
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
                    nums.push(cleaned[start..i].to_string());
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
