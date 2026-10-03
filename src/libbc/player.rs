#[cfg(test)]
use std::time::Duration;

use anyhow::{Error, Result};
use async_channel::Receiver;

use crate::libbc::ai;
use crate::libbc::args::{about, args_genre, args_list_devices, args_sub_genre};
use crate::libbc::command::Command;
use crate::libbc::http_client::get_request;
use crate::libbc::playback_completion::NotifyOnEnd;
use crate::libbc::playback_tasks::{ManagedTask, Preparation, PreparedAudio};
use crate::libbc::playlist::{fetch_discover_page, format, PlayList, Selection};
use crate::libbc::progress_bar::{
    disable_tick, disable_tick_on_screen, enable_tick, enable_tick_on_screen, run,
    set_quit_pending, update_song_info_on_screen,
};
use crate::libbc::search::{input_panel, Search};
use crate::libbc::shared_data::SharedState;
use crate::libbc::sink::{list_host_devices, MusicStruct};
use crate::libbc::terminal::{show_alt_term, show_alt_term2};
use crate::models::bc_discover_index::PostData;
use crate::models::bc_discover_json::DiscoverJsonRequest;
use crate::models::bc_error::BcradioError;
use crate::models::shared_data_models::ResultsJson;
use crate::{ceil, format_duration};
use rodio::Sink;
use tokio::task::JoinHandle;

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct QuitDisplay;

impl QuitDisplay {
    fn new() -> Self {
        set_quit_pending(true);
        Self
    }
}

impl Drop for QuitDisplay {
    fn drop(&mut self) {
        set_quit_pending(false);
    }
}

fn map_volume_to_rodio_volume(volume: u8) -> f32 {
    (volume as f32 / 9_f32).powf(2.0)
}

#[derive(Default)]
struct PlaybackWork {
    preparation: Option<Preparation>,
    ready: Option<(String, PreparedAudio)>,
    refill: Option<(PostData, ManagedTask<Result<DiscoverJsonRequest>>)>,
    urgent: bool,
    finished: Option<tokio::sync::oneshot::Receiver<()>>,
    skipping: bool,
}

enum PlaybackEvent {
    Command(Command),
    Prepared(Result<PreparedAudio>),
    Refilled(Result<DiscoverJsonRequest>),
    Wake,
    Finished,
}

async fn playback_finished(receiver: &mut Option<tokio::sync::oneshot::Receiver<()>>) {
    match receiver {
        Some(receiver) => {
            let _ = receiver.await;
        }
        None => std::future::pending().await,
    }
}

async fn pending_task<T>(task: Option<&mut ManagedTask<Result<T>>>) -> Result<T> {
    match task {
        Some(task) => (&mut task.handle).await?,
        None => std::future::pending().await,
    }
}

impl PlaybackWork {
    async fn next_event(
        &mut self,
        commands: &Receiver<Command>,
        state: &SharedState,
    ) -> Result<PlaybackEvent> {
        tokio::select! {
            biased;
            command = commands.recv() => Ok(PlaybackEvent::Command(command?)),
            _ = playback_finished(&mut self.finished) => Ok(PlaybackEvent::Finished),
            result = pending_task(self.preparation.as_mut().map(|p| &mut p.task)) => Ok(PlaybackEvent::Prepared(result)),
            result = pending_task(self.refill.as_mut().map(|(_, task)| task)) => Ok(PlaybackEvent::Refilled(result)),
            _ = state.changed.notified() => Ok(PlaybackEvent::Wake),
        }
    }

    fn sync_preparation(&mut self, state: &SharedState) {
        let track = state.preparation_track();
        let url = track.as_ref().map(|track| track.url.as_str());
        if self
            .preparation
            .as_ref()
            .is_some_and(|p| Some(p.url.as_str()) != url)
        {
            self.preparation = None;
        }
        if self
            .ready
            .as_ref()
            .is_some_and(|(ready_url, _)| Some(ready_url.as_str()) != url)
        {
            self.ready = None;
        }
        if self.preparation.is_none() && self.ready.is_none() {
            if let Some(track) = track {
                let preparation = Preparation::start(track);
                if self.urgent {
                    preparation.task.skip_analysis();
                }
                self.preparation = Some(preparation);
            }
        }
    }

