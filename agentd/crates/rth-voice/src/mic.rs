

use base64::Engine;
use rth_protocol::{Event, MAX_AUDIO_CHUNK_BYTES, MIC_SAMPLE_RATE};
use sha2::{Digest, Sha256};

pub const DEFAULT_CHUNK_MS: u32 = 40;

pub struct MicChunker {
    pub stream_id: String,
    chunk_samples: usize,
    buf: Vec<i16>,
    next_chunk: u64,
    next_sample: u64,
}

impl MicChunker {
    pub fn new(stream_id: impl Into<String>, chunk_ms: u32) -> Self {
        let max = MAX_AUDIO_CHUNK_BYTES / 2;
        let chunk_samples = ((MIC_SAMPLE_RATE * chunk_ms / 1000) as usize).clamp(1, max);
        Self { stream_id: stream_id.into(), chunk_samples, buf: Vec::new(), next_chunk: 0, next_sample: 0 }
    }

    pub fn chunk_samples(&self) -> usize {
        self.chunk_samples
    }

    pub fn samples_emitted(&self) -> u64 {
        self.next_sample
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    pub fn discard(&mut self) -> usize {
        std::mem::take(&mut self.buf).len()
    }

    pub fn push(&mut self, samples: &[i16]) -> Vec<Event> {
        self.buf.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.buf.len() >= self.chunk_samples {
            let rest = self.buf.split_off(self.chunk_samples);
            let chunk = std::mem::replace(&mut self.buf, rest);
            out.push(self.event(&chunk));
        }
        out
    }

    pub fn finish(&mut self, tally: &Tally) -> (Option<Event>, Event) {
        let tail = self.buf.len() as u64;
        let last = (!self.buf.is_empty()).then(|| {
            let chunk = std::mem::take(&mut self.buf);
            self.event(&chunk)
        });
        let end = Event::UserAudioEnd {
            stream_id: self.stream_id.clone(),
            chunks: tally.chunks + u64::from(last.is_some()),
            samples: tally.samples + tail,
            dropped_chunks: tally.dropped_chunks, speaker: Default::default() };
        (last, end)
    }

    fn event(&mut self, chunk: &[i16]) -> Event {
        let data = base64::engine::general_purpose::STANDARD.encode(crate::le_from_samples(chunk));
        let e = Event::UserAudio {
            stream_id: self.stream_id.clone(),
            chunk: self.next_chunk,
            sample_offset: self.next_sample,
            sample_rate: MIC_SAMPLE_RATE,
            format: "s16le".into(),
            data,
            digest: None,
        };
        self.next_chunk += 1;
        self.next_sample += chunk.len() as u64;
        e
    }
}

#[derive(Default, Clone)]
pub struct Tally {
    pub chunks: u64,
    pub samples: u64,
    pub bytes: u64,
    pub dropped_chunks: u64,
    sha: Sha256,
}

impl Tally {

    pub fn sent(&mut self, e: &Event) {
        if let Event::UserAudio { data, .. } = e
            && let Ok(pcm) = base64::engine::general_purpose::STANDARD.decode(data)
        {
            self.chunks += 1;
            self.samples += (pcm.len() / 2) as u64;
            self.bytes += pcm.len() as u64;
            self.sha.update(&pcm);
        }
    }

    pub fn dropped(&mut self) {
        self.dropped_chunks += 1;
    }

    pub fn sha256(&self) -> String {
        crate::hex(&self.sha.clone().finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_protocol::{Envelope, MAX_FRAME, MonotonicClock, Source, Stamper};

    #[test]
    fn chunking_round_trips_through_the_wire_with_contiguous_offsets() {
        let input: Vec<i16> = (0..16_000 + 123).map(|i| (i % 2000) as i16 - 1000).collect();
        let mut m = MicChunker::new("mic-1", 40);
        let mut tally = Tally::default();
        let mut host = Stamper::new(Source::Host, "host-x", Box::new(MonotonicClock::start()));
        let mut events = Vec::new();

        for piece in input.chunks(173) {
            events.extend(m.push(piece));
        }
        let mut rebuilt = Vec::new();
        let mut end = None;
        let mut i = 0;
        while i < events.len() || end.is_none() {
            if i == events.len() {
                let (last, e) = m.finish(&tally);
                events.extend(last);
                end = Some(e);
                continue;
            }
            let e = &events[i];
            let line = host.stamp(e.clone(), None).to_line();
            assert!(line.len() < MAX_FRAME);
            let back = Envelope::parse_from(line.trim_end(), Source::Host).unwrap();
            let Event::UserAudio { chunk, sample_offset, sample_rate, data, .. } = &back.event else { panic!() };
            assert_eq!((*chunk, *sample_offset, *sample_rate), (i as u64, rebuilt.len() as u64, 16_000));
            let pcm = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
            assert!(pcm.len() <= MAX_AUDIO_CHUNK_BYTES);
            rebuilt.extend(crate::samples_from_le(&pcm));
            tally.sent(&back.event);
            i += 1;
        }
        assert_eq!(rebuilt, input, "byte-exact after chunking, framing and base64");
        assert_eq!(events.len(), 26, "25 full 40 ms chunks and one short tail");
        let Some(Event::UserAudioEnd { chunks, samples, .. }) = end else { panic!() };
        assert_eq!((chunks, samples), (26, input.len() as u64));
        let direct = crate::hex(&Sha256::digest(crate::le_from_samples(&input)));
        assert_eq!(tally.sha256(), direct);
    }

    #[test]
    fn chunk_size_is_capped_by_the_protocol() {
        assert_eq!(MicChunker::new("m", 10_000).chunk_samples(), MAX_AUDIO_CHUNK_BYTES / 2);
        assert_eq!(MicChunker::new("m", 20).chunk_samples(), 320);
    }
}
