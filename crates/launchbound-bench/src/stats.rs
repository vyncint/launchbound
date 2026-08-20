//! Interval statistics (docs/BENCHMARKING.md): a benchmark that reports a mean
//! and no interval is not evidence. Median with a distribution-free 95% CI
//! (order statistics), Tukey-fence outlier rejection, and an overlap test —
//! configurations whose intervals overlap are indistinguishable, never
//! ranked.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    /// Samples kept after outlier rejection.
    pub n: usize,
    pub outliers_rejected: usize,
    pub median_ms: f64,
    /// Distribution-free 95% CI on the median (order statistics).
    pub ci95_lo_ms: f64,
    pub ci95_hi_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
}

/// Summarize raw timings. The Tukey fences (1.5 IQR) run first; the median
/// CI uses the normal approximation to the binomial order-statistic
/// interval, clamped to the sample range.
pub fn summarize(samples_ms: &[f64]) -> Option<Summary> {
    if samples_ms.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN timings"));

    let q1 = quantile(&sorted, 0.25);
    let q3 = quantile(&sorted, 0.75);
    let iqr = q3 - q1;
    let (lo_fence, hi_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let kept: Vec<f64> = sorted
        .iter()
        .copied()
        .filter(|&x| x >= lo_fence && x <= hi_fence)
        .collect();
    let outliers_rejected = sorted.len() - kept.len();
    let n = kept.len();
    if n == 0 {
        return None;
    }

    let median = quantile(&kept, 0.5);
    // Order-statistic 95% CI for the median: ranks n/2 ± 1.96*sqrt(n)/2.
    let half_width = 1.96 * (n as f64).sqrt() / 2.0;
    let lo_rank = ((n as f64) / 2.0 - half_width).floor().max(0.0) as usize;
    let hi_rank = (((n as f64) / 2.0 + half_width).ceil() as usize).min(n - 1);
    let mean = kept.iter().sum::<f64>() / n as f64;

    Some(Summary {
        n,
        outliers_rejected,
        median_ms: median,
        ci95_lo_ms: kept[lo_rank],
        ci95_hi_ms: kept[hi_rank],
        min_ms: kept[0],
        max_ms: kept[n - 1],
        mean_ms: mean,
    })
}

/// Linear-interpolated quantile of a sorted slice.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q * (sorted.len() - 1) as f64;
    let base = pos.floor() as usize;
    let frac = pos - base as f64;
    if base + 1 < sorted.len() {
        sorted[base] * (1.0 - frac) + sorted[base + 1] * frac
    } else {
        sorted[base]
    }
}

/// Two summaries whose 95% CIs overlap are indistinguishable (docs/BENCHMARKING.md).
pub fn indistinguishable(a: &Summary, b: &Summary) -> bool {
    a.ci95_lo_ms <= b.ci95_hi_ms && b.ci95_lo_ms <= a.ci95_hi_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_and_rejects_outliers() {
        let mut samples: Vec<f64> = (0..100).map(|i| 1.0 + (i % 7) as f64 * 0.001).collect();
        samples.push(50.0); // gross outlier
        let s = summarize(&samples).unwrap();
        assert_eq!(s.outliers_rejected, 1);
        assert!(s.median_ms > 0.99 && s.median_ms < 1.01);
        assert!(s.ci95_lo_ms <= s.median_ms && s.median_ms <= s.ci95_hi_ms);
    }

    #[test]
    fn overlap_means_indistinguishable() {
        let a = summarize(&[1.0, 1.01, 1.02, 0.99, 1.0]).unwrap();
        let b = summarize(&[1.01, 1.02, 1.03, 1.0, 1.01]).unwrap();
        assert!(indistinguishable(&a, &b));
        let c = summarize(&[2.0, 2.01, 2.02, 1.99, 2.0]).unwrap();
        assert!(!indistinguishable(&a, &c));
    }

    #[test]
    fn empty_input_is_none_not_zero() {
        assert!(summarize(&[]).is_none());
    }
}
