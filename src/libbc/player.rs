
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_channel::{Receiver, Sender, unbounded};
use async_trait::async_trait;
use futures::future::abortable;
use lazy_static::lazy_static;
use rodio::Sink;

use crate::{ceil, format_duration};
use crate::libbc::args::{about};
use crate::libbc::http_client::Client;
use crate::libbc::playlist::PlayList;
use crate::libbc::progress_bar::Progress;
use crate::libbc::search::Search;
use crate::libbc::shared_data::SharedState;
use crate::libbc::sink::{Mp3, MusicStruct};
use crate::libbc::terminal::{quit, show_alt_term};
use crate::models::shared_data_models::ResultsJson;


fn map_volume_to_rodio_volume(volume: u8) -> f32 {
    (volume as f32 / 9_f32).powf(2.0)
}

lazy_static! {
    pub static ref RXTX: (Sender<char>, Receiver<char>) = unbounded();
    pub static ref PARK: Mutex<bool> = Mutex::new(false);
    pub static ref PROG: Mutex<bool> = Mutex::new(true);
}

#[async_trait]
pub trait Player<'a>: Send + Sync + 'static {
    async fn player_thread() -> Result<()>;
    fn track_info(&self) -> Result<Vec<String>>;
}

#[async_trait]
impl Player<'static> for SharedState {
    async fn player_thread() -> Result<()> {
        let state: SharedState = SharedState::default();

        let progress = state.to_owned();
        let (_, hdl0) = abortable(progress.bar.run().await);

        match state.ask() {
            Ok(post_data)
                => state.store_results(post_data).unwrap(),
            Err(e)
                => quit(e),
        };
        *PARK.lock().unwrap() = true;
        let mut _current_volume = 9;

        let stream_handle = MusicStruct::new();
        let sink = Sink::try_new(&stream_handle.stream_handle.unwrap())?;

        loop {
            if sink.empty() {
                state.fill_playlist()?;
            }
            state.enqueue_truck_buffer().await?;
            play(&state, &sink).await?;

            if let Ok(res) = RXTX.deref().1.try_recv() {
                match res {
                    '0'..='9' => { // change volume
                        _current_volume = res.to_string().parse()?;
                        sink.set_volume(map_volume_to_rodio_volume(_current_volume));
                    }
                    'n' => { sink.stop() }
                    'p' => { // play pause
                        if sink.is_paused() {
                            sink.play();
                            state.bar.enable_tick();
                        } else {
                            sink.pause();
                            state.bar.disable_tick();
                        }
                    }
                    'i' => { info(&state)? }
                    'm' => { menu(&state)? }
                    'f' => { state.search(None)? }
                    's' => { search(&state).await? }
                    'h' => { help(&state)? }
                    'Q' => { break; }
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
                if state.bar.get_progress_bar_current_position().is_zero() {
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
        v.append(&mut vec![format!(" {:>14} {}", "Artist:", current_track.artist_name)]);
        v.append(&mut vec![format!(" {:>14} {}", "Album:",  current_track.album_title)]);
        v.append(&mut vec![format!(" {:>14} {}", "Song:",   current_track.track)]);
        v.append(&mut vec![format!(" {:>14} {}", "Duration:",
                                 format_duration!(ceil!(current_track.clone().duration, 1.0) as u32))]);

        match current_track.results {
            ResultsJson::Select(g) => {
                let genres = self.get_genres().0;
                let genre = genres
                    .iter()
                    .find(|&x| x.id == g.band_genre_id as i64).cloned();

                v.append(&mut vec![format!(" {:>14} {}", "Category:", genre.unwrap().label)]);
                v.append(&mut vec![format!(" {:>14} {}", "Genre:",    current_track.genre.clone().unwrap_or_default())]);
                v.append(&mut vec![format!(" {:>14} {}", "Subgenre:", current_track.subgenre.clone().unwrap_or_default())]);
                v.append(&mut vec![format!(" {:>14} {} {:3.2}", "Item Price:",
                                                                            g.item_currency,
                                                                            g.item_price )]);
                v.append(&mut vec![format!(" {:>14} {}", "Labels:",   g.label_name.unwrap_or_default())]);
                v.append(&mut vec![format!(" {:>14} {}", "Location:", g.band_location.unwrap_or_default())]);
                v.append(&mut vec![format!(" {:>14} {}", "Release Date:", g.release_date)]);
                v.append(&mut vec![format!(" {:>14} {}", "Label URL:", g.label_url.unwrap_or_default())]);
                v.append(&mut vec![format!(" {:>14} {}", "Band URL:",  g.band_url)]);
                v.append(&mut vec![format!(" {:>14} {}", "Item URL:",  g.item_url)]);
            },
            ResultsJson::Search(g) => {
                v.append(&mut vec![format!(" {:>14} {}", "Release Date:", g.current.release_date)]);
                v.append(&mut vec![format!(" {:>14} {}", "Album URL:",  g.album_url.unwrap_or_default())]);
                v.append(&mut vec![format!(" {:>14} {}", "Item URL:",  g.item_url.unwrap_or_default())]);
            },
            ResultsJson::None => {},
        }

        Ok(v)
    }
}


async fn play(state: &SharedState, sink: &Sink) -> Result<()> {
    if sink.empty() && state.get_buffer_set_queue_length() > 0 {
        let buf = state.get_track_buffer(0);
        state.move_to_current_track();
        state
            .bar
            .update_song_info_on_screen(state.get_current_track_info())?;

        match Mp3::load(buf)?.symphonia_decoder().await {
            Ok(mp3) => sink.append(mp3),
            Err(e) => println!("skip: Decode Error {:?}", e),
        }
    };
    Ok(())
}

async fn search(state: &SharedState) -> Result<()> {
    state.bar.disable_tick_on_screen();
    let search_str = state.show_input_panel()?;
    state.bar.enable_tick_on_screen();

    *PARK.lock().unwrap() = true;
    if search_str.is_some() {
        state.search(search_str)?;
    }
    Ok(())
}

fn help(state: &SharedState) -> Result<()> {
    state.bar.disable_tick_on_screen();
    show_alt_term(about().split('\n')
                      .collect::<Vec<&str>>(), None)?;

    state.bar.enable_tick_on_screen();
    *PARK.lock().unwrap() = true;
    Ok(())
}

fn info(state: &SharedState) -> Result<()> {
    state.bar.disable_tick_on_screen();

    let v = state.track_info()?;

    match state.get_current_art_id() {
        Some(art_id) => {
            let url = format!("https://f4.bcbits.com/img/a{}_16.jpg", art_id);
            let client: Client = Default::default();
            let img = client.get_curl_request(url).unwrap().vec()?;
            show_alt_term(v, Option::from(img))?;
        },
        None => show_alt_term(v, None)?
    }

    state.bar.enable_tick_on_screen();
    *PARK.lock().unwrap() = true;
    Ok(())
}

fn menu(state: &SharedState) -> Result<()> {
    state.bar.disable_tick_on_screen();
    state.top_menu()?;
    state.bar.enable_tick_on_screen();

    *PARK.lock().unwrap() = true;
    Ok(())
}