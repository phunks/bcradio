use std::time::Duration;

use anyhow::{Error, Result};
use async_channel::Receiver;

use crate::libbc::ai;
use crate::libbc::args::{about, args_genre, args_list_devices, args_sub_genre};
use crate::libbc::command::Command;
use crate::libbc::http_client::get_request;
use crate::libbc::playlist::{format, PlayList, Selection};
use crate::libbc::progress_bar::{
    disable_tick, disable_tick_on_screen, enable_tick, enable_tick_on_screen, run,
    set_quit_pending, update_song_info_on_screen,
};
use crate::libbc::search::{input_panel, Search};
use crate::libbc::shared_data::SharedState;
use crate::libbc::sink::{list_host_devices, Mp3, MusicStruct};
use crate::libbc::terminal::{show_alt_term, show_alt_term2};
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

// Check for a finished track periodically without waking the playback task 20 times a second.
// Incoming commands still wake it immediately.
async fn next_command(commands: &Receiver<Command>) -> Result<Option<Command>> {
    match tokio::time::timeout(Duration::from_millis(250), commands.recv()).await {
        Ok(command) => Ok(Some(command?)),
        Err(_) => Ok(None),
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

        loop {
            if sink.empty() || state.is_ai_playlist() {
                state.fill_playlist().await?;
            }

            state.enqueue_truck_buffer().await?;

            play(&state, &sink).await?;

            // Wake immediately on a command, but continue advancing playback while idle.
            if let Some(res) = next_command(&commands).await? {
                match res {
                    Command::Volume(volume) => {
                        // change volume
                        _current_volume = volume;
                        sink.set_volume(map_volume_to_rodio_volume(_current_volume));
                    }
                    Command::Next => sink.stop(),
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
                    Command::Menu => menu(&state).await?,
                    Command::Playlist => {
                        state.fill_playlist().await?;
                        playlist(&state)?
                    }
                    Command::History => history(&state)?,
                    Command::FavoriteSearch => state.search(None).await?,
                    Command::Search => search(&state).await?,
                    Command::AiPlaylist => {
                        ai_playlist(&state, true).await?;
                    }
                    Command::Help => help(&state)?,
                    Command::Quit => {
                        let _quit_display = QuitDisplay::new();
                        if wait_for_quit_or_cancel(&sink, &commands).await {
                            break;
                        }
                    }
                    Command::CancelQuit => {}
                    Command::Interrupt => break,
                }
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
async fn wait_for_quit_or_cancel(sink: &Sink, commands: &Receiver<Command>) -> bool {
    if sink.empty() {
        return true;
    }
    let was_paused = sink.is_paused();
    if was_paused {
        sink.play();
        enable_tick();
    }
    loop {
        if sink.empty() {
            return true;
        }
        match tokio::time::timeout(Duration::from_millis(50), commands.recv()).await {
            Ok(Ok(Command::CancelQuit)) => {
                if was_paused {
                    sink.pause();
                    disable_tick();
                }
                return false;
            }
            Ok(Err(_)) => tokio::time::sleep(Duration::from_millis(50)).await,
            _ => {} // Ignore other commands while waiting for the song to finish.
        }
    }
}

async fn play(state: &SharedState, sink: &Sink) -> Result<()> {
    if sink.empty() {
        let Some(buf) = state.take_ready_track() else {
            return Ok(());
        };

        match Mp3::load(buf)?.symphonia_decoder().await {
            Ok(mp3) => {
                state.record_playback_start();
                update_song_info_on_screen(&state.get_current_track_info())?;
                sink.append(mp3);
            }
            Err(e) => println!("skip: Decode Error {:?}", e),
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    #[tokio::test]
    async fn playback_wakes_immediately_for_commands() {
        let (sender, receiver) = async_channel::unbounded();
        sender.send(Command::Next).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(100), next_command(&receiver))
                .await
                .expect("a queued command should not wait for the playback timer")
                .unwrap(),
            Some(Command::Next)
        );
    }

    #[tokio::test]
    async fn playback_times_out_when_idle() {
        let (_sender, receiver) = async_channel::unbounded();
        assert_eq!(next_command(&receiver).await.unwrap(), None);
    }

    #[tokio::test]
    async fn quitting_with_no_active_song_does_not_wait() {
        let (sink, _output) = Sink::new_idle();
        let (_sender, receiver) = async_channel::unbounded();
        tokio::time::timeout(
            Duration::from_millis(200),
            wait_for_quit_or_cancel(&sink, &receiver),
        )
        .await
        .expect("an empty sink should quit immediately");
    }

    #[tokio::test]
    async fn quitting_waits_for_song_and_resumes_paused_playback() {
        let (sink, mut output) = Sink::new_idle();
        let (_sender, receiver) = async_channel::unbounded();
        sink.append(SamplesBuffer::new(1, 48_000, vec![0_f32; 100]));
        sink.pause();

        let consumer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            for _ in 0..101 {
                output.next();
            }
        });
        let mut waiting = Box::pin(wait_for_quit_or_cancel(&sink, &receiver));
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
        sink.append(SamplesBuffer::new(1, 48_000, vec![0_f32; 100]));
        let (sender, receiver) = async_channel::unbounded();
        sender.send(Command::CancelQuit).await.unwrap();
        assert!(!wait_for_quit_or_cancel(&sink, &receiver).await);
        assert!(!sink.empty());
    }

    #[tokio::test]
    async fn esc_restores_pause_after_cancelling_quit() {
        let (sink, _output) = Sink::new_idle();
        sink.append(SamplesBuffer::new(1, 48_000, vec![0_f32; 100]));
        sink.pause();
        let (sender, receiver) = async_channel::unbounded();
        sender.send(Command::CancelQuit).await.unwrap();
        assert!(!wait_for_quit_or_cancel(&sink, &receiver).await);
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

pub(crate) async fn ai_playlist(state: &SharedState, during_playback: bool) -> Result<bool> {
    let _screen = during_playback.then(|| state.input_gate.resume_after_screen());
    if during_playback {
        disable_tick_on_screen();
    }
    // Restore the progress bar even if opening or reading the editor fails.
    let _dest = during_playback.then_some(Dest());
    let Some(description) =
        input_panel("? describe an AI playlist (Enter: generate, Esc: cancel)")?
    else {
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
