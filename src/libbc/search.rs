use std::collections::HashMap;
use std::io;
use anyhow::{Error, Result};
use async_trait::async_trait;
use chrono::{NaiveDateTime, TimeZone};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::*;
use curl::multi::Easy2Handle;
use inquire::MultiSelect;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Borders;
use ratatui::Terminal;
use scraper::{Html, Selector};
use itertools::Itertools;
use regex::Regex;
use tui_textarea::{Input, Key, TextArea};
use crate::debug_println;

use crate::libbc::http_adapter::{html_to_json, http_adapter, json_to_track};
use crate::libbc::http_client::{Client, Collector, decoder};
use crate::libbc::player::PARK;
use crate::libbc::progress_bar::Progress;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal::{clear_screen, draw};
use crate::models::bc_error::BcradioError;
use crate::models::search_models::{SearchJsonRequest, SearchJsonResponse};
use crate::models::shared_data_models::Track;

#[async_trait]
pub trait Search {
    fn search(&self, search_text: Option<String>) -> Result<()>;
    fn show_input_panel(&self) -> Result<Option<String>>;
}

#[async_trait]
impl Search for SharedState {
    fn search(&self, mut search_text: Option<String>) -> Result<()> {
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

        let client = Client::default();
        let val = client.post_curl_request(url, search_json_req)?.vec()?;

        let search_json_response =
            simd_json::from_slice::<SearchJsonResponse>(val.clone().as_mut_slice())?;
        let mut v: Vec<String> = Vec::new();
        for search_item in search_json_response.auto.results {
            if let Some(url) = search_item.item_url_path.to_owned() {
                v.push(url);
            }
        }

        let url_list = v.iter().map(|s| s.to_string()).take(10).collect();
        self.bar.enable_spinner();

        use std::time::Instant; //debug
        let _start = Instant::now(); //debug
        let mut r = http_adapter(url_list, collector, multi_output)?;
        debug_println!("Debug http_adapter: {:?}\r", _start.elapsed()); //debug

        self.bar.disable_spinner();

        let uniq = r.iter().unique_by(|p| &p.band_id).collect::<Vec<_>>();
        if uniq.len() > 1 {
            *PARK.lock().unwrap() = false;

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
                Err(_) => { r.clear() }
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
            *PARK.lock().unwrap() = true;
        }

        for i in r {
            self.push_front_tracklist(i)
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
                Input { key: Key::Enter, .. } => break,
                Input { key: Key::Esc, .. } => {
                    execute!(term.backend_mut(), LeaveAlternateScreen)?;
                    term.show_cursor()?;
                    return Ok(None)
                },
                input => { textarea.input(input); },
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
    return match doc.select(&Selector::parse(parse).unwrap()).next() {
        None => Err(Error::from(BcradioError::PhaseError)),
        Some(a) => Ok(a.value().attr(attribute).unwrap().to_string()),
    };
}

pub fn base_url(item_url: String) -> String {
    let re = Regex::new("(https?://.*?)/.*").unwrap();
    re.replace(item_url.as_str(), "$1").to_string()
}

fn collector(handle: &mut Easy2Handle<Collector<Track>>)
{
    let html = decoder(handle.get_ref().res.clone());
    if let Ok(t) = html_to_json(html) { handle.get_mut().dat = json_to_track(t).unwrap() }
}

fn multi_output(handles: HashMap<usize, Easy2Handle<Collector<Track>>>) -> Vec<Track> {
    let mut v = Vec::<Track>::new();
    handles.iter().for_each(|x|{
        #[allow(clippy::len_zero)]
        if x.1.get_ref().dat.len() > 0 {
            v.append(&mut x.1.get_ref().dat.clone())
        }
    });
    v
}

const MP3_SIGNATURE_1: [u8; 2] = [0xFF, 0xFB];
const MP3_SIGNATURE_2: [u8; 2] = [0xFF, 0xF3];
const MP3_SIGNATURE_3: [u8; 2] = [0xFF, 0xF2];
const MP3_SIGNATURE_4: [u8; 2] = [0xFF, 0xFA];
//0x49, 0x44, 0x43
const MP3_SIGNATURE_5: [u8; 2] = [0x49, 0x44];

pub fn is_mp3(v: Vec<u8>) -> bool {
    match v.get(0..2) {
        Some(mp3)
        => matches!(mp3.try_into().unwrap(),
                MP3_SIGNATURE_1
                | MP3_SIGNATURE_2
                | MP3_SIGNATURE_3
                | MP3_SIGNATURE_4
                | MP3_SIGNATURE_5 ),
        _ => false,
    }
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
        let url = String::from(
            "https://bandcamp.com/download?id=1234567890&ts=1234567890&sig=1234567890",
        );
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
