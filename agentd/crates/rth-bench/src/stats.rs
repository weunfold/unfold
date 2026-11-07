

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub n: usize,
    pub unit: String,
    pub method: String,
    pub min: Option<f64>,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
}

pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    Some(sorted[rank.min(sorted.len()) - 1])
}

pub fn summarize(samples: &[f64], unit: &str) -> Summary {
    let mut s: Vec<f64> = samples.iter().copied().filter(|x| x.is_finite()).collect();
    s.sort_by(|a, b| a.total_cmp(b));
    let round = |x: f64| (x * 1000.0).round() / 1000.0;
    Summary {
        n: s.len(),
        unit: unit.to_string(),
        method: "nearest-rank".to_string(),
        min: s.first().copied().map(round),
        p50: percentile(&s, 50.0).map(round),
        p95: percentile(&s, 95.0).map(round),
        max: s.last().copied().map(round),
        mean: (!s.is_empty()).then(|| round(s.iter().sum::<f64>() / s.len() as f64)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_on_thirty_samples() {
        let s: Vec<f64> = (1..=30).map(f64::from).collect();
        assert_eq!(percentile(&s, 50.0), Some(15.0));

        assert_eq!(percentile(&s, 95.0), Some(29.0));
        assert_eq!(percentile(&[], 95.0), None);
        let sum = summarize(&[3.0, 1.0, 2.0], "ms");
        assert_eq!((sum.n, sum.p50, sum.max), (3, Some(2.0), Some(3.0)));
    }
}
