
use std::clone::Clone;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::iter::Iterator;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use curl::multi::Easy2Handle;

use crate::debug_println;
use crate::libbc::http_adapter::http_adapter;
use crate::models::bc_discover_index::{Element, PostData};
use crate::libbc::progress_bar::{Bar, Progress};
use crate::libbc::http_client::Collector;
use crate::libbc::search::is_mp3;
use crate::models::shared_data_models::{CurrentTrack, State, Track};

#[derive(Default, Debug)]
pub struct SharedState {
    pub state: Arc<Mutex<State>>,
    pub bar: Bar<'static>,
    phantom: PhantomData<&'static ()>,
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
            let i = 0;
            if !self.exists_track_buffer(0) {
                self.bar.enable_spinner();
                let url = self.get_track_url(i);

                let buf = match http_adapter(vec!(url), response_filter, mp3_output) {
                    Ok(v) => {
                        v.first().unwrap().clone().unwrap()
                    },
                    Err(e) => return Err(e),
                };

                use std::time::Instant; //debug
                let _start = Instant::now(); //debug
                let duration = mp3_duration::from_read(&mut io::Cursor::new(buf.clone())).unwrap();
                debug_println!("Debug mp3: {:?}\r", _start.elapsed()); //debug
                debug_println!("{:?}\r", duration);
                self.set_track_buffer(i, buf, duration);
                self.bar.disable_spinner();
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

    pub fn save_genres(&self, genres: Vec<Element>, subgenres: Vec<Element>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.genres = genres;
        lock.player.subgenres = subgenres;
    }

    pub fn get_genres(&self) -> (Vec<Element>, Vec<Element>) {
        let lock = self.state.lock().unwrap();
        (lock.player.genres.to_owned(), lock.player.subgenres.to_owned())
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

fn response_filter(handle: &mut Easy2Handle<Collector<Option<Vec<u8>>>>) {
    match handle.response_code() {
        Ok(200) => handle.get_mut().dat = vec!(Option::from(handle.get_ref().res.clone())),
        _ => handle.get_mut().dat = vec!(None),
    }
}

fn mp3_output(handles: HashMap<usize, Easy2Handle<Collector<Option<Vec<u8>>>>>) -> Vec<Option<Vec<u8>>> {
    let mut v = Vec::<Option<Vec<u8>>>::new();
    handles.iter().for_each(|x|{
        x.1.get_ref().dat.iter().for_each(|x|
            if let Some(aa) = x.clone() {
                match is_mp3(aa) {
                    true => v.append(&mut vec![x.clone()]),
                    false => {},
                }
            }
        )
    });
    v
}