//! Optional, bounded leading-silence detection on decoded PCM.
//! No seeking or complete-track PCM buffer is required.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rodio::Source;

static ENABLED: AtomicBool = AtomicBool::new(true);
const THRESHOLD: f32 = 0.0001; // -80 dBFS; deliberately conservative for quiet intros.
const MAX_SCAN: Duration = Duration::from_secs(10);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Removes only complete interleaved PCM frames at the beginning of a source.
/// Construction reads at most ten seconds of audio. Disabled construction does
/// not consume any samples. After detection, samples pass through unchanged.
pub struct LeadingSilence<S> {
    source: S,
    pending: VecDeque<f32>,
    pending_channels: u16,
    pending_rate: u32,
    trimmed: Duration,
    duration: Option<Duration>,
}

impl<S: Source<Item = f32>> LeadingSilence<S> {
    #[cfg(test)]
    pub fn new(source: S, enabled: bool) -> Self {
        Self::new_cancellable(source, enabled, &AtomicBool::new(false))
    }

    /// Stop scanning at a frame boundary when playback is requested urgently.
    pub fn new_cancellable(mut source: S, enabled: bool, skip: &AtomicBool) -> Self {
        let original_duration = source.total_duration();
        let mut pending = VecDeque::new();
        let mut trimmed = Duration::ZERO;
        let mut pending_channels = source.channels();
        let mut pending_rate = source.sample_rate();
        let mut segment_rate = pending_rate;
        let mut segment_frames = 0_u64;
        let mut previous_segments = Duration::ZERO;
        if enabled {
            while !skip.load(Ordering::Relaxed) {
                let Some(first) = source.next() else { break };
                // Reading the first sample can advance the decoder to a new
                // packet, so query the format after that read.
                pending_channels = source.channels();
                pending_rate = source.sample_rate();
                pending.push_back(first);
                for _ in 1..pending_channels {
                    if let Some(sample) = source.next() {
                        pending.push_back(sample);
                    } else {
                        break;
                    }
                }
                if pending_channels == 0
                    || pending_rate == 0
                    || pending.len() != usize::from(pending_channels)
                    || pending
                        .iter()
                        .any(|s| !s.is_finite() || s.abs() > THRESHOLD)
                {
                    break;
                }
                if segment_rate != pending_rate {
                    previous_segments = trimmed;
                    segment_frames = 0;
                    segment_rate = pending_rate;
                }
                let next_trimmed = previous_segments
                    + Duration::from_secs_f64(
                        (segment_frames + 1) as f64 / f64::from(segment_rate),
                    );
                if next_trimmed > MAX_SCAN {
                    break;
                }
                segment_frames += 1;
                trimmed = next_trimmed;
                pending.clear();
            }
        }
        Self {
            source,
            pending,
            pending_channels,
            pending_rate,
            trimmed,
            duration: original_duration.map(|duration| duration.saturating_sub(trimmed)),
        }
    }

    pub fn trimmed_duration(&self) -> Duration {
        self.trimmed
    }
}

impl<S: Source<Item = f32>> Iterator for LeadingSilence<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.pending.pop_front().or_else(|| self.source.next())
    }
}

impl<S: Source<Item = f32>> Source for LeadingSilence<S> {
    fn current_frame_len(&self) -> Option<usize> {
        if self.pending.is_empty() {
            self.source.current_frame_len()
        } else {
            Some(self.pending.len())
        }
    }

    fn channels(&self) -> u16 {
        if self.pending.is_empty() {
            self.source.channels()
        } else {
            self.pending_channels
        }
    }

