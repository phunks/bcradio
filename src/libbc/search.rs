use crate::libbc::progress_bar::Progress;
use crate::libbc::shared_data::SharedState;
use crate::libbc::stream_adapter::StreamAdapter;
use crate::libbc::http_client::post_request;
use crate::models::shared_data_models::Track;
use crate::models::search_models::{SearchJsonRequest, SearchJsonResponse, TrackInfo};
use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::Borders;
use ratatui::Terminal;
use scraper::{Html, Selector};

use std::cmp;
use std::io;
use bytes::BytesMut;
use simd_json::OwnedValue as Value;
use simd_json::prelude::ValueAsScalar;
use tui_textarea::{Input, Key, TextArea};


#[async_trait]
pub trait Search {
    async fn html_to_json(res: Vec<u8>) -> Result<Value>;
    async fn json_to_track(self, json: Value) -> Result<Vec<Track>>;
    async fn json_to_track_with_band_id(self, json: Value) -> Result<Vec<Track>>;
    async fn j2t(self, json: Value, fuzzy: bool) -> Result<Vec<Track>>;
    async fn search(&self, search_type: &str, search_text: Option<String>) -> Result<()>;
    fn show_input_panel(&self) -> Result<Option<String>>;
}

#[async_trait]
impl Search for SharedState {
    async fn html_to_json(res: Vec<u8>) -> Result<Value> {
        let html = String::from_utf8(res)?;
        let doc = Html::parse_document(&html);

        let c = doc
            .select(&Selector::parse("script[data-tralbum]").unwrap())
            .next()
            .unwrap()
            .value()
            .attr("data-tralbum")
            .unwrap();

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
        let artist_name = &json["artist"].as_str().unwrap().to_string();
        let album_title = &json["current"]["title"].as_str().unwrap().to_string();
        let art_id = &json["current"]["art_id"].as_i64();
        let band_id = &json["current"]["band_id"].as_i64().unwrap();
        let mut v: Vec<Track> = Vec::new();
        let t = &json["trackinfo"];

        if let Ok(track_info) = simd_json::serde::from_refowned_value::<Vec<TrackInfo>>(t) {
            for i in track_info.iter() {
                let t = Track {
                    genre_text: String::new(),
                    album_title: album_title.to_owned(),
                    artist_name: artist_name.to_owned(),
                    art_id: *art_id,
                    band_id: *band_id,
                    url: i.file.mp3_128.to_owned().unwrap().to_owned(),
                    duration: i.duration,
                    track: i.title.to_owned().unwrap().to_owned(),
                    buffer: vec![],
                };
                #[allow(clippy::if_same_then_else)]
                if bid.eq(band_id) {
                    v.push(t);
                } else if fuzzy {
                    v.push(t);
                }
            }
        }
        Ok(v)
    }

    async fn search(&self, search_type: &str, mut search_text: Option<String>) -> Result<()> {
        self.bar.enable_spinner();
        if search_text.is_none() {
            search_text = Option::from(self.get_current_track_info().artist_name);
        }

        #[cfg(not(debug))]
        let url =
            String::from("https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic");
        #[cfg(debug)]
        let url =
            String::from("http://localhost:8080/api/bcsearch_public_api/1/autocomplete_elastic");
        let search_json_req = SearchJsonRequest {
            search_text: search_text.to_owned().unwrap(),
            search_filter: String::from(search_type),
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
        let r = match search_type {
            "t" => { // track search
                self.to_owned()
                    .bulk_url(
                        url_list,
                        <SharedState as Search>::html_to_json,
                        <SharedState as Search>::json_to_track_with_band_id,
                    )
                    .await
            }
            "a" => { // album search
                self.to_owned()
                    .bulk_url(
                        url_list,
                        <SharedState as Search>::html_to_json,
                        <SharedState as Search>::json_to_track,
                    )
                    .await
            }
            _ => Ok(Vec::new()),
        };

        for i in r? { self.push_front_tracklist(i) }
        self.bar.disable_spinner();
        Ok(())
    }

    fn show_input_panel(&self) -> Result<Option<String>> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        enable_raw_mode()?;
        crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        let mut textarea = TextArea::default();
        textarea.set_block(
            ratatui::widgets::block::Block::default()
                .borders(Borders::NONE)
                .title("? free word search")
        );

        loop {
            term.draw(|f| {
                const MIN_HEIGHT: usize = 1;
                let height = cmp::max(textarea.lines().len(), MIN_HEIGHT) as u16 + 1; // + 1 for title
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(height), Constraint::Min(0)].as_slice())
                    .split(f.size());
                f.render_widget(textarea.widget(), chunks[0]);
            })?;
            match crossterm::event::read()?.into() {
                Input {
                    key: Key::Enter, ..
                } => break,
                Input { key: Key::Esc, .. } => break,
                input => {
                    textarea.input(input);
                }
            }
        }

        crossterm::execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        disable_raw_mode()?;
        term.show_cursor()?;
        Ok(if !textarea.lines()[0].is_empty() {
            Some(textarea.lines()[0].to_owned())
        } else {
            None
        })
    }
}
