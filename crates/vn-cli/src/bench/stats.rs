use std::time::Duration;

pub const CV_UNRELIABLE: f64 = 0.10;

const T_CRITICAL_95: [f64; 30] = [
    12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
    2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
    2.052, 2.048, 2.045, 2.042,
];

pub struct PhaseStats {
    pub name: &'static str,
    pub min: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub total: Duration,
    pub stddev: Duration,
    pub ci95: Duration,
    pub outliers: usize,
    pub runs: usize,
    pub samples: Vec<Duration>,
}

fn quantile(sorted: &[Duration], q: f64) -> f64 {
    let position = q * (sorted.len() - 1) as f64;
    let below = position.floor() as usize;
    let above = position.ceil() as usize;
    let low = sorted[below].as_nanos() as f64;
    let high = sorted[above].as_nanos() as f64;
    low + (high - low) * (position - below as f64)
}

impl PhaseStats {
    pub fn from_samples(name: &'static str, samples: &[Duration]) -> Self {
        let mut sorted = samples.to_vec();
        sorted.sort();
        let count = samples.len();
        let min = *sorted.first().expect("samples must not be empty");
        let max = *sorted.last().expect("samples must not be empty");
        let total: Duration = samples.iter().sum();
        let mean_ns = total.as_nanos() as f64 / count as f64;

        let spread = samples
            .iter()
            .map(|s| {
                let d = s.as_nanos() as f64 - mean_ns;
                d * d
            })
            .sum::<f64>();
        let stddev_ns = if count > 1 {
            (spread / (count - 1) as f64).sqrt()
        } else {
            0.0
        };
        let t = T_CRITICAL_95
            .get(count.wrapping_sub(2))
            .copied()
            .unwrap_or(1.96);
        let ci_ns = if count > 1 {
            t * stddev_ns / (count as f64).sqrt()
        } else {
            0.0
        };

        let (q1, q3) = (quantile(&sorted, 0.25), quantile(&sorted, 0.75));
        let fence = 1.5 * (q3 - q1);
        let outliers = sorted
            .iter()
            .filter(|s| {
                let ns = s.as_nanos() as f64;
                ns < q1 - fence || ns > q3 + fence
            })
            .count();

        PhaseStats {
            name,
            min,
            p50: sorted[count / 2],
            p95: Duration::from_nanos(quantile(&sorted, 0.95) as u64),
            max,
            total,
            stddev: Duration::from_nanos(stddev_ns as u64),
            ci95: Duration::from_nanos(ci_ns as u64),
            outliers,
            runs: count,
            samples: samples.to_vec(),
        }
    }

    pub fn mean(&self) -> Duration {
        self.total / self.runs as u32
    }

    pub fn cv(&self) -> f64 {
        if self.p50.as_nanos() == 0 {
            return 0.0;
        }
        self.stddev.as_nanos() as f64 / self.p50.as_nanos() as f64
    }

    pub fn ci_ratio(&self) -> f64 {
        let mean = self.mean().as_nanos();
        if mean == 0 {
            return 0.0;
        }
        self.ci95.as_nanos() as f64 / mean as f64
    }
}
