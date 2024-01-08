use std::collections::VecDeque;
use std::future::Future;
use std::io;

use anyhow::{Error, Result};
use async_trait::async_trait;
use bytes::BytesMut;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::{cursor, execute};
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use inquire::{InquireError, Select};
use inquire::ui::{Attributes, Color, RenderConfig, Styled, StyleSheet};
use rand::seq::SliceRandom;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui::widgets::Borders;
use scraper::{Html, Selector};
use tui_textarea::TextArea;

use crate::libbc::player::PARK;
use crate::debug_println;
use crate::libbc::http_client::{Client};
use crate::libbc::shared_data::SharedState;
use crate::libbc::stream_adapter::StreamAdapter;
use crate::libbc::terminal;
use crate::libbc::terminal::quit;
use crate::models::bc_discover_index::{DiscoverIndexRequest, Element, PostData};
use crate::models::bc_discover_json::{DiscoverJsonRequest, Results};
use crate::models::bc_error::BcradioError;
use crate::models::shared_data_models::Track;

#[async_trait]
pub trait PlayList {
    fn ask(&self) -> Result<PostData>;
    fn store_results(&self, post_data: PostData) -> Result<()>;
    fn fill_playlist(&self) -> Result<()>;
    fn discover_index(&self) -> Result<DiscoverIndexRequest>;
    fn discover_json(&self, post_data: PostData) -> Result<Vec<Results>>;
    fn choice(&self, t: DiscoverIndexRequest) -> Result<PostData>;
    fn gen_track_list(&self, items: Vec<Results>) -> Result<VecDeque<Track>>;
    fn top_menu(&self) -> Result<()>;

}

#[async_trait]
impl PlayList for SharedState {
    fn ask(&self) -> Result<PostData> {
        let res = self.discover_index().unwrap();
        let post_data = self.choice(res);
        let post_data = match post_data {
            Ok(choice) => choice,
            Err(e) => return Err(e),
        };

        Ok(post_data)
    }

    fn store_results(&self, post_data: PostData) -> Result<()> {
        let res = self.discover_json(post_data).unwrap();
        self.append_tracklist(self.gen_track_list(res)?);
        Ok(())
    }

    fn fill_playlist(&self) -> Result<()> {
        let l = self.queue_length_from_truck_list();
        if l < 1 {
            debug_println!("debug: queue length: {}\r", l);
            match self.next_post().cursor{
                Some(_) => {
                    let post_data = self.next_post().clone();
                    let res = self.discover_json(post_data).unwrap();
                    self.append_tracklist(self.gen_track_list(res)?);
                },
                None => {
                    *PARK.lock().unwrap() = false;
                    terminal::clear_screen();
                    println!("playlist end. cancel to quit.\r");
                    match self.ask() {
                        Ok(post_data)
                            => self.store_results(post_data).unwrap(),
                        Err(e)
                            => quit(e),
                    }
                    *PARK.lock().unwrap() = true;
                }
            }
        }
        Ok(())
    }

    fn discover_index(&self) -> Result<DiscoverIndexRequest>{
        let url = String::from("https://bandcamp.com/discover/deep-house");

        let client: Client = Default::default();
        let buf = client
            .get_curl_request(url)
            .unwrap()
            .to_vec()
            .unwrap();

        let slice = String::from_utf8(buf).unwrap();
        let doc = Html::parse_document(&slice);

        let c = doc
            .select(&Selector::parse("div[id='DiscoverApp']").unwrap())
            .next()
            .unwrap()
            .value()
            .attr("data-blob")
            .unwrap();
        let json: Result<DiscoverIndexRequest, serde_json::Error> =
                serde_json::from_slice(&mut bytes_mut(c.as_bytes())?);

        match json {
            Ok(r) => Ok(r),
            Err(e) => Err(Error::from(e)),
        }
    }

    fn discover_json(&self, post_data: PostData) -> Result<Vec<Results>>{
        let url = String::from("https://bandcamp.com/api/discover/1/discover_web");

        let client: Client = Default::default();
        let a = client.post_curl_request(url, post_data.clone()).unwrap().to_vec().unwrap();
        let aaa = String::from_utf8(a.clone())?;

        let json: Result<DiscoverJsonRequest, serde_json::Error> =
                serde_json::from_slice(&mut bytes_mut(a.as_slice())?);
        let cursor = json.as_ref().unwrap().cursor.clone();
        self.set_next_postdata(PostData {
            cursor,
            ..post_data
        });
        Ok(json?.results)
    }