    fn next(&mut self, state: &SharedState, sink: &Sink) {
        if self.finished.is_none() || self.skipping {
            self.preparation = None;
            self.ready = None;
            state.skip_pending_track();
        } else {
            // Unlike stop(), skip_one() does not make the next append block
            // waiting for a stopped queue to flush. Wait for source-drop instead.
            sink.skip_one();
            self.skipping = true;
            if let Some(preparation) = &self.preparation {
                preparation.task.skip_analysis();
            }
        }
        self.urgent = true;
    }

    fn start_ready(&mut self, state: &SharedState, sink: &Sink) -> Result<()> {
        if self.finished.is_none() {
            if let Some((url, audio)) = self.ready.take() {
                if state.activate_prepared_track(&url, audio.original_duration) {
                    state.record_playback_start();
                    let mut info = state.get_current_track_info();
                    info.duration = audio.playback_duration.as_secs_f32();
                    update_song_info_on_screen(&info)?;
                    let (source, finished) = NotifyOnEnd::new(audio.source);
                    self.finished = Some(finished);
                    sink.append(source);
                    self.urgent = false;
                }
            }
        }
        Ok(())
    }
}

pub trait Player<'a>: Send + Sync + 'static {
    async fn player_thread(state: Self, commands: Receiver<Command>) -> Result<()>;
    fn track_info(&self) -> Result<Vec<String>>;
}

