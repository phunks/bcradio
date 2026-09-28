use crate::lazy_regex;
use crate::libbc::http_adapter::{html_to_track, http_adapter};
use crate::libbc::http_client::post_request;
use crate::libbc::progress_bar::{disable_spinner, enable_spinner};
use crate::libbc::scorer::score_sort;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal::{clear_screen, draw, AlternateScreen};
use crate::models::bc_error::BcradioError;
use crate::models::search_models::{SearchItem, SearchJsonRequest, SearchJsonResponse};
use anyhow::{Context, Error, Result};
use chrono::{NaiveDateTime, TimeZone};
use inquire::MultiSelect;
use itertools::Itertools;
use log::info;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Borders;
use ratatui::Terminal;
use regex::Regex;
use scraper::{Html, Selector};
use std::io;
use std::sync::LazyLock;
use tui_textarea::{Input, Key, TextArea};

pub trait Search {
    async fn search(&self, search_text: Option<String>) -> Result<()>;
    fn show_input_panel(&self) -> Result<Option<String>>;
}

/// Reuse the free-word search editor for both Bandcamp search and AI prompts.
pub fn input_panel(title: &str) -> Result<Option<String>> {
    let _screen = AlternateScreen::enter(false)?;
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    let mut textarea = TextArea::default();

    textarea.set_block(
        ratatui::widgets::block::Block::default()
            .borders(Borders::NONE)
            .title(title),
    );

    loop {
        draw(&mut term, textarea.clone())?;
        match crossterm::event::read()?.into() {
            Input {
                key: Key::Enter, ..
            } => break,
            Input { key: Key::Esc, .. } => return Ok(None),
            input => {
                textarea.input(input);
            }
        }
    }

    let text = textarea.lines().join(" ");
    Ok((!text.trim().is_empty()).then_some(text))
}

/// Search Bandcamp for a term without changing the playback queue.
pub async fn find_tracks(term: &str) -> Result<Vec<crate::models::shared_data_models::Track>> {
    let request = SearchJsonRequest {
        search_text: term.to_owned(),
        search_filter: "t".into(),
        full_page: false,
        fan_id: None,
    };
    let val = post_request(
        "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic",
        &request,
    )
    .await
    .context("Bandcamp search request failed")?;
    let response: SearchJsonResponse =
        serde_json::from_slice(&val).context("invalid Bandcamp search response")?;
    let result_count = response.auto.results.len();
    let urls = ai_search_urls(&response.auto.results);
    tracing::debug!(
        term,
        result_count,
        url_count = urls.len(),
        "Bandcamp search results"
    );
    if result_count > 0 && urls.is_empty() {
        anyhow::bail!("Bandcamp search returned {result_count} results but no valid track URLs");
    }
    let tracks = http_adapter(urls, html_to_track)
        .await
        .context("failed to load Bandcamp search results")?;
    tracing::debug!(
        term,
        playable_count = tracks.len(),
        "Bandcamp playable tracks"
    );
    Ok(tracks)
}

// Fetch every result returned by the autocomplete API, not just its first five.
fn ai_search_urls(results: &[SearchItem]) -> Vec<String> {
    results
        .iter()
        .filter_map(search_item_url)
        .unique()
        .collect()
}

fn search_item_url(item: &SearchItem) -> Option<String> {
    let path = item.item_url_path.as_deref()?;
    let parsed = url::Url::parse(path).ok().or_else(|| {
        let root = url::Url::parse(item.item_url_root.as_deref()?).ok()?;
        root.join(path).ok()
    })?;
    (parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed
            .host_str()
            .is_some_and(|host| host == "bandcamp.com" || host.ends_with(".bandcamp.com")))
    .then(|| parsed.to_string())
}

#[cfg(test)]
mod ai_search_tests {
    use super::{ai_search_urls, search_item_url};
    use crate::models::search_models::SearchItem;

    fn item(root: &str, path: &str) -> SearchItem {
        serde_json::from_value(serde_json::json!({
            "type": "t", "id": 1, "name": "song", "band_id": 2,
            "item_url_root": root, "item_url_path": path
        }))
        .unwrap()
    }

    #[test]
    fn resolves_relative_bandcamp_search_results() {
        assert_eq!(
            search_item_url(&item("https://artist.bandcamp.com", "/track/song")),
            Some("https://artist.bandcamp.com/track/song".into())
        );
        assert_eq!(
            search_item_url(&item(
                "https://artist.bandcamp.com",
                "https://other.bandcamp.com/track/song"
            )),
            Some("https://other.bandcamp.com/track/song".into())
        );
        for path in [
            "https://bandcamp.com.evil.test/track/song",
            "http://artist.bandcamp.com/track/song",
            "https://user:pass@artist.bandcamp.com/track/song",
        ] {
            assert_eq!(
                search_item_url(&item("https://artist.bandcamp.com", path)),
                None
            );
        }
        assert_eq!(
            search_item_url(&item("https://evil.test", "/track/song")),
            None
        );
    }

    #[test]
    fn ai_search_includes_all_valid_urls_and_ignores_duplicates() {
        let mut results = (0..8)
            .map(|i| item("https://artist.bandcamp.com", &format!("/track/song-{i}")))
            .collect::<Vec<_>>();
        results.push(item("https://artist.bandcamp.com", "/track/song-0"));
        results.push(item("https://evil.test", "/track/not-bandcamp"));
        let urls = ai_search_urls(&results);
        assert_eq!(urls.len(), 8);
        assert_eq!(urls[7], "https://artist.bandcamp.com/track/song-7");
    }
}

impl Search for SharedState {
    async fn search(&self, mut search_text: Option<String>) -> Result<()> {
        if search_text.is_none() {
            search_text = Option::from(self.get_current_track_info().artist_name);
        }
        let search_text = search_text.unwrap_or_default();

        let url = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";
        let search_json_req = SearchJsonRequest {
            search_text: search_text.clone(),
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
            let _input = self.input_gate.pause();

            let _screen = AlternateScreen::enter(true)?;
            clear_screen();
            let stdout = io::stdout();
            let stdout = stdout.lock();
            let backend = CrosstermBackend::new(stdout);
            let term = Terminal::new(backend)?;

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

            drop(term);
            drop(_screen);
        }

        let r = score_sort(r, search_text.as_str());
        for i in r.into_iter().enumerate() {
            self.push_front_tracklist(i.1);
        }
        Ok(())
    }

    fn show_input_panel(&self) -> Result<Option<String>> {
        input_panel("? free word search")
    }
}

pub fn parse_doc(doc: Html, parse: &str, attribute: &str) -> Result<String> {
    let selector = Selector::parse(parse).map_err(|_| Error::from(BcradioError::PhaseError))?;
    match doc.select(&selector).next() {
        None => Err(Error::from(BcradioError::PhaseError)),
        Some(a) => a
            .value()
            .attr(attribute)
            .map(ToString::to_string)
            .ok_or_else(|| Error::from(BcradioError::PhaseError)),
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
