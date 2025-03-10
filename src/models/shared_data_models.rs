use std::collections::VecDeque;
use std::marker::PhantomData;

use crate::models::bc_discover_index::{Element, PostData};
use crate::models::bc_discover_json::Results;
use crate::models::search_models::ItemPage;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

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
    pub results: ResultsJson,
    pub genre: Option<String>,
    pub subgenre: Option<String>,
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
            genre: self.genre.clone(),
            subgenre: self.subgenre.clone(),
        }
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub enum ResultsJson {
    Select(Box<Results>),
    Search(Box<ItemPage>),
    #[default]
    None,
}

#[derive(Default, Debug)]
pub struct CurrentTrack {
    pub duration: f32,
    pub track: String,
    pub art_id: Option<i64>,
    pub band_id: i64,
    pub album_title: String,
    pub artist_name: String,
    pub genre_text: String,
    pub play_date: DateTime<Local>,
    pub results: ResultsJson,
    pub genre: Option<String>,
    pub subgenre: Option<String>,
}

impl Clone for CurrentTrack {
    fn clone(&self) -> CurrentTrack {
        CurrentTrack {
            duration: self.duration,
            track: self.track.clone(),
            art_id: self.art_id,
            band_id: self.band_id,
            album_title: self.album_title.clone(),
            artist_name: self.artist_name.clone(),
            genre_text: self.genre_text.clone(),
            play_date: self.play_date,
            results: self.results.clone(),
            genre: self.genre.clone(),
            subgenre: self.subgenre.clone(),
        }
    }
}

#[derive(Default, Debug)]
pub struct State {
    pub player: PlaylistInfo,
    #[allow(unused)]
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
