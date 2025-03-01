use std::sync::LazyLock;
use anyhow::{Error, Result};
use async_trait::async_trait;
use chrono::{NaiveDateTime, TimeZone};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::*;
use inquire::MultiSelect;
use itertools::Itertools;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Borders;
use ratatui::Terminal;
use regex::Regex;
use scraper::{Html, Selector};
use std::io;
use log::info;
use tui_textarea::{Input, Key, TextArea};
use crate::lazy_regex;
use crate::libbc::http_adapter::{html_to_track, http_adapter};
use crate::libbc::http_client::post_request;
use crate::libbc::player::{park_lock, park_unlock};
use crate::libbc::progress_bar::{disable_spinner, enable_spinner};
use crate::libbc::scorer::score_sort;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal::{clear_screen, draw};
use crate::models::bc_error::BcradioError;
use crate::models::search_models::{SearchJsonRequest, SearchJsonResponse};

#[async_trait]
pub trait Search {
    async fn search(&self, search_text: Option<String>) -> Result<()>;
    fn show_input_panel(&self) -> Result<Option<String>>;
}

#[async_trait]
impl Search for SharedState {
    async fn search(&self, mut search_text: Option<String>) -> Result<()> {
        if search_text.is_none() {
            search_text = Option::from(self.get_current_track_info().artist_name);
        }

        let url =
            "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";
        let search_json_req = SearchJsonRequest {
            search_text: search_text.to_owned().unwrap(),
            search_filter: String::from("t"),
            full_page: false,
            fan_id: None,
        };

        let val = post_request(url, &search_json_req).await?;

        let search_json_response =
            simd_json::from_slice::<SearchJsonResponse>(val.clone().as_mut_slice())?;
        let mut v: Vec<String> = Vec::new();
        for search_item in search_json_response.auto.results {
            if let Some(url) = search_item.item_url_path.to_owned() {
                v.push(url);
            }
        }

        let url_list = v.iter().map(|s| s.to_string()).take(10).collect();
        enable_spinner();

        use std::time::Instant; //debug
        let _start = Instant::now(); //debug

        let mut r = http_adapter(url_list, html_to_track).await?;
        info!("Debug http_adapter: {:?}\r", _start.elapsed()); //debug

        disable_spinner();

        let uniq = r.iter().unique_by(|p| &p.band_id).collect::<Vec<_>>();
        if uniq.len() > 1 {
            park_lock();

            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            enable_raw_mode()?;
            execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
            clear_screen();
            let backend = CrosstermBackend::new(stdout);
            let mut term = Terminal::new(backend)?;

            let choice = MultiSelect::new(
                "Multiple search results found with different id.",
                uniq.iter().map(|x| x.artist_name.clone()).collect(),
            )
            .prompt();
            match choice {
                Err(_) => r.clear(),
                Ok(choice) => {
                    let t = uniq
                        .into_iter()
                        .filter(|&x| choice.iter().contains(&x.artist_name))
                        .collect::<Vec<_>>();
                    r = r
                        .clone()
                        .into_iter()
                        .filter(|x| {
                            t.clone()
                                .into_iter()
                                .map(|y| y.band_id)
                                .contains(&x.band_id)
                        })
                        .collect::<Vec<_>>();
                }
            }

            execute!(
                term.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;

            term.show_cursor()?;
            disable_raw_mode()?;
            park_unlock()
        }

        let r = score_sort(r, search_text.unwrap().as_str());
        for i in r.into_iter().enumerate() {
            self.push_front_tracklist(i.1);
        }
        Ok(())
    }

    fn show_input_panel(&self) -> Result<Option<String>> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        let mut textarea = TextArea::default();

        textarea.set_block(
            ratatui::widgets::block::Block::default()
                .borders(Borders::NONE)
                .title("? free word search"),
        );

        loop {
            draw(&mut term, textarea.clone())?;
            match crossterm::event::read()?.into() {
                Input {
                    key: Key::Enter, ..
                } => break,
                Input { key: Key::Esc, .. } => {
                    execute!(term.backend_mut(), LeaveAlternateScreen)?;
                    term.show_cursor()?;
                    return Ok(None);
                }
                input => {
                    textarea.input(input);
                }
            }
        }

        execute!(term.backend_mut(), LeaveAlternateScreen)?;
        term.show_cursor()?;

        Ok(if !textarea.lines()[0].is_empty() {
            Some(textarea.lines()[0].to_owned())
        } else {
            None
        })
    }
}

pub fn parse_doc(doc: Html, parse: &str, attribute: &str) -> Result<String> {
    match doc.select(&Selector::parse(parse).unwrap()).next() {
        None => Err(Error::from(BcradioError::PhaseError)),
        Some(a) => Ok(a.value().attr(attribute).unwrap().to_string()),
    }
}

lazy_regex!(RE: r"(https?://.*?)/.*");
pub fn base_url(item_url: &str) -> String {
    RE.replace(item_url, "$1").to_string()
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