impl Player<'static> for SharedState {
    async fn player_thread(state: Self, commands: Receiver<Command>) -> Result<()> {
        if args_list_devices() {
            list_host_devices();
            return Err(Error::from(BcradioError::Quit));
        }

        if args_genre().is_some() || args_sub_genre().is_some() {
            let post_data = state.silent(args_genre(), args_sub_genre())?;
            state.store_results(&post_data).await?;
        } else {
            loop {
                match state.ask().await? {
                    Selection::Discover(post_data) => {
                        state.store_results(&post_data).await?;
                        break;
                    }
                    Selection::AiInput if ai_playlist(&state, false).await? => break,
                    Selection::AiInput => continue,
                }
            }
        }

        let mut _current_volume = 9;

        let stream_handle = MusicStruct::new()?;
        let sink = Sink::try_new(&stream_handle.stream_handle)?;
        state.input_gate.activate_playback();
        let _progress_handle = AbortOnDrop(run().await);
        let mut work = PlaybackWork::default();

        loop {
            if state.is_ai_playlist() {
                // AI refill already claims and spawns a single background request.
                state.fill_playlist().await?;
            } else if state.queue_length_from_truck_list() < 2 && work.refill.is_none() {
                let post = state.next_post();
                if post.cursor.is_some() {
                    let request = post.clone();
                    let task = ManagedTask::new(
                        tokio::spawn(async move { fetch_discover_page(&request).await }),
                        Default::default(),
                    );
                    work.refill = Some((post, task));
                } else if work.finished.is_none() && state.queue_length_from_truck_list() == 0 {
                    // Modal selection intentionally owns input while the queue is empty.
                    state.fill_playlist().await?;
                }
            }

            work.sync_preparation(&state);
            work.start_ready(&state, &sink)?;
            // Start the following download immediately after handing audio to Rodio.
            work.sync_preparation(&state);

            match work.next_event(&commands, &state).await? {
                PlaybackEvent::Prepared(result) => {
                    let preparation = work.preparation.take().unwrap();
                    match result {
                        Ok(audio) => work.ready = Some((preparation.url.clone(), audio)),
                        Err(e) => {
                            log::error!("track preparation failed: {e:#}");
                            state.skip_pending_track();
                        }
                    }
                }
                PlaybackEvent::Refilled(result) => {
                    let (mut post, _) = work.refill.take().unwrap();
                    // A modal playlist change can invalidate an in-flight request.
                    if serde_json::to_value(&post)? == serde_json::to_value(state.next_post())?
                        && !state.is_ai_playlist()
                    {
                        let page = result?;
                        let tracks = state.gen_track_list(&page.results)?;
                        post.cursor = page.cursor;
                        state.set_next_postdata(&post);
                        state.append_tracklist(tracks);
                    }
                }
                PlaybackEvent::Wake => {}
                PlaybackEvent::Finished => {
                    work.finished = None;
                    work.skipping = false;
                }
                PlaybackEvent::Command(res) => match res {
                    Command::Volume(volume) => {
                        // change volume
                        _current_volume = volume;
                        sink.set_volume(map_volume_to_rodio_volume(_current_volume));
                    }
                    Command::Next => work.next(&state, &sink),
                    Command::TogglePause => {
                        // play pause
                        if sink.is_paused() {
                            sink.play();
                            enable_tick();
                        } else {
                            sink.pause();
                            disable_tick();
                        }
                    }
                    Command::Info => info(&state).await?,
                    Command::Menu => {
                        work.refill = None;
                        work.preparation = None;
                        work.ready = None;
                        menu(&state).await?;
                    }
                    Command::Playlist => playlist(&state)?,
                    Command::History => history(&state)?,
                    Command::FavoriteSearch => {
                        work.refill = None;
                        work.preparation = None;
                        work.ready = None;
                        favorite_search(&state).await?;
                    }
                    Command::Search => {
                        work.refill = None;
                        work.preparation = None;
                        work.ready = None;
                        search(&state).await?;
                    }
                    Command::AiPlaylist => {
                        work.refill = None;
                        work.preparation = None;
                        work.ready = None;
                        ai_playlist(&state, true).await?;
                    }
                    Command::Help => help(&state)?,
                    Command::Options => options(&state)?,
                    Command::Quit => {
                        let _quit_display = QuitDisplay::new();
                        if wait_for_quit_or_cancel(&sink, &commands, &mut work.finished).await {
                            break;
                        }
                    }
                    Command::CancelQuit => {}
                    Command::Interrupt => break,
                },
            }
        }

        Ok(())
    }

    fn track_info(&self) -> Result<Vec<String>> {
        let current_track = self.get_current_track_info();
        let mut v = Vec::new();

        v.append(&mut vec!["".to_string()]);
        v.append(&mut vec![format!(
            " {:>14} {}",
            "Artist:", current_track.artist_name
        )]);
        v.append(&mut vec![format!(
            " {:>14} {}",
            "Album:", current_track.album_title
        )]);
        v.append(&mut vec![format!(
            " {:>14} {}",
            "Song:", current_track.track
        )]);
        v.append(&mut vec![format!(
            " {:>14} {}",
            "Duration:",
            format_duration!(ceil!(current_track.clone().duration, 1.0) as u32)
        )]);

        match current_track.results {
            ResultsJson::Select(g) => {
                let genres = self.get_genres().0;
                let genre = genres
                    .iter()
                    .find(|&x| x.id == g.band_genre_id as i64)
                    .cloned();

                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Category:",
                    genre.unwrap_or_default().label
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Genre:",
                    current_track.genre.clone().unwrap_or_default()
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Subgenre:",
                    current_track.subgenre.clone().unwrap_or_default()
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {} {:3.2}",
                    "Item Price:",
                    g.price.currency,
                    g.price.amount as f64 / 100.0
                )]);
                v.append(&mut vec![format!(" {:>14} {}", "Labels:", g.band_name)]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Location:",
                    g.band_location.unwrap_or_default()
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Release Date:", g.release_date
                )]);
                v.append(&mut vec![format!(" {:>14} {}", "Label URL:", g.band_url)]);
                // v.append(&mut vec![format!(" {:>14} {}", "Band URL:", g.band_url)]);
                v.append(&mut vec![format!(" {:>14} {}", "Item URL:", g.item_url)]);
            }
            ResultsJson::Search(g) => {
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Release Date:", g.current.release_date
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Album URL:",
                    g.album_url.unwrap_or_default()
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Item URL:",
                    g.item_url.unwrap_or_default()
                )]);
            }
            ResultsJson::None => {}
        }

        Ok(v)
    }
}

