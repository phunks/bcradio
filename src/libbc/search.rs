use std::io;
use anyhow::{Error, Result};
use async_trait::async_trait;
use crossterm::*;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Borders;
use ratatui::Terminal;
use scraper::{Html, Selector};
use bytes::BytesMut;
use chrono::{NaiveDateTime, TimeZone};
use regex::Regex;
use simd_json::OwnedValue as Value;
use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};
use tui_textarea::{Input, Key, TextArea};

use crate::libbc::terminal::draw;
use crate::models::bc_error::BcradioError;
use crate::libbc::progress_bar::Progress;
use crate::libbc::shared_data::SharedState;
use crate::libbc::stream_adapter::StreamAdapter;
use crate::libbc::http_client::post_request;
use crate::models::shared_data_models::{ResultsJson, Track};
use crate::models::search_models::{Current, ItemPage, SearchJsonRequest, SearchJsonResponse, TrackInfo};

#[async_trait]
pub trait Search {
    async fn html_to_json(res: Vec<u8>) -> Result<Value>;
    async fn json_to_track(self, json: Value) -> Result<Vec<Track>>;
    async fn json_to_track_with_band_id(self, json: Value) -> Result<Vec<Track>>;
    async fn j2t(self, json: Value, fuzzy: bool) -> Result<Vec<Track>>;
    async fn search(&self, search_text: Option<String>) -> Result<()>;
    fn show_input_panel(&self) -> Result<Option<String>>;
}

#[async_trait]
impl Search for SharedState {
    async fn html_to_json(res: Vec<u8>) -> Result<Value> {
        let html = String::from_utf8(res)?;
        let doc = Html::parse_document(&html);

        let c = parse_doc(doc.clone(),
                          "script[data-tralbum]",
                          "data-tralbum")?;

        let mut b = BytesMut::new();
        b.extend_from_slice(c.as_ref());
        Ok(simd_json::from_slice(&mut b)?)
    }

    async fn json_to_track(self, json: Value) -> Result<Vec<Track>> {
        self.j2t(json, true).await
    }

    async fn json_to_track_with_band_id(self, json: Value) -> Result<Vec<Track>> {
        self.j2t(json, false).await
    }

    async fn j2t(self, json: Value, fuzzy: bool) -> Result<Vec<Track>> {
        let bid = self.get_current_track_info().band_id;

        let item_url = json["url"].to_string();
        let base_item_url = base_url(item_url.clone());
        let item_path = match json.get("album_url") {
            Some(a) => {
                if !a.to_string().is_empty() && !base_item_url.is_empty() {
                    format!("{}{}", base_item_url, a)
                } else {
                    String::from("")
                }
            },
            None => String::from(""),
        };

        let tracks: ItemPage = ItemPage {
            current: Current {
                title: json["current"]["title"].to_string(),
                art_id: json["art_id"].as_i64(),
                band_id: json["current"]["band_id"].as_i64().unwrap(),
                release_date: json["current"]["publish_date"].to_string(),
            },
            artist: json["artist"].to_string(),
            trackinfo: simd_json::serde::from_refowned_value::<Vec<TrackInfo>>(&json["trackinfo"]).unwrap(),
            album_url: Option::from(item_path),
            item_url: Option::from(item_url),
        };

        let mut v: Vec<Track> = Vec::new();

        for i in tracks.trackinfo.iter() {
            let t = Track {
                album_title: tracks.current.title.to_owned(),
                artist_name: tracks.artist.to_owned(),
                art_id: tracks.current.art_id,
                band_id: tracks.current.band_id,
                url: i.file.mp3_128.to_owned().unwrap(),
                duration: i.duration,
                track: i.title.to_owned().unwrap(),
                buffer: vec![],
                results: ResultsJson::Search(Box::new(tracks.clone())),
                genre: None,
                subgenre: None,
            };
            #[allow(clippy::if_same_then_else)]
            if bid.eq(&tracks.current.band_id) {
                v.push(t);
            } else if fuzzy {
                v.push(t);
            }
        }

        Ok(v)
    }

    async fn search(&self, mut search_text: Option<String>) -> Result<()> {
        if search_text.is_none() {
            search_text = Option::from(self.get_current_track_info().artist_name);
        }

        let url =
            String::from("https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic");
        let search_json_req = SearchJsonRequest {
            search_text: search_text.to_owned().unwrap(),
            search_filter: String::from("t"),
            full_page: false,
            fan_id: None,
        };
        let response = post_request(url, search_json_req).await;
        let search_json_response = response?.json::<SearchJsonResponse>().await?;
        let mut v: Vec<String> = Vec::new();
        for search_item in search_json_response.auto.results {
            if let Some(url) = search_item.item_url_path.to_owned() {
                v.push(url);
            }
        }

        let url_list = v.iter().map(|s| s.to_string()).collect();
        self.bar.enable_spinner();
        let r = self.to_owned()
            .bulk_url(
                url_list,
                <SharedState as Search>::html_to_json,
                <SharedState as Search>::json_to_track,
            )
            .await.unwrap();

        self.bar.disable_spinner();

        for i in r { self.push_front_tracklist(i) }
        Ok(())
    }

    fn show_input_panel(&self) -> Result<Option<String>> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        let mut textarea = TextArea::default();

        textarea.set_block(
            ratatui::widgets::block::Block::default()
                .borders(Borders::NONE)
                .title("? free word search")
        );

        loop {
            draw(&mut term, textarea.clone())?;
            match crossterm::event::read()?.into() {
                Input {
                    key: Key::Enter,
                    ..
                } => break,
                Input {
                    key: Key::Esc,
                    ..
                } => break,
                input => {
                    textarea.input(input);
                }
            }
        }

        execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture)?;

        term.show_cursor()?;
        Ok(if !textarea.lines()[0].is_empty() {
            Some(textarea.lines()[0].to_owned())
        } else {
            None
        })
    }
}

pub fn parse_doc(doc: Html, parse: &str, attribute: &str) -> Result<String> {
    return match doc
        .select(&Selector::parse(parse).unwrap())
        .next() {
        None => Err(Error::from(BcradioError::PhaseError)),
        Some(a) => {
            Ok(a.value().attr(attribute).unwrap().to_string())
        },
    };
}

fn base_url(item_url: String) -> String {
    let re = Regex::new("(https?://.*?)/.*").unwrap();
    re.replace(item_url.as_str(), "$1").to_string()
}

/// https://rosettacode.org/wiki/Date_manipulation#Rust
/// Chrono allows parsing time zone abbreviations like "EST", but
/// their meaning is ignored due to a lack of standardization.
///
/// This solution compromises by augmenting the parsed datetime
/// with the timezone using the IANA abbreviation.
#[allow(dead_code)]
fn format_date(date: String) -> String {
    // ex: 16 Jan 2024 15:03:36 GMT
    let ndt = NaiveDateTime::parse_from_str(&date, "%d %b %Y %H:%M:%S %Z").unwrap();
    let dt = chrono_tz::GMT.from_local_datetime(&ndt).unwrap();
    dt.format("%Y-%m-%d %H:%M:%S %Z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz;
    #[test]
    fn test_base_url() {
        let url = String::from("https://bandcamp.com/download?id=1234567890&ts=1234567890&sig=1234567890");
        assert_eq!(base_url(url), "https://bandcamp.com");
        let url = String::from("");
        assert_eq!(base_url(url), "");
    }
    #[test]
    fn test_format_date() {
        let date = String::from("16 Jan 2023 15:03:36 GMT");
        assert_eq!(format_date(date), "2023-01-16 15:03:36 GMT");
    }
}