    fn choice(&self, t: DiscoverIndexRequest) -> Result<PostData> {
        inquire::set_global_render_config(render_config());

        let g = t.app_data.initial_state.genres;
        self.save_genres(g.clone());
        let t = t.app_data.initial_state.subgenres;

        loop {
            let genre_ans = Select::new("genre?",
                    g.iter().map(|x| &x.label).collect()).prompt();
            match genre_ans {
                Ok(choice) => choice,
                Err(e) => match e {
                    InquireError::OperationCanceled
                        => return Err(Error::from(BcradioError::Cancel)),
                    InquireError::OperationInterrupted
                        => return Err(Error::from(BcradioError::OperationInterrupted)),
                    other_error
                        => panic!("inquire error: {:?}", other_error),
                }
            };

            let genres = g
                .iter()
                .find(|&x| &&x.label == genre_ans.as_ref().unwrap());
            let subgenres = t
                .iter()
                .filter(|&x| x.parent_id.unwrap() == genres.unwrap().id)
                .cloned()
                .collect::<Vec<Element>>();

            return if subgenres.len() == 0 {
                Ok(PostData {
                    tag_norm_names: vec![
                        genres.unwrap().slug.to_string()
                    ],
                    ..Default::default()
                })
            } else {
                let subgenre_ans = Select::new("sub genre?",
                        subgenres.iter().map(|x| &x.label).collect()).prompt();

                match subgenre_ans {
                    Ok(choice) => choice,
                    Err(e) => match e {
                        InquireError::OperationCanceled
                            => continue,
                        InquireError::OperationInterrupted
                            => return Err(Error::from(BcradioError::OperationInterrupted)),
                        other_error
                            => panic!("inquire error: {:?}", other_error),
                    }
                };

                let subgenre_value = subgenres
                    .iter()
                    .find(|&x| &&x.label == subgenre_ans.as_ref().unwrap());
                if subgenre_value.unwrap().slug.starts_with("all-") {
                    return Ok(PostData {
                        tag_norm_names: vec![
                            genres.unwrap().slug.to_string()
                        ],
                        ..Default::default()
                    })
                }
                Ok(PostData {
                    tag_norm_names: vec![
                        genres.unwrap().slug.to_string(),
                        subgenre_value.unwrap().slug.to_string(),
                    ],
                    ..Default::default()
                })
            }
        }
    }

    fn gen_track_list(&self, items: Vec<Results>) -> Result<VecDeque<Track>> {
        let mut track_list = VecDeque::new();
        for i in items.iter() {
            track_list.append(&mut VecDeque::from([Track {
                album_title: i.title.to_owned(),
                artist_name: i.featured_track.band_name.to_owned(),
                art_id: Option::from(i.item_image_id),
                band_id: i.band_id,
                url: i.featured_track.stream_url.to_owned(),
                duration: i.item_duration,
                track: i.featured_track.title.to_owned(),
                buffer: vec![],
                results: Option::from(i.to_owned()),
            }]));
        }
        Ok(track_list)
    }

    fn top_menu(&self) -> Result<()> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        enable_raw_mode()?;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        let mut textarea = TextArea::default();
        textarea.set_block(
            ratatui::widgets::block::Block::default()
                .borders(Borders::NONE)
                .title("menu")
        );
        execute!(
            io::stdout(),
            cursor::MoveTo(0, 0))?;

        match self.ask() {
            Ok(post_data) => {
                self.clear_all_tracklist();
                self.store_results(post_data).unwrap();
            },
            Err(e) => {
                match e.downcast_ref().unwrap() {
                    BcradioError::InvalidUrl => {},
                    BcradioError::OperationInterrupted
                        => {
                            execute!(
                                term.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture)?;
                            term.show_cursor()?;
                            quit(Error::from(e))
                    },
                    _ => {}
                }
            },
        }

        execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture)?;
        term.show_cursor()?;
        Ok(())
    }
}

fn render_config() -> RenderConfig {
    RenderConfig {
        help_message: StyleSheet::new() // help message
            .with_fg(Color::rgb(150, 150, 140)),
        prompt_prefix: Styled::new("?") // question prompt
            .with_fg(Color::rgb(150, 150, 140)),
        highlighted_option_prefix: Styled::new(">") // cursor
            .with_fg(Color::rgb(150, 250, 40)),
        selected_option: Some(StyleSheet::new() // focus
            .with_fg(Color::rgb(250, 180, 40))),
        answer: StyleSheet::new()
            .with_attr(Attributes::ITALIC)
            .with_attr(Attributes::BOLD)
            .with_fg(Color::rgb(220, 220, 240)),
        ..Default::default()
    }
}

pub fn bytes_mut(a: &[u8]) -> Result<BytesMut> {
    let mut b = BytesMut::new();
    b.extend_from_slice(a);
    Ok(b)
}

#[cfg(test)]
mod tests {
    use scraper::{Html, Selector};
    use bcradio::libbc::http_client::Client;
    use crate::models::bc_discover_json::DiscoverJsonRequest;
    #[test]
    fn test_json_parse() {
        let burl = String::from("http://localhost:8080/deep-house");

        let client: Client = Default::default();
        let buf = client.get_curl_request(burl).unwrap().to_vec().unwrap();
        let slice = String::from_utf8(buf).unwrap();
        let doc = Html::parse_document(&slice);

        let c = doc
            .select(&Selector::parse("div[id='DiscoverApp']").unwrap())
            .next()
            .unwrap()
            .value()
            .attr("data-blob")
            .unwrap();
        let json: Result<DiscoverJsonRequest, simd_json::Error>  = simd_json::from_reader(c.as_bytes());
        println!("test");
    }
}