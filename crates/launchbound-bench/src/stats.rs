//! Interval statistics (docs/BENCHMARKING.md): a benchmark that reports a mean
//! and no interval is not evidence. Median with a distribution-free 95% CI
//! (order statistics), Tukey-fence outlier rejection, and an overlap test —
//! configurations whose intervals overlap are indistinguishable, never
//! ranked.

use serde::{Deserialize, Serialize};

/// A candidate's timings, reduced to what a decision needs.
///
/// The interval is the point. Two configurations whose 95% CIs overlap are
/// reported indistinguishable and never ranked against each other
/// (`docs/BENCHMARKING.md`), because a tool that puts a winner's name on
/// measurement noise is worse than one that says it cannot tell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    /// Samples kept after outlier rejection.
    pub n: usize,
    /// Samples dropped by the Tukey fences, NaNs among them.
    pub outliers_rejected: usize,
    /// Median of the kept samples — the statistic everything ranks on,
    /// chosen over the mean because a single descheduled launch should not
    /// move it.
    pub median_ms: f64,
    /// Distribution-free 95% CI on the median (order statistics).
    pub ci95_lo_ms: f64,
    /// Upper bound of that interval.
    pub ci95_hi_ms: f64,
    /// Fastest kept sample.
    pub min_ms: f64,
    /// Slowest kept sample.
    pub max_ms: f64,
    /// Mean of the kept samples. Reported for context; nothing ranks on it.
    pub mean_ms: f64,
}

/// Summarize raw timings. The Tukey fences (1.5 IQR) run first; the median
/// CI uses the normal approximation to the binomial order-statistic
/// interval, clamped to the sample range.
///
/// # NaN
///
/// A NaN timing cannot be ordered against anything, so the sort uses
/// [`f64::total_cmp`], which is total: `-NaN` sorts below `-inf` and `+NaN`
/// above `+inf`. A NaN then fails both fence comparisons and is *rejected as
/// an outlier*, counted in `outliers_rejected`. If enough of them poison the
/// quantiles that nothing survives the fences, the result is `None` — which
/// is the honest summary of a sample that has none. Nothing here panics on a
/// NaN, and no NaN reaches `median_ms`.
pub fn summarize(samples_ms: &[f64]) -> Option<Summary> {
    if samples_ms.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = samples_ms.to_vec();
    sorted.sort_by(f64::total_cmp);

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

    // A NaN timing used to be an `expect("no NaN timings")` away from taking
    // the process down. `total_cmp` orders it, the Tukey fences reject it, and
    // `summarize` keeps its contract: a value or `None`, never a panic.
    #[test]
    fn a_nan_timing_is_rejected_as_an_outlier_and_never_panics() {
        let mut samples: Vec<f64> = (0..50).map(|i| 1.0 + (i % 7) as f64 * 0.001).collect();
        samples.push(f64::NAN);
        let s = summarize(&samples).expect("a summary, not a panic");
        assert!(
            s.outliers_rejected >= 1,
            "the NaN must not survive the fences"
        );
        assert!(
            s.median_ms.is_finite(),
            "median {} is not finite",
            s.median_ms
        );
        assert!(s.ci95_lo_ms.is_finite() && s.ci95_hi_ms.is_finite());
        assert!(s.ci95_lo_ms <= s.median_ms && s.median_ms <= s.ci95_hi_ms);
    }

    // All-NaN is the honest `None`, not a crash and not a fabricated number.
    #[test]
    fn an_all_nan_sample_summarizes_to_none() {
        assert!(summarize(&[f64::NAN; 8]).is_none());
    }

    // Both signs, and the infinities, since `total_cmp` treats them as
    // distinct ends of the order.
    #[test]
    fn every_non_finite_shape_is_survivable() {
        for probe in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut samples: Vec<f64> = (0..30).map(|i| 1.0 + (i % 5) as f64 * 0.001).collect();
            samples.push(probe);
            // The only requirement is that it returns.
            let _ = summarize(&samples);
        }
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
