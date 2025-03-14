use std::env;
use std::ops::Deref;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Error, Result};
use async_channel::{unbounded, Receiver, Sender};
use async_trait::async_trait;
use futures::future::abortable;

use rodio::Sink;
use crate::libbc::progress_bar::{disable_tick, disable_tick_on_screen, enable_tick, enable_tick_on_screen, get_progress_bar_current_position, run, update_song_info_on_screen};
use crate::libbc::args::{about, args_genre, args_list_devices, args_sub_genre, args_verbose_log};
use crate::libbc::http_client::{get_blocking_request};
use crate::libbc::playlist::{format, PlayList};
use crate::libbc::search::Search;
use crate::libbc::shared_data::SharedState;
use crate::libbc::sink::{list_host_devices, Mp3, MusicStruct};
use crate::libbc::terminal::{quit, show_alt_term, show_alt_term2};
use crate::models::shared_data_models::ResultsJson;
use crate::{ceil, format_duration};
use crate::models::bc_error::BcradioError;

fn map_volume_to_rodio_volume(volume: u8) -> f32 {
    (volume as f32 / 9_f32).powf(2.0)
}

pub static RXTX: LazyLock<(Sender<char>, Receiver<char>)> = LazyLock::new(unbounded);
pub static PARK: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));
pub static PROG: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(true));
pub fn park_lock() { *PARK.lock().unwrap() = false; }
pub fn park_unlock() { *PARK.lock().unwrap() = true; }

#[async_trait]
pub trait Player<'a>: Send + Sync + 'static {
    async fn player_thread() -> Result<()>;
    fn track_info(&self) -> Result<Vec<String>>;
}
#[async_trait]
impl Player<'static> for SharedState {
    async fn player_thread() -> Result<()> {
        let state: SharedState = SharedState::default();

        let (_, hdl0) = abortable(run().await);

        if env::var("RUST_LOG").is_err() {
            let level = format!("bcradio={}", args_verbose_log());

            env::set_var("RUST_LOG", level);
            pretty_env_logger::init_timed();
        }

        if args_list_devices() {
            list_host_devices();
            quit(Error::from(BcradioError::Quit));
        }

        if args_genre().is_some() || args_sub_genre().is_some() {
            let post_data = state.silent(args_genre(), args_sub_genre())?;
            state.store_results(&post_data).await;
        } else {
            match state.ask() {
                Ok(post_data) => state.store_results(&post_data).await,
                Err(e) => quit(e),
            };
        }

        park_unlock();
        let mut _current_volume = 9;

        let stream_handle = MusicStruct::new();
        let sink = Sink::try_new(&stream_handle.stream_handle.unwrap())?;

        loop {
            if sink.empty() {
                state.fill_playlist().await?;
            }

            state.enqueue_truck_buffer().await?;

            play(&state, &sink).await?;

            if let Ok(res) = RXTX.deref().1.try_recv() {
                match res {
                    '0'..='9' => {
                        // change volume
                        _current_volume = res.to_string().parse()?;
                        sink.set_volume(map_volume_to_rodio_volume(_current_volume));
                    }
                    'n' => sink.stop(),
                    'p' => {
                        // play pause
                        if sink.is_paused() {
                            sink.play();
                            enable_tick();
                        } else {
                            sink.pause();
                            disable_tick();
                        }
                    }
                    'i' => info(&state)?,
                    'm' => menu(&state)?,
                    'l' => {
                        state.fill_playlist().await?;
                        playlist(&state)?
                    }
                    'f' => state.search(None).await?,
                    's' => search(&state).await?,
                    'h' => help()?,
                    'Q' => {
                        break;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // till the end
        let hdl1 = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                if get_progress_bar_current_position().is_zero() {
                    break;
                };
            }
        });
        tokio::join!(hdl1).0.is_ok().then(|| hdl0.abort());
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
                    "Item Price:", g.item_currency, g.item_price
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Labels:",
                    g.label_name.unwrap_or_default()
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Location:",
                    g.band_location.unwrap_or_default()
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Release Date:", g.release_date
                )]);
                v.append(&mut vec![format!(
                    " {:>14} {}",
                    "Label URL:",
                    g.label_url.unwrap_or_default()
                )]);
                v.append(&mut vec![format!(" {:>14} {}", "Band URL:", g.band_url)]);
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

async fn play(state: &SharedState, sink: &Sink) -> Result<()> {
    if sink.empty() && state.get_buffer_set_queue_length() > 0 {
        let buf = state.get_track_buffer(0);
        if buf.is_empty() {
            return Ok(());
        }

        state.move_to_current_track();

        update_song_info_on_screen(&state.get_current_track_info())?;

        match Mp3::load(buf)?.symphonia_decoder().await {
            Ok(mp3) => sink.append(mp3),
            Err(e) => println!("skip: Decode Error {:?}", e),
        }
    };
    Ok(())
}

async fn search(state: &SharedState) -> Result<()> {
    disable_tick_on_screen();
    let search_str = state.show_input_panel()?;
    enable_tick_on_screen();

    if search_str.is_some() {
        state.search(search_str).await?;
    }
    park_unlock();
    Ok(())
}

fn help() -> Result<()> {
    let _dest = Dest();
    disable_tick_on_screen();
    show_alt_term(&about().split('\n')
        .map(|x|x.to_string()).collect::<Vec<_>>(), None)?;

    Ok(())
}

fn info(state: &SharedState) -> Result<()> {
    let _dest = Dest();
    disable_tick_on_screen();

    let v = state.track_info()?;

    match state.get_current_art_id() {
        Some(art_id) => {
            let url = format!("https://f4.bcbits.com/img/a{}_16.jpg", art_id);
            let img = get_blocking_request(&url)?;
            show_alt_term(&v, Option::from(img))?;
        }
        None => show_alt_term(&v, None)?,
    }

    Ok(())
}

fn menu(state: &SharedState) -> Result<()> {
    let _dest = Dest();
    disable_tick_on_screen();
    state.top_menu()?;
    Ok(())
}

fn playlist(state: &SharedState) -> Result<()> {
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

struct Dest();
impl Drop for Dest {
    fn drop(&mut self) {
        enable_tick_on_screen();
        park_unlock();
    }
}