//! Downloaded-buffer tail analysis and a separate PCM end-marker adapter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{ensure, Result};
use rodio::Source;

use crate::libbc::sink::Mp3;

static ENABLED: AtomicBool = AtomicBool::new(true);
const THRESHOLD: f32 = 0.0001; // -80 dBFS, matching leading detection.
const KEEP_TAIL: Duration = Duration::from_millis(100);
const PREROLL: Duration = Duration::from_millis(250);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// An exclusive end position on the unmodified, gapless-decoded PCM timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndMarker {
    pub frame: u64,
    pub sample_rate: u32,
}

impl EndMarker {
    pub fn duration(self) -> Duration {
        Duration::from_secs_f64(self.frame as f64 / f64::from(self.sample_rate))
    }
}

/// Failure is non-fatal: the caller keeps the MP3 and simply omits its marker.
#[cfg(test)]
pub fn analyze_mp3(buffer: &[u8]) -> Result<Option<EndMarker>> {
    analyze_mp3_cancellable(buffer, &AtomicBool::new(false))
}

pub fn analyze_mp3_cancellable(buffer: &[u8], skip: &AtomicBool) -> Result<Option<EndMarker>> {
    if skip.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let started = Instant::now();
    let mp3 = Mp3::load(buffer.to_vec())?;
    let mut pcm = mp3.decoder()?.convert_samples::<f32>();
    let duration = mp3
        .pcm_duration()?
        .ok_or_else(|| anyhow::anyhow!("unknown PCM duration"))?;
    let marker = analyze_duration_cancellable(&mut pcm, duration, skip)?;
    log::debug!(
        "MP3 tail analysis: {:?}, elapsed {:?}",
        marker,
        started.elapsed()
    );
    Ok(marker)
}

#[cfg(test)]
fn analyze_duration<S: Source<Item = f32>>(
    pcm: &mut S,
    duration: Duration,
) -> Result<Option<EndMarker>> {
    analyze_duration_cancellable(pcm, duration, &AtomicBool::new(false))
}

fn analyze_duration_cancellable<S: Source<Item = f32>>(
    pcm: &mut S,
    duration: Duration,
    skip: &AtomicBool,
) -> Result<Option<EndMarker>> {
    let rate = pcm.sample_rate();
    let channels = pcm.channels();
    ensure!(rate > 0 && channels > 0, "invalid PCM format");
    let expected_end = (duration.as_secs_f64() * f64::from(rate)).round() as u64;
    let margin = (KEEP_TAIL.as_secs_f64() * f64::from(rate)).ceil() as u64;
    for seconds in [3, 6, 10] {
        if skip.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let start = duration.saturating_sub(Duration::from_secs(seconds));
        let seek = start.saturating_sub(PREROLL);
        pcm.try_seek(seek)
            .map_err(|e| anyhow::anyhow!("tail seek failed: {e}"))?;
        let first_frame = (seek.as_secs_f64() * f64::from(rate)).floor() as u64;
        let detection_start = (start.as_secs_f64() * f64::from(rate)).ceil() as u64;
        let mut frame = first_frame;
        let mut last_sound = None;
        // Bound decoding even if duration metadata is wrong. Accurate seek may
        // still parse headers from the beginning, but does not decode that prefix.
        let limit = expected_end.saturating_sub(first_frame) + u64::from(rate);
        loop {
            if skip.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let Some(first) = pcm.next() else { break };
            ensure!(
                pcm.sample_rate() == rate && pcm.channels() == channels,
                "PCM format changed"
            );
            let mut audible = !first.is_finite() || first.abs() > THRESHOLD;
            for _ in 1..channels {
                let sample = pcm
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("incomplete PCM frame"))?;
                audible |= !sample.is_finite() || sample.abs() > THRESHOLD;
            }
            if frame >= detection_start && audible {
                last_sound = Some(frame + 1);
            }
            frame += 1;
            ensure!(
                frame - first_frame <= limit,
                "tail exceeds duration metadata"
            );
        }
        // Do not apply absolute markers when duration and decoded EOF disagree.
        ensure!(
            frame.abs_diff(expected_end) <= 2,
            "PCM duration/seek mismatch"
        );
        if let Some(last_sound) = last_sound {
            let end = last_sound.saturating_add(margin).min(frame);
            return Ok((end < frame).then_some(EndMarker {
                frame: end,
                sample_rate: rate,
            }));
        }
        if start.is_zero() {
            // Entirely silent short tracks are intentionally left untouched.
            return Ok(None);
        }
    }
    // No audible boundary within ten seconds: do not guess where the song ends.
    Ok(None)
}

