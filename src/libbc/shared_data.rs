use anyhow::Result;
use chrono::Local;
use futures::future::{abortable, AbortHandle};
use std::clone::Clone;
use std::collections::VecDeque;
use std::io;
use std::iter::Iterator;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use log::info;
use crate::libbc::http_client::get_request;
use crate::libbc::progress_bar::{disable_spinner, enable_spinner};
use crate::libbc::terminal;
use crate::models::bc_discover_index::{Element, PostData};
use crate::models::shared_data_models::{CurrentTrack, State, Track};

pub static ENQUE_FLG: AtomicBool = AtomicBool::new(true);

#[derive(Default, Debug)]
pub struct SharedState {
    pub state: Arc<Mutex<State>>,
    pub task: Arc<Mutex<Vec<AbortHandle>>>,
    phantom: PhantomData<&'static ()>,
}

impl Clone for SharedState {
    fn clone(&self) -> Self {
        SharedState {
            state: Arc::clone(&self.state),
            task: self.task.clone(),
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
        let ss = self.clone();

        let l: usize = ss.queue_length_from_truck_list();
        if l > 0 && !ss.exists_track_buffer(0) && ENQUE_FLG.load(Ordering::Relaxed) {
            // add audio buffer
            let i = 0;
            let a = tokio::spawn(async move {
                if ENQUE_FLG
                    .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
                    .unwrap()
                {
                    enable_spinner();
                    let url = &ss.get_track_url(i);
                    match get_request(url).await {
                        Ok(buf) => {
                            let duration =
                            mp3_duration::from_read(&mut io::Cursor::new(buf.clone())).unwrap();
                            ss.set_track_buffer(url, buf, duration);
                        }
                        Err(e) => {
                            terminal::quit(e);
                        }
                    }

                    disable_spinner();
                };
                let _ =
                    ENQUE_FLG.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed);
            });
            let b = abortable(a).1;
            self.append_task(b);
        }
        Ok(())
    }

    fn append_task(&self, hdl: AbortHandle) {
        let mut v = vec![hdl];
        let mut lock = self.task.lock().unwrap();
        lock.iter_mut().for_each(|i| i.abort());
        lock.clear();
        lock.append(&mut v);
    }

    pub fn append_tracklist(&self, mut playlist: VecDeque<Track>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.append(&mut playlist);
    }

    pub fn push_front_tracklist(&self, playlist: Track) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.push_front(playlist);
    }

    #[allow(dead_code)]
    pub fn insert_tracklist(&self, n: usize, playlist: Track) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.insert(n, playlist);
    }

    pub fn clear_all_tracklist(&self) {
        info!("clear_all_tracklist\r");
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.clear();
    }

    pub fn drain_tracklist(&self, l: usize) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.drain(..l - 1);
    }

    pub fn get_tracklist(&self) -> VecDeque<Track> {
        let lock = self.state.lock().unwrap();
        lock.player.tracks.clone()
    }

    pub fn set_next_postdata(&self, post_data: &PostData) {
        let mut lock = self.state.lock().unwrap();
        lock.player.post_data = post_data.clone();
    }

    pub fn save_genres(&self, genres: Vec<Element>, subgenres: Vec<Element>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.genres = genres;
        lock.player.subgenres = subgenres;
    }

    pub fn get_genres(&self) -> (Vec<Element>, Vec<Element>) {
        let lock = self.state.lock().unwrap();
        (
            lock.player.genres.to_owned(),
            lock.player.subgenres.to_owned(),
        )
    }

    pub fn set_genre(&self, genre: &str) {
        let mut lock = self.state.lock().unwrap();
        lock.player.genre = genre.to_owned();
    }
    #[allow(dead_code)]
    pub fn get_genre(&self) -> String {
        let lock = self.state.lock().unwrap();
        lock.player.genre.to_owned()
    }

    pub fn set_subgenre(&self, subgenre: &str) {
        let mut lock = self.state.lock().unwrap();
        lock.player.subgenre = subgenre.to_owned();
    }
    #[allow(dead_code)]
    pub fn get_subgenre(&self) -> String {
        let lock = self.state.lock().unwrap();
        lock.player.subgenre.to_owned()
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

    pub fn set_track_buffer(&self, url: &str, buf: Vec<u8>, duration: Duration) {
        let mut lock = self.state.lock().unwrap();
        match lock.player.tracks.iter().position(|x| x.url == url) {
            None => {}
            Some(pos) => {
                lock.player.tracks[pos].buffer = buf;
                lock.player.tracks[pos].duration = duration.as_secs_f32();
            }
        }
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

    pub fn move_to_current_track(&self) {
        let mut lock = self.state.lock().unwrap();
        let track = lock.player.tracks.pop_front().unwrap();
        lock.player.current_track.duration = track.duration;
        lock.player.current_track.track = track.track;
        lock.player.current_track.album_title = track.album_title;
        lock.player.current_track.art_id = track.art_id;
        lock.player.current_track.band_id = track.band_id;
        lock.player.current_track.artist_name = track.artist_name;
        lock.player.current_track.play_date = Local::now();
        lock.player.current_track.results = track.results;
        lock.player.current_track.genre = track.genre;
        lock.player.current_track.subgenre = track.subgenre;
    }

    pub fn get_current_track_info(&self) -> CurrentTrack {
        let lock = self.state.lock().unwrap();
        lock.player.current_track.to_owned()
    }

    pub fn get_current_art_id(&self) -> Option<i64> {
        let lock = self.state.lock().unwrap();
        lock.player.current_track.art_id
    }
}
