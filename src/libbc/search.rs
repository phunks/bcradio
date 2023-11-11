
use std::cmp;
use std::io;
use anyhow::Result;
use async_trait::async_trait;
use ratatui::widgets::Borders;
use reqwest::Response;
use scraper::{Html, Selector};
use serde_json::{from_slice, Value};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tui_textarea::{Input, Key, TextArea};
use crate::libbc::progress_bar::Progress;
use crate::libbc::stream_adapter::StreamAdapter;
use crate::models::search_models::{SearchJsonRequest, SearchJsonResponse, TrackInfo};
use crate::libbc::shared_data::{post_request, SharedState, Track};

#[async_trait]
pub trait Search {
    async fn html_to_json(res: Response) -> Result<Value>;
    async fn json_to_track(self, json: Value) -> Result<Vec<String>>;
    async fn json_to_track_with_band_id(self, json: Value) -> Result<Vec<String>>;
    async fn j2t(self, json: Value, fuzzy: bool) -> Result<Vec<String>>;
    async fn search(&self, search_type: &str, search_text: Option<String>) -> Result<()>;
    fn show_input_panel(&self) -> Result<String>;
}

#[async_trait]
impl Search for SharedState {
    async fn html_to_json(res: Response) -> Result<Value> {
        let html = &res.text().await?;
        let doc = Html::parse_document(html);

        let c = doc.select(&Selector::parse("script[data-tralbum]")
            .unwrap()).next().unwrap().value().attr("data-tralbum").unwrap();
        Ok(from_slice(c.as_ref())?)
    }

    async fn json_to_track(self, json: Value) -> Result<Vec<String>> {
        self.j2t(json, true).await
    }

    async fn json_to_track_with_band_id(self, json: Value) -> Result<Vec<String>> {
        self.j2t(json, false).await
    }

    async fn j2t(self, json: Value, fuzzy: bool) -> Result<Vec<String>> {
        let bid = self.get_current_track_info().band_id;
        let artist_name = &json["artist"].as_str().unwrap().to_string();
        let album_title = &json["current"]["title"].as_str().unwrap().to_string();
        let art_id = &json["current"]["art_id"].as_i64();
        let band_id = &json["current"]["band_id"].as_i64().unwrap();
        let mut v: Vec<String> = Vec::new();
        let t = &json["trackinfo"];

        if let Ok(track_info) = serde_json::from_str::<Vec<TrackInfo>>(&t.to_string()) {
            for i in track_info.iter() {
                let t = Track {
                    genre_text: String::new(),
                    album_title: album_title.to_string(),
                    artist_name: artist_name.to_string(),
                    art_id: *art_id,
                    band_id: *band_id,
                    url: i.file.mp3_128.clone().unwrap().to_string(),
                    duration: i.duration,
                    track: i.title.clone().unwrap().to_string(),
                    buffer: vec![],
                };
                let json = serde_json::to_string(&t).unwrap();
                if bid.eq(band_id) {
                    v.push(json);
                } else if fuzzy {
                    v.push(json);
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
            let url = String::from("https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic");
        #[cfg(debug)]
            let url = String::from("http://localhost:8080/api/bcsearch_public_api/1/autocomplete_elastic");
        let search_json_req = SearchJsonRequest {
            search_text: search_text.clone().unwrap(),
            search_filter: String::from(search_type),
            full_page: false,
            fan_id: None,
        };
        let response = post_request(url, search_json_req).await;
        let search_json_response = response?.json::<SearchJsonResponse>().await?;
        let mut v: Vec<String> = Vec::new();
        for search_item in search_json_response.auto.results {
            if let Some(url) = search_item.item_url_path.clone() {
                v.push(url);
            }
        }

        let url_list = v
            .iter()
            .map(|s| s.to_string())
            .collect();
        let content_type = "text/html";
        let client = self.gh_client(content_type)?;
        let r = match &*search_type {
            "t" => { // track search
                self.clone().bulk_url(client, url_list,
                  <SharedState as Search>::html_to_json,
                  <SharedState as Search>::json_to_track_with_band_id).await
            }
            "a" => { // album search
                self.clone().bulk_url(client, url_list,
                  <SharedState as Search>::html_to_json,
                  <SharedState as Search>::json_to_track).await
            }
            _ => Ok(Vec::new())
        };

        for i in r? {
            let a = serde_json::from_str::<Track>(&i)?;
            self.push_front_tracklist(a);
        }
        self.bar.disable_spinner();
        Ok(())
    }

    fn show_input_panel(&self) -> Result<String> {
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
                Input { key: Key::Enter, .. } => break,
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
        Ok(textarea.lines()[0].clone())
    }
}
