

use realfft::RealFftPlanner;
pub const SAMPLES: usize = 128_000;
const FFT: usize = 400;
const BINS: usize = FFT / 2 + 1;
pub fn prepare(audio: &[f32]) -> Vec<f32> {
    let audio = &audio[audio.len().saturating_sub(SAMPLES)..];
    let mut padded = vec![0.0; SAMPLES];
    padded[SAMPLES - audio.len()..].copy_from_slice(audio);
    let mean = padded.iter().map(|x| f64::from(*x)).sum::<f64>() / SAMPLES as f64;
    let variance = padded
        .iter()
        .map(|x| (f64::from(*x) - mean).powi(2))
        .sum::<f64>()
        / SAMPLES as f64;
    let scale = (variance + 1e-7).sqrt();
    for x in &mut padded {
        *x = ((f64::from(*x) - mean) / scale) as f32;
    }
    padded
}
fn mel(hz: f64) -> f64 {
    if hz < 1000.0 {
        hz / (200.0 / 3.0)
    } else {
        15.0 + (hz / 1000.0).ln() / (6.4_f64.ln() / 27.0)
    }
}
fn hz(m: f64) -> f64 {
    if m < 15.0 {
        m * (200.0 / 3.0)
    } else {
        1000.0 * ((m - 15.0) * (6.4_f64.ln() / 27.0)).exp()
    }
}
pub fn extract(audio: &[f32]) -> Result<Vec<f32>, String> {
    if audio.iter().any(|x| !x.is_finite()) {
        return Err("Non-finite turn audio".into());
    }
    let audio = prepare(audio);
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let window: Vec<_> = (0..FFT)
        .map(|i| 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / FFT as f32).cos()))
        .collect();
    let edges: Vec<_> = (0..82).map(|i| hz(mel(8000.0) * i as f64 / 81.0)).collect();
    let mut filters = vec![0.0_f32; 80 * BINS];
    for m in 0..80 {
        for k in 0..BINS {
            let freq = k as f64 * 16000.0 / FFT as f64;
            filters[m * BINS + k] = (((freq - edges[m]) / (edges[m + 1] - edges[m]))
                .min((edges[m + 2] - freq) / (edges[m + 2] - edges[m + 1]))
                .max(0.0)
                * 2.0
                / (edges[m + 2] - edges[m])) as f32;
        }
    }
    let mut features = vec![0.0_f32; 80 * 800];
    for t in 0..800 {
        for i in 0..FFT {
            let j = t as isize * 160 + i as isize - 200;
            let j = if j < 0 {
                -j
            } else if j >= SAMPLES as isize {
                2 * SAMPLES as isize - 2 - j
            } else {
                j
            };
            input[i] = audio[j as usize] * window[i];
        }
        fft.process_with_scratch(&mut input, &mut output, &mut scratch)
            .map_err(|e| e.to_string())?;
        for m in 0..80 {
            let power = output
                .iter()
                .zip(&filters[m * BINS..(m + 1) * BINS])
                .map(|(c, w)| c.norm_sqr() * w)
                .sum::<f32>();
            features[m * 800 + t] = power.max(1e-10).log10();
        }
    }
    let max = features.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    for x in &mut features {
        *x = (x.max(max - 8.0) + 4.0) / 4.0;
    }
    Ok(features)
}
#[cfg(test)]
mod tests {
    #[test]
    fn silence_is_finite_and_has_expected_whisper_shape() {
        let f = super::extract(&[]).unwrap();
        assert_eq!(f.len(), 80 * 800);
        assert!(f.iter().all(|x| (*x + 1.5).abs() < 1e-6));
    }
    #[test]
    fn only_the_last_eight_seconds_affect_features() {
        let mut a = vec![0.25; super::SAMPLES + 100];
        a[100..].fill(0.0);
        assert_eq!(super::prepare(&a), super::prepare(&a[100..]));
        assert!(super::extract(&[f32::NAN]).is_err());
    }
}

#[cfg(test)]
mod reference_test {
    #[test]
    fn matches_official_whisper_feature_extractor() {
        let audio: Vec<f32> = (0..19200)
            .map(|i| {
                (0.2 * (i as f64 * std::f64::consts::TAU * 217.0 / 16000.0).sin()
                    + 0.05 * (i as f64 * std::f64::consts::TAU * 643.0 / 16000.0).cos())
                    as f32
            })
            .collect();
        let features = super::extract(&audio).unwrap();
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../testdata/turn-reference.json"
        ))
        .unwrap();
        let mut max_diff = 0.0_f64;
        for bin in reference["bins"].as_array().unwrap() {
            let i = bin[0].as_u64().unwrap() as usize;
            let difference = (f64::from(features[i]) - bin[1].as_f64().unwrap()).abs();
            max_diff = max_diff.max(difference);
        }
        assert!(
            max_diff < 0.002,
            "Whisper feature max difference {max_diff}"
        );
    }
}
