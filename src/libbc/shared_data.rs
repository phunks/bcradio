use std::clone::Clone;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use tokio::io::copy;
use futures::StreamExt;
use std::iter::Iterator;
use std::marker::PhantomData;
use chrono::Local;
use http::header;
use reqwest::Response;
use serde::Serialize;
use anyhow::Result;
use crate::debug_println;
use crate::libbc::progress_bar::{Bar, Progress};
pub use crate::models::shared_data_models::{State, Track, CurrentTrack};

#[derive(Debug)]
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
    pub fn new() -> Self {
        SharedState {
            state: Arc::new(Mutex::new(State::new())),
            bar: Bar::new(),
            phantom: Default::default(),
        }
    }

    pub fn get_queue_length_from_truck_list(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player.tracks.len()
    }

    pub async fn enqueue_truck_buffer(&self) -> Result<()> {
        let l = self.get_queue_length_from_truck_list();
        if 0 < l { // add audio buffer
            for i in 0..l {
                if ! self.exists_track_buffer(0) {
                    let url = self.get_track_url(i);
                    let mut stream = get_request(url).await?.bytes_stream();
                    let mut buf: Vec<u8> = Vec::new();

                    while let Some(resp) = stream.next().await {
                        copy(&mut resp?.as_ref(), &mut buf).await?;
                    };
                    self.set_track_buffer(i, buf);
                }
                if i > 1 { break }
            }
        }
        Ok(())
    }

    pub fn set_random_pagination(&self, mut pages: VecDeque<usize>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.rnd_pages.append(&mut pages);
    }

    pub fn get_rnd_pages(&self, pos: usize) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player.rnd_pages[pos]
    }

    pub fn get_random_page_list_length(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player.rnd_pages.len()
    }

    pub fn drain_rnd_page_list_element(&self, i: usize) {
        let mut lock = self.state.lock().unwrap();
        lock.player.rnd_pages.drain(..i);
    }

    pub fn append_tracklist(&self, mut playlist: VecDeque<Track>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.append(&mut playlist);
    }

    pub fn push_front_tracklist(&self, playlist: Track) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.push_front(playlist);
    }

    pub fn set_total_count(&self, total_count: i64) {
        let mut lock = self.state.lock().unwrap();
        lock.player.total_count = total_count;
    }

    pub fn get_buffer_set_queue_length(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player.tracks.iter().filter(|&n| !n.buffer.is_empty()).count()
    }

    pub fn set_track_buffer(&self, pos: usize, buf: Vec<u8>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks[pos].buffer = buf;
    }

    pub fn exists_track_buffer(&self, pos: usize) -> bool {
        let lock = self.state.lock().unwrap();
        !lock.player.tracks[pos].buffer.is_empty()
    }

    pub fn get_track_buffer(&self, pos: usize) -> Vec<u8> {
        let lock = self.state.lock().unwrap();
        lock.player.tracks[pos].buffer.clone()
    }

    pub fn get_track_url(&self, pos: usize) -> String {
        let lock = self.state.lock().unwrap();
        lock.player.tracks[pos].url.clone()
    }

    pub fn set_selected_url(&self, url: String) {
        let mut lock = self.state.lock().unwrap();
        lock.server.select_url.push_front(url);
    }

    pub fn get_selected_url(&self) -> String {
        let lock = self.state.lock().unwrap();
        lock.server.select_url[0].clone()
    }

    pub fn get_selected_url_list_length(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.server.select_url.len()
    }

    pub fn move_to_current_track(&self) {
        let mut lock = self.state.lock().unwrap();
        let track = lock.player.tracks.pop_front().unwrap();
        lock.player.current_track.duration    = track.duration;
        lock.player.current_track.track       = track.track;
        lock.player.current_track.album_title = track.album_title;
        lock.player.current_track.band_id     = track.band_id;
        lock.player.current_track.artist_name = track.artist_name;
        lock.player.current_track.genre_text  = track.genre_text;
        lock.player.current_track.play_date   = Local::now();
    }

    pub fn get_current_track_info(&self) -> CurrentTrack {
        let lock = self.state.lock().unwrap();
        lock.player.current_track.clone()
    }
}

pub async fn get_request(url: String) -> Result<Response> {
    debug_println!("debug: request_url {}", url);
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100")
        .gzip(true)
        .build()?;
    let buf = client
        .get(url.as_str())
        .send();
    Ok(buf.await.expect("error reqwest"))
}

pub async fn post_request<T>(url: String, post_data: T) -> Result<Response>
    where
        T: Serialize,
{
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", header::HeaderValue::from_static("application/json; charset=utf-8"));

    debug_println!("debug: request_url_post {}", url);
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100")
        .gzip(true)
        .default_headers(headers)
        .build()?;
    let buf = client
        .post(url.as_str())
        .json(&post_data)
        .send();
    Ok(buf.await.expect("error reqwest"))
}

#[cfg(test)]
mod tests {
    use tokio::runtime::Runtime;
    use crate::models::search_models::SearchJsonRequest;
    use super::*;
    fn runtime() -> &'static Runtime {
        static RUNTIME: once_cell::sync::OnceCell<Runtime> = once_cell::sync::OnceCell::new();
        RUNTIME.get_or_init(|| tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap())
    }

    #[test]
    fn test_get_request() {
        runtime().block_on(async {
            let url = String::from("http://localhost:8080");
            assert_eq!(
                get_request(url).await.status(),
                200
            );
        });
    }

    #[test]
    fn test_post_request() {
        runtime().block_on(async {
            let request = String::from("test");
            let url = String::from("http://localhost:8080/api/bcsearch_public_api/1/autocomplete_elastic");
            let search_json_req = SearchJsonRequest {
                search_text: request.clone(),
                search_filter: String::from("t"),
                full_page: false,
                fan_id: None,
            };
            assert_eq!(post_request(url, search_json_req).await.status(), 200);
        });
    }
}