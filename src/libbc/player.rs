
use std::sync::Mutex;
use std::time::Duration;
use futures::future::abortable;
use rodio::Sink;
use async_trait::async_trait;
use anyhow::Result;
use async_channel::{Receiver, Sender, unbounded};
use lazy_static::lazy_static;
use crate::libbc::shared_data::SharedState;
use crate::libbc::progress_bar::Progress;
use crate::libbc::playlist::PlayList;
use crate::libbc::search::Search;
use crate::libbc::sink::{MusicStruct, Mp3};
use std::ops::Deref;
use crate::libbc::args;


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
}

#[async_trait]
impl Player<'static> for SharedState {
    async fn player_thread() -> Result<()>
    {
        let state: SharedState = SharedState::new();

        let progress = state.clone();
        let (_, hdl0) = abortable(progress.bar.run().await);
        let url = state.ask_url().await;
        state.set_selected_url(url?);
        let mut current_volume = 9;

        let stream_handle = MusicStruct::new();
        let sink = Sink::try_new(&stream_handle.stream_handle.unwrap())?;

        loop {
            state.generate_playlist_url().await?;
            state.enqueue_truck_buffer().await?;

            if sink.empty() && state.get_buffer_set_queue_length() > 0 {
                let buf = state.get_track_buffer(0);
                state.move_to_current_track();
                state.bar.update_song_info_on_screen(state.get_current_track_info())?;
                match Mp3::load(buf)?.decoder() {
                    Ok(mp3) => {
                        sink.append(mp3) },
                    Err(e) => {
                        println!("skip: Decode Error {:?}", e) }
                };
                *PARK.lock().unwrap() = true;
            }

            if let Ok(res) = RXTX.deref().1.try_recv() {
                match res {
                    '0'..='9' => { // volume
                        current_volume = res.to_string().parse()?;
                        sink.set_volume(map_volume_to_rodio_volume(current_volume));
                    }
                    'n' => { // play next
                        sink.stop();
                    }
                    'p' => { // play pause
                        if sink.is_paused() {
                            sink.play();
                            state.bar.enable_tick();
                        } else {
                            sink.pause();
                            state.bar.disable_tick();
                        }
                    }
                    'f' => { // favorite search
                        state.search("t", None).await?;
                    }
                    's' => { // free word search
                        state.bar.disable_tick_on_screen();
                        let search_str = state.show_input_panel()?;
                        state.bar.enable_tick_on_screen();
                        *PARK.lock().unwrap() = true;
                        if ! search_str.is_empty() {
                            state.search("a",
                                         Option::from(search_str)).await?;
                        }
                    }
                    'h' => {
                        state.bar.disable_tick_on_screen();
                        let _ = args::show_help()?;
                        state.bar.enable_tick_on_screen();
                        *PARK.lock().unwrap() = true;
                    }
                    'Q' => { // graceful kill
                        break;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        /// till the end
        let hdl1 = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                if state.bar.get_progress_bar_current_position().is_zero() { break };
            }
        });
        if let _ = tokio::join!(hdl1) {
            hdl0.abort()
        }
        Ok(())
    }
}