/// Returns true when playback finishes; Esc returns false to resume the normal queue.
async fn wait_for_quit_or_cancel(
    sink: &Sink,
    commands: &Receiver<Command>,
    finished: &mut Option<tokio::sync::oneshot::Receiver<()>>,
) -> bool {
    if finished.is_none() {
        return true;
    }
    let was_paused = sink.is_paused();
    if was_paused {
        sink.play();
        enable_tick();
    }
    loop {
        tokio::select! {
            _ = playback_finished(finished) => {
                *finished = None;
                return true;
            }
            command = commands.recv() => {
                match command {
                    Ok(Command::CancelQuit) => {
                        if was_paused {
                            sink.pause();
                            disable_tick();
                        }
                        return false;
                    }
                    Ok(Command::Interrupt) | Err(_) => return true,
                    _ => {} // Ignore other commands while finishing this song.
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libbc::leading_silence::LeadingSilence;
    use rodio::buffer::SamplesBuffer;

    fn pending_preparation(url: &str) -> Preparation {
        Preparation {
            url: url.into(),
            task: ManagedTask::new(tokio::spawn(std::future::pending()), Default::default()),
        }
    }

    #[tokio::test]
    async fn command_does_not_wait_for_preparation_or_refill() {
        let (sender, receiver) = async_channel::unbounded();
        let mut work = PlaybackWork {
            preparation: Some(pending_preparation("next")),
            refill: Some((
                PostData::default(),
                ManagedTask::new(tokio::spawn(std::future::pending()), Default::default()),
            )),
            ..Default::default()
        };
        sender.send(Command::Next).await.unwrap();
        assert!(matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                work.next_event(&receiver, &SharedState::default())
            )
            .await
            .unwrap()
            .unwrap(),
            PlaybackEvent::Command(Command::Next)
        ));
    }

    #[tokio::test]
    async fn preparation_completion_wakes_without_timer() {
        let (_sender, receiver) = async_channel::unbounded();
        let mut work = PlaybackWork {
            preparation: Some(Preparation {
                url: "next".into(),
                task: ManagedTask::new(
                    tokio::spawn(async { Err(anyhow::anyhow!("test failure")) }),
                    Default::default(),
                ),
            }),
            ..Default::default()
        };
        assert!(matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                work.next_event(&receiver, &SharedState::default())
            )
            .await
            .unwrap()
            .unwrap(),
            PlaybackEvent::Prepared(Err(_))
        ));
    }

    #[tokio::test]
    async fn refill_and_ai_notifications_wake_without_timer() {
        let (_sender, receiver) = async_channel::unbounded();
        let state = SharedState::default();
        let mut work = PlaybackWork {
            refill: Some((
                PostData::default(),
                ManagedTask::new(
                    tokio::spawn(async { Err(anyhow::anyhow!("test refill failure")) }),
                    Default::default(),
                ),
            )),
            ..Default::default()
        };
        assert!(matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                work.next_event(&receiver, &state)
            )
            .await
            .unwrap()
            .unwrap(),
            PlaybackEvent::Refilled(Err(_))
        ));
        work.refill = None;
        state.changed.notify_one();
        assert!(matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                work.next_event(&receiver, &state)
            )
            .await
            .unwrap()
            .unwrap(),
            PlaybackEvent::Wake
        ));
    }

    #[tokio::test]
    async fn skip_playing_song_cancels_analysis_but_keeps_next_download() {
        let state = SharedState::default();
        let (sink, _output) = rodio::Sink::new_idle();
        sink.append(SamplesBuffer::new(1, 1_000, vec![0.5; 1_000]));
        let mut work = PlaybackWork {
            preparation: Some(pending_preparation("next")),
            finished: Some(tokio::sync::oneshot::channel().1),
            ..Default::default()
        };
        work.next(&state, &sink);
        let preparation = work.preparation.as_ref().unwrap();
        assert!(preparation
            .task
            .skip_analysis
            .load(std::sync::atomic::Ordering::Relaxed));
        assert!(!preparation.task.handle.is_finished());
        assert!(work.urgent);
    }

    #[tokio::test]
    async fn skip_waiting_song_aborts_preparation_and_advances_queue() {
        use crate::models::shared_data_models::Track;
        let state = SharedState::default();
        state.append_tracklist(std::collections::VecDeque::from([
            Track {
                url: "first".into(),
                track: "first".into(),
                ..Default::default()
            },
            Track {
                url: "second".into(),
                track: "second".into(),
                ..Default::default()
            },
        ]));
        let (sink, _output) = rodio::Sink::new_idle();
        let mut work = PlaybackWork {
            preparation: Some(pending_preparation("first")),
            ..Default::default()
        };
        let skip = work
            .preparation
            .as_ref()
            .unwrap()
            .task
            .skip_analysis
            .clone();
        work.next(&state, &sink);
        assert!(work.preparation.is_none());
        assert!(skip.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(state.preparation_track().unwrap().url, "second");
    }

    #[test]
    fn trimmed_pcm_can_be_played_without_an_audio_device() {
        let (sink, mut output) = Sink::new_idle();
        let pcm = LeadingSilence::new(
            SamplesBuffer::new(2, 48_000, vec![0.0, 0.0, 0.5, -0.5, 0.0, 0.0]),
            true,
        );
        sink.append(pcm);
        let samples: Vec<f32> = output.by_ref().take(4).collect();
        assert_eq!(samples, vec![0.5, -0.5, 0.0, 0.0]);
        // An idle sink keeps its output alive with silence after the source ends.
        let _ = output.next();
        assert!(sink.empty());
    }

    #[tokio::test]
    async fn playback_wakes_immediately_for_commands() {
        let (sender, receiver) = async_channel::unbounded();
        sender.send(Command::Next).await.unwrap();
        let mut work = PlaybackWork::default();
        let state = SharedState::default();
        assert!(matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                work.next_event(&receiver, &state)
            )
            .await
            .expect("a queued command should not wait for the playback timer")
            .unwrap(),
            PlaybackEvent::Command(Command::Next)
        ));
    }

    #[tokio::test]
    async fn playback_stays_asleep_when_idle() {
        let (_sender, receiver) = async_channel::unbounded();
        let mut work = PlaybackWork::default();
        assert!(tokio::time::timeout(
            Duration::from_millis(300),
            work.next_event(&receiver, &SharedState::default())
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn finished_source_wakes_control_loop_without_polling() {
        let (_sender, receiver) = async_channel::unbounded();
        let state = SharedState::default();
        let (sink, mut output) = Sink::new_idle();
        let (source, finished) = NotifyOnEnd::new(SamplesBuffer::new(1, 48_000, vec![0.5; 10]));
        sink.append(source);
        let mut work = PlaybackWork {
            finished: Some(finished),
            ..Default::default()
        };
        for _ in 0..11 {
            output.next();
        }
        assert!(matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                work.next_event(&receiver, &state)
            )
            .await
            .unwrap()
            .unwrap(),
            PlaybackEvent::Finished
        ));
        assert!(sink.empty());
    }

    #[tokio::test]
    async fn quitting_with_no_active_song_does_not_wait() {
        let (sink, _output) = Sink::new_idle();
        let (_sender, receiver) = async_channel::unbounded();
        tokio::time::timeout(
            Duration::from_millis(200),
            wait_for_quit_or_cancel(&sink, &receiver, &mut None),
        )
        .await
        .expect("an empty sink should quit immediately");
    }

    #[tokio::test]
    async fn quitting_waits_for_song_and_resumes_paused_playback() {
        let (sink, mut output) = Sink::new_idle();
        let (_sender, receiver) = async_channel::unbounded();
        let (source, finished) = NotifyOnEnd::new(SamplesBuffer::new(1, 48_000, vec![0_f32; 100]));
        let mut finished = Some(finished);
        sink.append(source);
        sink.pause();

        let consumer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            for _ in 0..101 {
                output.next();
            }
        });
        let mut waiting = Box::pin(wait_for_quit_or_cancel(&sink, &receiver, &mut finished));
        assert!(
            tokio::time::timeout(Duration::from_millis(75), &mut waiting)
                .await
                .is_err()
        );
        assert!(tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .expect("quitting should complete when the source finishes"));
        assert!(!sink.is_paused());
        consumer.join().unwrap();
    }

    #[tokio::test]
    async fn esc_cancels_quit_without_stopping_the_song() {
        let (sink, _output) = Sink::new_idle();
        let (source, finished) = NotifyOnEnd::new(SamplesBuffer::new(1, 48_000, vec![0_f32; 100]));
        let mut finished = Some(finished);
        sink.append(source);
        let (sender, receiver) = async_channel::unbounded();
        sender.send(Command::CancelQuit).await.unwrap();
        assert!(!wait_for_quit_or_cancel(&sink, &receiver, &mut finished).await);
        assert!(!sink.empty());
    }

    #[tokio::test]
    async fn esc_restores_pause_after_cancelling_quit() {
        let (sink, _output) = Sink::new_idle();
        let (source, finished) = NotifyOnEnd::new(SamplesBuffer::new(1, 48_000, vec![0_f32; 100]));
        let mut finished = Some(finished);
        sink.append(source);
        sink.pause();
        let (sender, receiver) = async_channel::unbounded();
        sender.send(Command::CancelQuit).await.unwrap();
        assert!(!wait_for_quit_or_cancel(&sink, &receiver, &mut finished).await);
        assert!(sink.is_paused());
        assert!(!sink.empty());
    }
}

