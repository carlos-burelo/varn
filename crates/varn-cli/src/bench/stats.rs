//! Per-phase sample statistics.

use std::time::Duration;

/// Coefficient of variation above which a measurement is too noisy to compare
/// against another binary without a paired methodology.
pub const CV_NOISY: f64 = 0.05;

/// Above this, loose A/B comparison is meaningless.
pub const CV_UNRELIABLE: f64 = 0.10;

pub struct PhaseStats {
    pub name: &'static str,
    pub color_fn: fn(varn_term::chalk::Chalk) -> varn_term::chalk::Chalk,
    pub min: Duration,
    pub p50: Duration,
    pub max: Duration,
    pub total: Duration,
    pub stddev: Duration,
    pub runs: usize,
}

impl PhaseStats {
    pub fn from_samples(
        name: &'static str,
        color_fn: fn(varn_term::chalk::Chalk) -> varn_term::chalk::Chalk,
        samples: &[Duration],
    ) -> Self {
        let min = *samples.iter().min().expect("samples must not be empty");
        let max = *samples.iter().max().expect("samples must not be empty");
        let total: Duration = samples.iter().sum();
        let mean_ns = total.as_nanos() as f64 / samples.len() as f64;

        let mut sorted = samples.to_vec();
        sorted.sort();
        let p50 = sorted[sorted.len() / 2];

        let variance = samples
            .iter()
            .map(|s| {
                let d = s.as_nanos() as f64 - mean_ns;
                d * d
            })
            .sum::<f64>()
            / samples.len() as f64;
        let stddev = Duration::from_nanos(variance.sqrt() as u64);

        PhaseStats {
            name,
            color_fn,
            min,
            p50,
            max,
            total,
            stddev,
            runs: samples.len(),
        }
    }

    pub fn mean(&self) -> Duration {
        self.total / self.runs as u32
    }

    /// Coefficient of variation: σ divided by p50. The scale-free noise
    /// measure, so it is comparable across phases spanning microseconds to
    /// milliseconds.
    pub fn cv(&self) -> f64 {
        if self.p50.as_nanos() == 0 {
            return 0.0;
        }
        self.stddev.as_nanos() as f64 / self.p50.as_nanos() as f64
    }

    /// Spread as a fraction of the minimum — how far the worst run strayed.
    pub fn spread(&self) -> f64 {
        if self.min.as_nanos() == 0 {
            return 0.0;
        }
        (self.max.as_nanos() - self.min.as_nanos()) as f64 / self.min.as_nanos() as f64
    }
}