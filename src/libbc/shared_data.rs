
use std::clone::Clone;
use std::collections::VecDeque;
use std::io;
use std::iter::Iterator;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use tokio::io::copy;
use async_std::stream::StreamExt;

use crate::debug_println;
use crate::models::bc_discover_index::{Element, PostData};
use crate::libbc::progress_bar::{Bar, Progress};
use crate::libbc::http_client::get_request;
use crate::models::shared_data_models::{CurrentTrack, State, Track};

#[derive(Default, Debug)]
pub struct SharedState {
    pub state: Arc<Mutex<State>>,
    pub bar: Bar<'static>,
    pub phantom: PhantomData<&'static ()>,
}

impl Clone for SharedState {
    fn clone(&self) -> Self {
        SharedState {
            state: Arc::clone(&self.state),
            bar: self.bar.clone(),
            phantom: Default::default(),
        }
    }
}

impl SharedState {
    pub fn queue_length_from_truck_list(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player.tracks.len()
    }

    pub async fn enqueue_truck_buffer(&self) -> Result<()> {
        let l: usize = self.queue_length_from_truck_list();
        if 0 < l { // add audio buffer
            for i in 0..l {
                if !self.exists_track_buffer(0) {
                    self.bar.enable_spinner();
                    let url = self.get_track_url(i);
                    let mut stream = get_request(url).await?.bytes_stream();
                    let mut buf: Vec<u8> = Vec::new();

                    while let Some(resp) = stream.next().await {
                        copy(&mut resp?.as_ref(), &mut buf).await?;
                    };
                    use std::time::Instant; //debug
                    let start = Instant::now(); //debug
                    let duration = mp3_duration::from_read(&mut io::Cursor::new(buf.clone())).unwrap();
                    debug_println!("Debug mp3: {:?}\r", start.elapsed()); //debug
                    debug_println!("{:?}\r", duration);
                    self.set_track_buffer(i, buf, duration);
                    self.bar.disable_spinner();
                }
                if i > 1 { break; }
            }
        }

        Ok(())
    }

    pub fn append_tracklist(&self, mut playlist: VecDeque<Track>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.append(&mut playlist);
    }

    pub fn push_front_tracklist(&self, playlist: Track) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.push_front(playlist);
    }

    pub fn clear_all_tracklist(&self) {
        debug_println!("clear_all_tracklist\r");
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.clear();
    }

    pub fn set_next_postdata(&self, post_data: PostData) {
        let mut lock = self.state.lock().unwrap();
        lock.player.post_data = post_data;
    }

    pub fn save_genres(&self, genres: Vec<Element>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.genres = genres;
    }

    pub fn get_genres(&self) -> Vec<Element> {
        let lock = self.state.lock().unwrap();
        lock.player.genres.to_owned()
    }

    pub fn next_post(&self) -> PostData {
        self.state.lock().unwrap().player.post_data.to_owned()
    }

    pub fn get_buffer_set_queue_length(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player
            .tracks
            .iter()
            .filter(|&n| !n.buffer.is_empty())
            .count()
    }

    pub fn set_track_buffer(&self, pos: usize, buf: Vec<u8>, duration: Duration) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks[pos].buffer = buf;
        lock.player.tracks[pos].duration = duration.as_secs_f32();
    }

    pub fn exists_track_buffer(&self, pos: usize) -> bool {
        let lock = self.state.lock().unwrap();
        !lock.player.tracks[pos].buffer.is_empty()
    }

    pub fn get_track_buffer(&self, pos: usize) -> Vec<u8> {
        let lock = self.state.lock().unwrap();
        lock.player.tracks[pos].buffer.to_owned()
    }

    pub fn get_track_url(&self, pos: usize) -> String {
        let lock = self.state.lock().unwrap();
        lock.player.tracks[pos].url.to_owned()
    }

    pub fn set_selected_url(&self, url: String) {
        let mut lock = self.state.lock().unwrap();
        lock.server.select_url.push_front(url);
    }

    pub fn get_selected_url(&self) -> String {
        let lock = self.state.lock().unwrap();
        lock.server.select_url[0].to_owned()
    }

    pub fn get_selected_url_list_length(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.server.select_url.len()
    }

    pub fn move_to_current_track(&self) {
        let mut lock = self.state.lock().unwrap();
        let track = lock.player.tracks.pop_front().unwrap();
        lock.player.current_track.duration = track.duration;
        lock.player.current_track.track = track.track;
        lock.player.current_track.album_title = track.album_title;
        lock.player.current_track.band_id = track.band_id;
        lock.player.current_track.artist_name = track.artist_name;
        lock.player.current_track.play_date = Local::now();
        lock.player.current_track.results = track.results;
    }

    pub fn get_current_track_info(&self) -> CurrentTrack {
        let lock = self.state.lock().unwrap();
        lock.player.current_track.to_owned()
    }
}