/// Stops PCM at a precomputed marker, before leading trimming is applied.
/// Without a marker this is a transparent pass-through adapter.
pub struct EndAt<S> {
    source: S,
    remaining: Option<u64>,
    duration: Option<Duration>,
}

impl<S: Source<Item = f32>> EndAt<S> {
    pub fn new(source: S, marker: Option<EndMarker>) -> Self {
        let marker = marker.filter(|m| m.sample_rate > 0 && m.sample_rate == source.sample_rate());
        let duration = marker
            .map(EndMarker::duration)
            .or_else(|| source.total_duration());
        let remaining = marker.map(|m| m.frame.saturating_mul(u64::from(source.channels())));
        Self {
            source,
            remaining,
            duration,
        }
    }
}

impl<S: Source<Item = f32>> Iterator for EndAt<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.remaining == Some(0) {
            return None;
        }
        let sample = self.source.next()?;
        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }
        Some(sample)
    }
}

impl<S: Source<Item = f32>> Source for EndAt<S> {
    fn current_frame_len(&self) -> Option<usize> {
        match (self.source.current_frame_len(), self.remaining) {
            (Some(len), Some(remaining)) => {
                Some(len.min(remaining.min(usize::MAX as u64) as usize))
            }
            (None, Some(remaining)) => Some(remaining.min(usize::MAX as u64) as usize),
            (len, None) => len,
        }
    }
    fn channels(&self) -> u16 {
        self.source.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libbc::leading_silence::LeadingSilence;
    use rodio::buffer::SamplesBuffer;

    fn analyze<S: Source<Item = f32>>(pcm: &mut S) -> Result<Option<EndMarker>> {
        let duration = pcm
            .total_duration()
            .ok_or_else(|| anyhow::anyhow!("unknown duration"))?;
        analyze_duration(pcm, duration)
    }

    // Deterministic seekable PCM source to exercise the same window algorithm.
    struct Pcm {
        samples: Vec<f32>,
        offset: usize,
        rate: u32,
        cancel_after: Option<(usize, std::sync::Arc<AtomicBool>)>,
    }
    impl Iterator for Pcm {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if let Some((offset, skip)) = &self.cancel_after {
                if self.offset >= *offset {
                    skip.store(true, Ordering::Relaxed);
                }
            }
            let sample = self.samples.get(self.offset).copied()?;
            self.offset += 1;
            Some(sample)
        }
    }
    impl Source for Pcm {
        fn current_frame_len(&self) -> Option<usize> {
            Some(self.samples.len() - self.offset)
        }
        fn channels(&self) -> u16 {
            2
        }
        fn sample_rate(&self) -> u32 {
            self.rate
        }
        fn total_duration(&self) -> Option<Duration> {
            Some(Duration::from_secs_f64(
                self.samples.len() as f64 / 2.0 / f64::from(self.rate),
            ))
        }
        fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
            self.offset = (pos.as_secs_f64() * f64::from(self.rate)).floor() as usize * 2;
            Ok(())
        }
    }
    fn pcm(seconds: usize, sound_end: usize) -> Pcm {
        let mut samples = vec![0.0; seconds * 1_000 * 2];
        for frame in 1_000..sound_end * 1_000 {
            samples[frame * 2 + 1] = 0.5;
        }
        Pcm {
            samples,
            offset: 0,
            rate: 1_000,
            cancel_after: None,
        }
    }

    #[test]
    fn cancelled_tail_does_not_seek_or_read_invalid_mp3() {
        let skip = AtomicBool::new(true);
        let mut source = pcm(8, 6);
        assert_eq!(
            analyze_duration_cancellable(&mut source, Duration::from_secs(8), &skip).unwrap(),
            None
        );
        assert_eq!(source.offset, 0);
        assert_eq!(analyze_mp3_cancellable(b"invalid", &skip).unwrap(), None);
    }

    #[test]
    fn tail_cancellation_during_decoding_leaves_no_marker() {
        let skip = std::sync::Arc::new(AtomicBool::new(false));
        let mut source = pcm(8, 6);
        source.cancel_after = Some((10_000, skip.clone()));
        assert_eq!(
            analyze_duration_cancellable(&mut source, Duration::from_secs(8), &skip).unwrap(),
            None
        );
        assert_eq!(source.offset, 10_002);
        assert!(source.offset < source.samples.len());
    }

    #[test]
    fn finds_tail_and_expands_window_with_stereo_and_margin() {
        for (seconds, sound_end) in [(8, 6), (12, 7), (15, 7)] {
            let marker = analyze(&mut pcm(seconds, sound_end)).unwrap().unwrap();
            assert_eq!(marker.frame, sound_end as u64 * 1_000 + 100);
            assert_eq!(marker.sample_rate, 1_000);
        }
    }

    #[test]
    fn no_boundary_or_no_tail_means_no_cut() {
        for (seconds, sound_end) in [(12, 12), (20, 2), (2, 0)] {
            assert_eq!(analyze(&mut pcm(seconds, sound_end)).unwrap(), None);
        }
    }

    #[test]
    fn duration_mismatch_is_rejected_and_short_tails_are_preserved() {
        let mut source = pcm(8, 6);
        assert!(analyze_duration(&mut source, Duration::from_secs(7)).is_err());
        let mut source = pcm(8, 8);
        source.samples[7_950 * 2..].fill(0.0);
        assert_eq!(analyze(&mut source).unwrap(), None);
    }

    #[test]
    fn marked_pcm_finishes_a_sink_without_an_audio_device() {
        let (sink, mut output) = rodio::Sink::new_idle();
        let source = EndAt::new(
            SamplesBuffer::new(2, 48_000, vec![0.5, -0.5, 0.0, 0.0]),
            Some(EndMarker {
                frame: 1,
                sample_rate: 48_000,
            }),
        );
        sink.append(source);
        assert_eq!(output.by_ref().take(2).collect::<Vec<_>>(), vec![0.5, -0.5]);
        let _ = output.next();
        assert!(sink.empty());
    }

    #[test]
    fn combines_with_leading_trim_and_preserves_internal_silence() {
        let mut samples = vec![0.0; 8_000 * 2];
        samples[2_000] = 0.5;
        samples[11_999] = -0.5;
        let mut pcm = Pcm {
            samples: samples.clone(),
            offset: 0,
            rate: 1_000,
            cancel_after: None,
        };
        let marker = analyze(&mut pcm).unwrap().unwrap();
        let end = EndAt::new(SamplesBuffer::new(2, 1_000, samples.clone()), Some(marker));
        let trimmed = LeadingSilence::new(end, true);
        assert_eq!(trimmed.total_duration(), Some(Duration::from_millis(5_100)));
        assert_eq!(trimmed.collect::<Vec<_>>(), samples[2_000..12_200]);
        let disabled = EndAt::new(SamplesBuffer::new(2, 1_000, samples.clone()), None);
        assert_eq!(disabled.collect::<Vec<_>>(), samples);
    }

    #[test]
    fn seek_failure_is_reported_for_safe_fallback() {
        let mut pcm = EndAt::new(SamplesBuffer::new(2, 1_000, vec![0.0; 2_000]), None);
        assert!(analyze(&mut pcm).is_err());
        assert!(analyze_mp3(b"not an mp3").is_err());
    }

    #[test]
    fn mp3_decoder_seeks_on_the_same_pcm_timeline() {
        // Valid MPEG-1 Layer III CBR frames with zero spectral data. No
        // external encoder, network, or audio device is needed for this test.
        let mut buffer = Vec::new();
        let mut info = vec![0_u8; 417];
        info[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
        info[36..40].copy_from_slice(b"Xing");
        info[40..44].copy_from_slice(&1_u32.to_be_bytes());
        info[44..48].copy_from_slice(&400_u32.to_be_bytes());
        buffer.extend(info);
        for _ in 0..400 {
            let mut frame = vec![0_u8; 417];
            frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
            buffer.extend(frame);
        }
        let mp3 = Mp3::load(buffer.clone()).unwrap();
        let mut pcm = mp3.decoder().unwrap().convert_samples::<f32>();
        let duration = mp3.pcm_duration().unwrap().unwrap();
        let rate = pcm.sample_rate();
        let channels = pcm.channels();
        let expected = (duration.as_secs_f64() * f64::from(rate)).round() as u64;
        let all_frames = pcm.count() as u64 / u64::from(channels);
        assert!(all_frames.abs_diff(expected) <= 2);
        pcm = mp3.decoder().unwrap().convert_samples::<f32>();
        let start = duration.saturating_sub(Duration::from_secs(3));
        let started = Instant::now();
        pcm.try_seek(start).unwrap();
        let decoded_frames = pcm.count() as u64 / u64::from(channels);
        let offset = (start.as_secs_f64() * f64::from(rate)).floor() as u64;
        assert!((offset + decoded_frames).abs_diff(all_frames) <= 2);
        eprintln!(
            "MP3 {:.2}s: tail seek/decode {:?}, decoded {} frames vs {} full frames",
            duration.as_secs_f64(),
            started.elapsed(),
            decoded_frames,
            all_frames
        );
        assert_eq!(analyze_mp3(&buffer).unwrap(), None);
    }
}
