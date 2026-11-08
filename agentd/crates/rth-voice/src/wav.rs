

use std::path::Path;

pub struct WavData {
    pub sample_rate: u32,
    pub channels: u16,

    pub mono: Vec<f32>,
}

impl WavData {
    pub fn seconds(&self) -> f64 {
        self.mono.len() as f64 / self.sample_rate.max(1) as f64
    }

    pub fn to_i16_at(&self, rate: u32) -> Vec<i16> {
        let mut r = crate::resample::Resampler::new(self.sample_rate, rate);
        let mut out = Vec::new();
        r.process(&self.mono, &mut out);
        out.into_iter().map(|v| (v * 32767.0).round().clamp(-32768.0, 32767.0) as i16).collect()
    }
}

pub fn read(path: &Path) -> Result<WavData, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(b: &[u8]) -> Result<WavData, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut i = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4) as usize;
        let body = i + 8;
        let end = (body + len).min(b.len());
        if id == b"fmt " && len >= 16 {
            let mut tag = u16_at(body);
            if tag == 0xFFFE && len >= 26 {
                tag = u16_at(body + 24);
            }
            fmt = Some((tag, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            let (tag, channels, sample_rate, bits) = fmt.ok_or("data chunk before fmt")?;
            let data = &b[body..end];
            let ch = channels.max(1) as usize;
            let frames: Vec<f32> = match (tag, bits) {
                (1, 16) => data.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0).collect(),
                (1, 24) => data.chunks_exact(3).map(|s| (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0).collect(),
                (1, 32) => data.chunks_exact(4).map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0).collect(),
                (3, 32) => data.chunks_exact(4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]])).collect(),
                _ => return Err(format!("unsupported WAV format tag {tag} with {bits} bits")),
            };
            let mono = frames.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
            return Ok(WavData { sample_rate, channels, mono });
        }
        i = body + len + (len & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + data.len() as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(channels.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2 * channels as u32).to_le_bytes());
        b.extend((2 * channels).to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"LIST");
        b.extend(3u32.to_le_bytes());
        b.extend([0, 0, 0, 0]);
        b.extend(b"data");
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(data);
        b
    }

    #[test]
    fn reads_pcm16_skips_unknown_chunks_and_mixes_down() {
        let w = parse(&wav(16_000, 2, &[16384, 0, -16384, -16384])).unwrap();
        assert_eq!((w.sample_rate, w.channels), (16_000, 2));
        assert_eq!(w.mono, vec![0.25, -0.5]);
        assert_eq!(w.to_i16_at(16_000), vec![8192, -16384]);
        assert!(parse(b"RIFF....WAVX").is_err());
    }
}
