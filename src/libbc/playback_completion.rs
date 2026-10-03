//! Notify when Rodio releases a played (or skipped) source.
//! Sending on drop, rather than inside next(), lets Rodio update its queue count first.

use std::time::Duration;

use rodio::Source;
use tokio::sync::oneshot;

pub struct NotifyOnEnd<S> {
    source: S,
    finished: Option<oneshot::Sender<()>>,
}

impl<S> NotifyOnEnd<S> {
    pub fn new(source: S) -> (Self, oneshot::Receiver<()>) {
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                source,
                finished: Some(sender),
            },
            receiver,
        )
    }
}

impl<S> Drop for NotifyOnEnd<S> {
    fn drop(&mut self) {
        if let Some(sender) = self.finished.take() {
            let _ = sender.send(());
        }
    }
}

impl<S: Source<Item = f32>> Iterator for NotifyOnEnd<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.source.next()
    }
}

impl<S: Source<Item = f32>> Source for NotifyOnEnd<S> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libbc::trailing_silence::{EndAt, EndMarker};
    use rodio::buffer::SamplesBuffer;

    #[tokio::test]
    async fn marker_and_unmarked_eof_notify_after_sink_is_empty() {
        for marker in [
            None,
            Some(EndMarker {
                frame: 2,
                sample_rate: 48_000,
            }),
        ] {
            let (sink, mut output) = rodio::Sink::new_idle();
            let (source, finished) = NotifyOnEnd::new(EndAt::new(
                SamplesBuffer::new(1, 48_000, vec![0.5; 10]),
                marker,
            ));
            sink.append(source);
            for _ in 0..11 {
                output.next();
            }
            tokio::time::timeout(Duration::from_millis(100), finished)
                .await
                .unwrap()
                .unwrap();
            assert!(sink.empty());
        }
    }

    #[tokio::test]
    async fn skip_notifies_even_while_paused_without_stopping_sink() {
        let (sink, mut output) = rodio::Sink::new_idle();
        let (source, finished) = NotifyOnEnd::new(SamplesBuffer::new(1, 48_000, vec![0.5; 48_000]));
        sink.append(source);
        sink.pause();
        sink.skip_one();
        for _ in 0..1_000 {
            output.next();
        }
        tokio::time::timeout(Duration::from_millis(100), finished)
            .await
            .unwrap()
            .unwrap();
        assert!(sink.empty());
        assert!(sink.is_paused());
        sink.append(SamplesBuffer::new(1, 48_000, vec![0.5; 10]));
        assert!(!sink.empty());
    }
}
