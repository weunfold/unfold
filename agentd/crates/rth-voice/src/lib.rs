

pub mod echo;
pub mod mic;
pub mod owner;
pub mod queue;
pub mod resample;
pub mod sim;
pub mod vad;
pub mod wav;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn samples_from_le(bytes: &[u8]) -> Vec<i16> {
    bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

pub fn le_from_samples(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}
