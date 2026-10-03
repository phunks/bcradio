//! Player-owned work: at most one preparation and one Discover refill.
//! Workers return results; only the playback loop changes the queue.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use rodio::Source;
use tokio::task::JoinHandle;

use crate::libbc::http_client::get_request;
use crate::libbc::leading_silence::{self, LeadingSilence};
use crate::libbc::progress_bar::{disable_spinner, enable_spinner};
use crate::libbc::sink::Mp3;
use crate::libbc::trailing_silence::{self, EndAt};
use crate::models::shared_data_models::Track;

pub struct ManagedTask<T> {
    pub handle: JoinHandle<T>,
    pub skip_analysis: Arc<AtomicBool>,
}

impl<T> ManagedTask<T> {
    pub fn new(handle: JoinHandle<T>, skip_analysis: Arc<AtomicBool>) -> Self {
        Self {
            handle,
            skip_analysis,
        }
    }

    pub fn skip_analysis(&self) {
        self.skip_analysis.store(true, Ordering::Relaxed);
    }
}

impl<T> Drop for ManagedTask<T> {
    fn drop(&mut self) {
        // abort alone cannot stop an already-running blocking worker.
        self.skip_analysis();
        self.handle.abort();
    }
}

pub struct PreparedAudio {
    pub source: Box<dyn Source<Item = f32> + Send>,
    pub original_duration: Duration,
    pub playback_duration: Duration,
}

pub struct Preparation {
    pub url: String,
    pub task: ManagedTask<Result<PreparedAudio>>,
}

impl Preparation {
    pub fn start(track: Track) -> Self {
        let url = track.url.clone();
        let skip_analysis = Arc::new(AtomicBool::new(false));
        let skip = skip_analysis.clone();
        let handle = tokio::spawn(async move {
            let _spinner = SpinnerGuard::new();
            let buffer = if track.buffer.is_empty() {
                get_request(&track.url).await?
            } else {
                track.buffer
            };
            tokio::task::spawn_blocking(move || prepare_audio(buffer, skip)).await?
        });
        Self {
            url,
            task: ManagedTask::new(handle, skip_analysis),
        }
    }
}

fn prepare_audio(buffer: Vec<u8>, skip: Arc<AtomicBool>) -> Result<PreparedAudio> {
    let trim_leading = leading_silence::enabled();
    let trim_trailing = trailing_silence::enabled();
    let original_duration = mp3_duration::from_read(&mut std::io::Cursor::new(&buffer))?;
    let mp3 = Mp3::load(buffer)?;
    let pcm_duration = mp3.pcm_duration().ok().flatten();
    let marker = if trim_trailing && !skip.load(Ordering::Relaxed) {
        match trailing_silence::analyze_mp3_cancellable(mp3.as_ref(), &skip) {
            Ok(marker) => marker,
            Err(e) => {
                log::debug!("tail analysis skipped: {e:#}");
                None
            }
        }
    } else {
        None
    };
    let marker = marker.filter(|_| !skip.load(Ordering::Relaxed));
    let source = LeadingSilence::new_cancellable(
        EndAt::new(mp3.decoder()?.convert_samples::<f32>(), marker),
        trim_leading,
        &skip,
    );
    let playback_duration = marker
        .map(|m| m.duration())
        .or(pcm_duration)
        .unwrap_or(original_duration)
        .saturating_sub(source.trimmed_duration());
    Ok(PreparedAudio {
        source: Box::new(source),
        original_duration,
        playback_duration,
    })
}

struct SpinnerGuard;

impl SpinnerGuard {
    fn new() -> Self {
        enable_spinner();
        Self
    }
}

impl Drop for SpinnerGuard {
    fn drop(&mut self) {
        disable_spinner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropping_task_stops_started_blocking_work_cooperatively() {
        let skip = Arc::new(AtomicBool::new(false));
        let worker_skip = skip.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let _ = started_tx.send(());
                while !worker_skip.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                let _ = done_tx.send(());
            })
            .await
            .unwrap();
        });
        let task = ManagedTask::new(handle, skip.clone());
        started_rx.await.unwrap();
        drop(task);
        assert!(skip.load(Ordering::Relaxed));
        tokio::time::timeout(Duration::from_secs(1), done_rx)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn skipped_analysis_still_prepares_a_real_mp3() {
        // MPEG-1 Layer III frames containing silence; no network or encoder.
        let mut buffer = Vec::new();
        for _ in 0..20 {
            let mut frame = vec![0_u8; 417];
            frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
            buffer.extend(frame);
        }
        let audio = prepare_audio(buffer, Arc::new(AtomicBool::new(true))).unwrap();
        assert!(audio.original_duration > Duration::ZERO);
        assert!(audio.playback_duration > Duration::ZERO);
        assert!(audio.source.count() > 0);
    }
}