async fn search(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();
    let search_str = state.show_input_panel()?;

    if search_str.is_some() {
        state.search(search_str).await?;
    }
    Ok(())
}

async fn favorite_search(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    state.search(None).await
}

pub(crate) async fn ai_playlist(state: &SharedState, during_playback: bool) -> Result<bool> {
    let _screen = during_playback.then(|| state.input_gate.resume_after_screen());
    if during_playback {
        disable_tick_on_screen();
    }
    // Restore the progress bar even if opening or reading the editor fails.
    let _dest = during_playback.then_some(Dest());
    let title = match crate::libbc::ai_profiles::load() {
        Ok(profiles) => profiles.input_title(),
        Err(_) => {
            "? describe an AI playlist (configuration unavailable; Enter: generate, Esc: cancel)"
                .into()
        }
    };
    let Some(description) = input_panel(&title)? else {
        return Ok(false);
    };
    if during_playback {
        enable_tick_on_screen();
    }
    let status = crate::libbc::progress_bar::GenerationStatus::new(during_playback);
    // Preserve the current song and queue on network, parsing or search errors.
    let result = ai::generate_playlist(&description, &[], &state.recent_songs()).await;
    drop(status);
    match result {
        Ok((tracks, terms)) => {
            if state.start_ai_playlist(description, terms, tracks) {
                Ok(true)
            } else {
                crate::libbc::terminal::print_error(
                    "No new playable tracks outside the 60-minute window",
                );
                Ok(false)
            }
        }
        Err(e) => {
            crate::libbc::terminal::print_error(format!("{e:#}"));
            Ok(false)
        }
    }
}

