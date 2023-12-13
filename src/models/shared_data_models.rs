use std::collections::VecDeque;
use std::marker::PhantomData;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Track {
    pub genre_text: String,
    pub album_title: String,
    pub artist_name: String,
    pub art_id: Option<i64>,
    // pub publish_date: String,
    // pub link: Link,
    // pub location: Option<String>,
    pub band_id: i64,
    pub url: String,
    pub duration: f32,
    pub track: String,
    pub buffer: Vec<u8>,
}

impl Clone for Track {
    fn clone(&self) -> Track {
        Track {
            genre_text: self.genre_text.clone(),
            album_title: self.album_title.clone(),
            artist_name: self.artist_name.clone(),
            art_id: self.art_id,
            // publish_date: self.publish_date.clone(),
            // link: self.link.clone(),
            // location: self.location.clone(),
            band_id: self.band_id,
            url: self.url.clone(),
            duration: self.duration,
            track: self.track.clone(),
            buffer: self.buffer.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Link {
    pub subdomain: String,
    pub slug: String,
}

#[derive(Debug)]
pub struct CurrentTrack {
    pub duration: f32,
    pub track: String,
    pub band_id: i64,
    pub album_title: String,
    pub artist_name: String,
    pub genre_text: String,
    pub play_date: DateTime<Local>,
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
        }
    }
}

#[derive(Debug)]
pub struct State {
    pub player: PlaylistInfo,
    pub server: ServerInfo,
    phantom: PhantomData<&'static ()>,
}

#[derive(Debug)]
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

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub current_track: CurrentTrack,
    pub tracks: VecDeque<Track>,
    pub total_count: i64,
    pub rnd_pages: VecDeque<usize>,
}

impl State {
    pub fn new() -> State {
        State {
            player: PlaylistInfo {
                current_track: CurrentTrack {
                    duration: 0.0,
                    album_title: String::new(),
                    artist_name: String::new(),
                    band_id: 0,
                    track: String::new(),
                    genre_text: String::new(),
                    play_date: DateTime::default(),
                },
                tracks: VecDeque::new(),
                total_count: 0,
                rnd_pages: VecDeque::new(),
            },
            server: ServerInfo {
                select_url: VecDeque::new(),
                top_page: String::from("https://bandcamp.com/"),
            },
            phantom: PhantomData,
        }
    }
}
