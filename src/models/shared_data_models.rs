use std::collections::VecDeque;
use std::marker::PhantomData;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use crate::models::bc_discover_index::{Element, PostData};
use crate::models::bc_discover_json::Results;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Track {
    pub album_title: String,
    pub artist_name: String,
    pub art_id: Option<i64>,
    pub band_id: i64,
    pub url: String,
    pub duration: f32,
    pub track: String,
    pub buffer: Vec<u8>,
    pub results: Option<Results>,
}

impl Clone for Track {
    fn clone(&self) -> Track {
        Track {
            album_title: self.album_title.clone(),
            artist_name: self.artist_name.clone(),
            art_id: self.art_id,
            band_id: self.band_id,
            url: self.url.clone(),
            duration: self.duration,
            track: self.track.clone(),
            buffer: self.buffer.clone(),
            results: self.results.clone(),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Link {
    pub subdomain: String,
    pub slug: String,
}

#[derive(Default, Debug)]
pub struct CurrentTrack {
    pub duration: f32,
    pub track: String,
    pub band_id: i64,
    pub album_title: String,
    pub artist_name: String,
    pub genre_text: String,
    pub play_date: DateTime<Local>,
    pub results: Option<Results>,
}

impl Clone for CurrentTrack {
    fn clone(&self) -> CurrentTrack {
        CurrentTrack {
            duration: self.duration,
            track: self.track.clone(),
            band_id: self.band_id,
            album_title: self.album_title.clone(),
            artist_name: self.artist_name.clone(),
            genre_text: self.genre_text.clone(),
            play_date: self.play_date,
            results: self.results.clone(),
        }
    }
}

#[derive(Default, Debug)]
pub struct State {
    pub player: PlaylistInfo,
    pub server: ServerInfo,
    phantom: PhantomData<&'static ()>,
}

#[derive(Default, Debug)]
pub struct ServerInfo {
    pub select_url: VecDeque<String>,
    top_page: String,
}

impl Clone for ServerInfo {
    fn clone(&self) -> Self {
        Self {
            select_url: self.select_url.clone(),
            top_page: self.top_page.clone(),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct PlaylistInfo {
    pub current_track: CurrentTrack,
    pub tracks: VecDeque<Track>,
    pub post_data: PostData,
    pub genres: Vec<Element>,
    pub subgenres: Vec<Element>,
    pub genre: String,
    pub subgenre: String,
}