fn options(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();
    match crate::libbc::options::show() {
        Err(e)
            if matches!(
                e.downcast_ref::<inquire::InquireError>(),
                Some(inquire::InquireError::OperationInterrupted)
            ) =>
        {
            Err(e)
        }
        Err(e) => {
            crate::libbc::terminal::print_error(format!("{e:#}"));
            Ok(())
        }
        Ok(()) => Ok(()),
    }
}

fn help(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();
    show_alt_term(
        &about()
            .split('\n')
            .map(|x| x.to_string())
            .collect::<Vec<_>>(),
        None,
    )?;

    Ok(())
}

async fn info(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();

    let v = state.track_info()?;

    match state.get_current_art_id() {
        Some(art_id) => {
            let url = format!("https://f4.bcbits.com/img/a{}_16.jpg", art_id);
            let img = get_request(&url).await?;
            show_alt_term(&v, Option::from(img))?;
        }
        None => show_alt_term(&v, None)?,
    }

    Ok(())
}

async fn menu(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();
    state.top_menu().await?;
    Ok(())
}

fn playlist(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();
    let mut v = vec![format!(
        "{:>2} {:30} {:>7} {:30} {}",
        "#", "Track", "Time", "Artist", "Artist by Album"
    )];
    let _width = 30;
    v.extend(
        state
            .get_tracklist()
            .iter()
            .enumerate()
            .take(12)
            .map(|(n, x)| format(n + 1, x))
            .collect::<Vec<String>>(),
    );

    match show_alt_term2(&v)? {
        None => {}
        Some(l) => state.drain_tracklist(l),
    }

    Ok(())
}

fn history(state: &SharedState) -> Result<()> {
    let _screen = state.input_gate.resume_after_screen();
    let _dest = Dest();
    disable_tick_on_screen();
    let mut lines =
        vec!["Playback history (newest first; j/k to scroll, Esc to close)".to_string()];
    let songs = state.history();
    if songs.is_empty() {
        lines.push("No songs played yet.".to_string());
    }
    lines.extend(songs.iter().map(|song| {
        format!(
            "{}  {} — {} ({})",
            song.play_date.format("%Y-%m-%d %H:%M:%S"),
            song.artist_name,
            song.track,
            song.album_title
        )
    }));
    show_alt_term2(&lines)?;
    Ok(())
}

struct Dest();
impl Drop for Dest {
    fn drop(&mut self) {
        enable_tick_on_screen();
    }
}