    fn sample_rate(&self) -> u32 {
        if self.pending.is_empty() {
            self.source.sample_rate()
        } else {
            self.pending_rate
        }
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    #[test]
    fn cancellation_before_scan_preserves_every_sample() {
        let samples = vec![0.0, 0.0, 0.5, -0.5];
        let source = LeadingSilence::new_cancellable(
            SamplesBuffer::new(2, 1_000, samples.clone()),
            true,
            &AtomicBool::new(true),
        );
        assert_eq!(source.trimmed_duration(), Duration::ZERO);
        assert_eq!(source.collect::<Vec<_>>(), samples);
    }

    #[test]
    fn cancellation_during_scan_stops_at_a_complete_frame() {
        struct CancelOnRead<'a> {
            source: SamplesBuffer<f32>,
            skip: &'a AtomicBool,
            reads: usize,
        }
        impl Iterator for CancelOnRead<'_> {
            type Item = f32;
            fn next(&mut self) -> Option<f32> {
                self.reads += 1;
                if self.reads == 3 {
                    self.skip.store(true, Ordering::Relaxed);
                }
                self.source.next()
            }
        }
        impl Source for CancelOnRead<'_> {
            fn current_frame_len(&self) -> Option<usize> {
                self.source.current_frame_len()
            }
            fn channels(&self) -> u16 {
                self.source.channels()
            }
            fn sample_rate(&self) -> u32 {
                self.source.sample_rate()
            }
            fn total_duration(&self) -> Option<Duration> {
                self.source.total_duration()
            }
        }
        let skip = AtomicBool::new(false);
        let source = LeadingSilence::new_cancellable(
            CancelOnRead {
                source: SamplesBuffer::new(2, 1_000, vec![0.0; 100]),
                skip: &skip,
                reads: 0,
            },
            true,
            &skip,
        );
        assert_eq!(source.trimmed_duration(), Duration::from_millis(2));
        assert_eq!(source.count(), 96);
    }

    #[test]
    fn trims_leading_only_and_preserves_stereo_alignment() {
        let samples = vec![
            0.0, 0.0, 0.00001, -0.00001, 0.0, 0.5, 0.0, 0.0, 0.2, -0.2, 0.0, 0.0,
        ];
        let source = LeadingSilence::new(SamplesBuffer::new(2, 1_000, samples.clone()), true);
        assert_eq!(source.trimmed_duration(), Duration::from_millis(2));
        assert_eq!(source.total_duration(), Some(Duration::from_millis(4)));
        assert_eq!(source.current_frame_len(), Some(2));
        assert_eq!(source.channels(), 2);
        assert_eq!(source.sample_rate(), 1_000);
        assert_eq!(source.collect::<Vec<_>>(), samples[4..]);
    }

    #[test]
    fn disabled_preserves_every_sample_and_duration() {
        let samples = vec![0.0, 0.0, 0.5, 0.0];
        let source = LeadingSilence::new(SamplesBuffer::new(1, 1_000, samples.clone()), false);
        assert_eq!(source.trimmed_duration(), Duration::ZERO);
        assert_eq!(source.total_duration(), Some(Duration::from_millis(4)));
        assert_eq!(source.collect::<Vec<_>>(), samples);
    }

    #[test]
    fn no_leading_silence_preserves_first_frame() {
        let samples = vec![0.3, -0.3, 0.0, 0.0];
        let source = LeadingSilence::new(SamplesBuffer::new(2, 1_000, samples.clone()), true);
        assert_eq!(source.trimmed_duration(), Duration::ZERO);
        assert_eq!(source.collect::<Vec<_>>(), samples);
    }

    #[test]
    fn empty_and_entirely_silent_sources_are_safe() {
        for samples in [vec![], vec![0.0; 20]] {
            let source = LeadingSilence::new(SamplesBuffer::new(2, 1_000, samples), true);
            assert_eq!(source.total_duration(), Some(Duration::ZERO));
            assert_eq!(source.count(), 0);
        }
    }

    #[test]
    fn scan_is_bounded_and_preserves_remaining_silence() {
        let source = LeadingSilence::new(SamplesBuffer::new(1, 1_000, vec![0.0; 12_000]), true);
        assert_eq!(source.trimmed_duration(), Duration::from_secs(10));
        assert_eq!(source.total_duration(), Some(Duration::from_secs(2)));
        assert_eq!(source.count(), 2_000);
    }

    #[test]
    fn non_finite_samples_are_not_silence() {
        let source = LeadingSilence::new(SamplesBuffer::new(1, 1_000, vec![0.0, f32::NAN]), true);
        assert_eq!(source.trimmed_duration(), Duration::from_millis(1));
        assert!(source.collect::<Vec<_>>()[0].is_nan());
    }

    #[test]
    fn real_sample_rates_use_sample_counts_without_rounding_drift() {
        for rate in [44_100, 48_000] {
            let source = LeadingSilence::new(
                SamplesBuffer::new(2, rate, vec![0.0; rate as usize * 22]),
                true,
            );
            assert_eq!(source.trimmed_duration(), MAX_SCAN);
            assert_eq!(source.count(), rate as usize * 2);
        }
    }

    #[test]
    fn unknown_duration_and_infinite_streams_are_bounded() {
        let pcm = SamplesBuffer::new(2, 1_000, vec![0.0; 20]).repeat_infinite();
        let source = LeadingSilence::new(pcm, true);
        assert_eq!(source.total_duration(), None);
        assert_eq!(source.trimmed_duration(), MAX_SCAN);
        assert_eq!(source.take(4).collect::<Vec<_>>(), vec![0.0; 4]);
    }
}
