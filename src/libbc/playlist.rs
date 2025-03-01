use std::sync::LazyLock;
use std::collections::VecDeque;
use std::io;

use anyhow::{Error, Result};
use bytes::BytesMut;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use futures::executor::block_on;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
use inquire::{InquireError, Select};
use itertools::Itertools;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Borders;
use ratatui::Terminal;
use regex::Regex;
use scraper::Html;
use tui_textarea::TextArea;

use crate::libbc::http_client::{get_blocking_request, post_request};
use crate::libbc::player::{park_lock, park_unlock};
use crate::libbc::search::parse_doc;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal;
use crate::libbc::terminal::quit;
use crate::models::bc_discover_index::{DiscoverIndexRequest, Element, PostData};
use crate::models::bc_discover_json::{DiscoverJsonRequest, Results};
use crate::models::bc_discover_tags::{DiscoverTagsJson, Struct, TagsPostData};
use crate::models::bc_error::BcradioError;
use crate::models::shared_data_models::{ResultsJson, Track};
use crate::{ceil, format_duration, lazy_regex};
use crate::libbc::progress_bar::destroy;

pub trait PlayList {
    fn ask(&self) -> Result<PostData>;
    fn silent(&self, genre: Option<String>, sub_genre: Option<String>) -> Result<PostData>;
    async fn store_results(&self, post_data: &PostData);
    async fn fill_playlist(&self) -> Result<()>;
    fn discover_index(&self, url: &str) -> Result<DiscoverIndexRequest>;
    async fn discover_json(&self, post_data: &PostData) -> Result<Vec<Results>>;
    async fn discover_tags_json(&self, post_data: &TagsPostData) -> Result<Vec<Element>>;
    fn choice(&self) -> Result<PostData>;
    fn gen_track_list(&self, items: &[Results]) -> Result<VecDeque<Track>>;
    fn top_menu(&self) -> Result<()>;
}

impl PlayList for SharedState {
    fn ask(&self) -> Result<PostData> {
        park_lock();
        let post_data = self.choice()?;
        park_unlock();
        Ok(post_data)
    }

    fn silent(&self, genre: Option<String>, sub_genre: Option<String>) -> Result<PostData> {
        let v = [genre, sub_genre]
            .into_iter()
            .filter(|i| i.is_some())
            .map(|x| slug(&x.unwrap()))
            .filter(|i| !i.is_empty())
            .collect::<Vec<_>>();

        let post_data = PostData {
            tag_norm_names: v,
            ..Default::default()
        };

        Ok(post_data)
    }

    async fn store_results(&self, post_data: &PostData) {
        let res = self.discover_json(post_data).await.unwrap();
        let aa = self.gen_track_list(&res).unwrap();
        self.append_tracklist(aa);
    }

    async fn fill_playlist(&self) -> Result<()> {
        let l = self.queue_length_from_truck_list();
        if l < 2 {
            match self.next_post().cursor {
                Some(_) => {
                    let post_data = &self.next_post();
                    let res = self.discover_json(post_data).await?;
                    self.append_tracklist(self.gen_track_list(&res)?);
                }
                None => {
                    destroy();
                    terminal::clear_screen();
                    println!("playlist is empty.\r");

                    match self.ask() {
                        Ok(post_data) => self.store_results(&post_data).await,
                        _ => quit(Error::from(BcradioError::Quit)),
                    }
                }
            }
        }
        Ok(())
    }

    fn discover_index(&self, url: &str) -> Result<DiscoverIndexRequest> {
        let buf = get_blocking_request(url)?;

        let slice = String::from_utf8(buf)?;
        let doc = Html::parse_document(&slice);

        let c = parse_doc(doc, "div[id='DiscoverApp']", "data-blob")?;

        let json: Result<DiscoverIndexRequest, serde_json::Error> =
            serde_json::from_slice(&bytes_mut(c.as_bytes())?);

        match json {
            Ok(r) => Ok(r),
            Err(e) => Err(Error::from(e)),
        }
    }

    async fn discover_json(&self, post_data: &PostData) -> Result<Vec<Results>> {
        let url = "https://bandcamp.com/api/discover/1/discover_web";
        let a = post_request(url, post_data).await;

        let json: DiscoverJsonRequest =
            serde_json::from_slice(&bytes_mut(a?.as_slice())?)?;

        let aa = json.results;
        self.set_next_postdata(&PostData {
            cursor: json.cursor.clone(),
            ..post_data.clone()
        });
        Ok(aa)
    }

    async fn discover_tags_json(&self, post_data: &TagsPostData) -> Result<Vec<Element>> {
        let url = "https://bandcamp.com/api/tag_search/2/related_tags";

        let a = post_request(url, post_data).await;

        let json: Result<DiscoverTagsJson, serde_json::Error> =
            serde_json::from_slice(&bytes_mut(a?.as_slice())?);
        let s = match json
            .unwrap_or_else(|_| DiscoverTagsJson::default())
            .single_results
            .first()
        {
            None => Struct::default(),
            Some(a) => a.clone(),
        };

        Ok(s.to_owned().related_tags
            .iter()
            .map(|x| Element {
                id: x.id,
                label: x.clone().name,
                slug: x.clone().norm_name,
                selected: None,
                parent_slug: None,
            })
            .collect::<Vec<Element>>())
    }

    fn choice(&self) -> Result<PostData> {
        park_lock();
        inquire::set_global_render_config(render_config());
        let url = "https://bandcamp.com/discover/";

        loop {
            let r = self.discover_index(url);
            let (g, t) = match r {
                Ok(mut t) => {
                    self.set_subgenre("");
                    let mut g = vec![Element {
                        label: "all genres".to_string(),
                        ..Default::default()
                    }];
                    g.append(&mut t.app_data.initial_state.genres);
                    let t = t.app_data.initial_state.subgenres;
                    self.save_genres(g.clone(), t.clone());

                    (g, t)
                }
                Err(_) => self.get_genres(),
            };

            let _genre_ans = Select::new("genre?", g.iter().map(|x| x.label.clone()).collect())
                .with_raw_return(true)
                .prompt();

            let genre_ans = match _genre_ans {
                Ok(ref choice) => {
                    self.set_genre(choice);
                    choice
                },
                Err(e) => match e {
                    InquireError::OperationCanceled => {
                        return Err(Error::from(BcradioError::Cancel))
                    }
                    InquireError::OperationInterrupted => {
                        return Err(Error::from(BcradioError::OperationInterrupted))
                    }
                    other_error => panic!("inquire error: {:?}", other_error),
                },
            };

            let element = pick_element(&g, genre_ans);
            match element {
                Some(ref genre) => {
                    if genre.label.starts_with("all genres") {
                        return Ok(PostData {
                            tag_norm_names: Vec::new(),
                            ..Default::default()
                        });
                    }
                }
                None => {
                    // tag request
                    let parent_labels = genre_list(&t, &g, genre_ans);

                    return match parent_labels.len() {
                        0 => {
                            let mut subgenres = Vec::<Element>::new();
                            let mut a = block_on(self.discover_tags_json(&TagsPostData {
                                tag_names: vec![slug(genre_ans)],
                                ..Default::default()
                            }))?;
                            if !a.is_empty() {
                                subgenres = vec![Element {
                                    label: format!("all \"{}\"", genre_ans),
                                    ..Default::default()
                                }];
                                subgenres.append(&mut a);
                            }

                            let mut tags = vec![slug(genre_ans)];
                            match Select::new(
                                "sub genre?",
                                subgenres.iter().map(|x| x.label.clone()).collect(),
                            )
                            .with_raw_return(false)
                            .prompt()
                            {
                                Ok(ref choice) => {
                                    self.set_subgenre("");
                                    if !choice.starts_with("all") {
                                        tags.append(&mut vec![slug(choice)]);
                                        self.set_subgenre(choice);
                                    }
                                }
                                Err(e) => match e {
                                    InquireError::OperationInterrupted => {
                                        return Err(Error::from(BcradioError::OperationInterrupted))
                                    }
                                    _ => continue,
                                },
                            };

                            Ok(PostData {
                                tag_norm_names: tags,
                                ..Default::default()
                            })
                        }
                        1 => {
                            // subgenre found, redirect
                            self.set_genre(&parent_labels[0]);
                            self.set_subgenre(genre_ans);
                            Ok(PostData {
                                tag_norm_names: vec![
                                    slug(&parent_labels[0]),
                                    slug(genre_ans),
                                ],
                                ..Default::default()
                            })
                        }
                        2.. => {
                            let ans = Select::new("which genre?", parent_labels)
                                .with_raw_return(false)
                                .prompt();

                            let ans = match ans {
                                Ok(ref choice) => choice,
                                Err(e) => match e {
                                    InquireError::OperationCanceled => {
                                        return Err(Error::from(BcradioError::Cancel))
                                    }
                                    InquireError::OperationInterrupted => {
                                        return Err(Error::from(BcradioError::OperationInterrupted))
                                    }
                                    _ => continue,
                                },
                            };
                            self.set_genre(ans);
                            self.set_subgenre(genre_ans);
                            Ok(PostData {
                                tag_norm_names: vec![slug(ans), slug(genre_ans)],
                                ..Default::default()
                            })
                        }
                    };
                }
            };

            let mut a = t
                .iter()
                .filter(|&x| x.to_owned().parent_slug.unwrap() == element.clone().unwrap().slug)
                .cloned()
                .collect::<Vec<Element>>();

            return if a.is_empty() {
                // audiobooks, podcasts..
                self.set_subgenre("");

                Ok(PostData {
                    tag_norm_names: vec![element.unwrap().slug.to_string()],
                    ..Default::default()
                })
            } else {
                let mut _subg = Vec::<Element>::new();
                _subg = vec![Element {
                    label: format!("all \"{}\"", genre_ans),
                    ..Default::default()
                }];

                _subg.append(&mut a);
                let mut tags = vec![slug(genre_ans)];
                match Select::new(
                    "sub genre?",
                    _subg.iter().map(|x| x.label.clone()).collect(),
                )
                .with_raw_return(false)
                .prompt()
                {
                    Ok(ref choice) => {
                        if !choice.starts_with("all") {
                            tags.append(&mut vec![slug(choice)]);
                            self.set_subgenre(choice);
                        }
                    }
                    Err(e) => match e {
                        InquireError::OperationInterrupted => {
                            return Err(Error::from(BcradioError::OperationInterrupted))
                        }
                        _ => continue,
                    },
                };

                Ok(PostData {
                    tag_norm_names: tags,
                    ..Default::default()
                })
            };
        }
    }

    fn gen_track_list(&self, items: &[Results]) -> Result<VecDeque<Track>> {
        let mut track_list = VecDeque::new();
        for i in items.iter() {
            track_list.append(&mut VecDeque::from([Track {
                album_title: i.title.to_owned(),
                artist_name: i.featured_track.band_name.to_owned(),
                art_id: i.item_image_id,
                band_id: i.band_id,
                url: i.featured_track.stream_url.to_owned(),
                duration: i.item_duration.unwrap_or_default(),
                track: i.featured_track.title.to_owned(),
                buffer: vec![],
                results: ResultsJson::Select(Box::new(i.clone())),
                genre: Some(self.get_genre().to_owned()),
                subgenre: Some(self.get_subgenre().to_owned()),
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
            EnableMouseCapture,
            cursor::MoveTo(0, 0)
        )?;

        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        let mut textarea = TextArea::default();
        textarea.set_block(
            ratatui::widgets::block::Block::default()
                .borders(Borders::NONE)
                .title("menu"),
        );

        match self.ask() {
            Ok(post_data) => {
                self.clear_all_tracklist();
                block_on(self.store_results(&post_data));
            }
            Err(e) => match e.downcast_ref().unwrap() {
                BcradioError::InvalidUrl => {}
                BcradioError::OperationInterrupted => {
                    execute!(
                        term.backend_mut(),
                        LeaveAlternateScreen,
                        DisableMouseCapture
                    )?;
                    term.show_cursor()?;
                    quit(e)
                }
                _ => {}
            },
        }

        execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        term.show_cursor()?;
        Ok(())
    }
}

fn genre_list(t: &[Element], g: &[Element], tag: &str) -> Vec<String> {
    let tt = slug(tag);
    t.iter()
        .filter(|&x| x.slug == tt)
        .cloned()
        .collect::<Vec<Element>>()
        .iter()
        .map(|x| {
            g.iter()
                .filter(|&b| b.slug == x.to_owned().parent_slug.unwrap())
                .cloned()
                .map(|x| x.label)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
}

fn render_config() -> RenderConfig<'static> {
    RenderConfig {
        help_message: StyleSheet::new() // help message
            .with_fg(Color::rgb(150, 150, 140)),
        prompt_prefix: Styled::new("?") // question prompt
            .with_fg(Color::rgb(150, 150, 140)),
        highlighted_option_prefix: Styled::new(">") // cursor
            .with_fg(Color::rgb(150, 250, 40)),
        selected_option: Some(
            StyleSheet::new() // focus
                .with_fg(Color::rgb(250, 180, 40)),
        ),
        answer: StyleSheet::new()
            .with_attr(Attributes::ITALIC)
            .with_attr(Attributes::BOLD)
            .with_fg(Color::rgb(220, 220, 240)),
        ..Default::default()
    }
}

fn pick_element(g: &[Element], key: &str) -> Option<Element> {
    match g.iter().find(|&x| x.label == key) {
        None => g.iter().find(|&x| x.slug == key).cloned(),
        Some(a) => Some(a.clone()),
    }
}

fn bytes_mut(a: &[u8]) -> Result<BytesMut> {
    let mut b = BytesMut::new();
    b.extend_from_slice(a);
    Ok(b)
}

lazy_regex!(
    RE1: r"[?#].*",
    RE2: r"[\[\]@!$'\(\)\*\+,:;=]",
    RE3: r"[/ _~&]",
    RE4: r"-+"
);
fn slug(s: &str) -> String {
    let b= &RE1.replace_all(s.trim(), "");
    let b= &RE2.replace_all(b, "");
    let b= &RE3.replace_all(b, "-");
    RE4.replace_all(b, "-").to_string()
}

/// show playlist
pub(crate) fn format(n: usize, x: &Track) -> String {
    let (title_width, title) = char_width(&x.track);
    let (artist_width, artist) = char_width(&x.artist_name);
    format!(
        "{:2} {:title_width$} {:>7} {:artist_width$} {}",
        n,
        title,
        format_duration!(ceil!(x.duration, 1.0) as u32),
        artist,
        x.album_title.clone()
    )
}

fn char_width(s: &str) -> (usize, String) {
    let max_length: i8 = 30;
    let mut n: i8 = 0;
    let mut m: i8 = 0;
    let mut v = Vec::new();
    for i in s.chars() {
        let a = combine_char_width(i);

        if n + a >= 29 {
            if n == 27 && a == 2 {
                v.append(&mut vec![" ".to_owned()]);
            }
            v.append(&mut vec!["..".into()]);
            break;
        } else {
            n += a;
            m += a - 1;
            v.append(&mut vec![i.into()]);
        }
    }

    ((max_length - m) as usize, v.iter().join(""))
}

fn combine_char_width(i: char) -> i8 {
    match i {
        '\u{0300}'..='\u{036F}' |
        '\u{1ab0}'..='\u{1aff}' |
        '\u{1dc0}'..='\u{1dff}' |
        '\u{20d0}'..='\u{20ff}' |
        '\u{2de0}'..='\u{2dff}' |
        '\u{3099}'..='\u{309a}' |
        '\u{303f}' |
        '\u{302a}'..='\u{302f}' |
        '\u{0e00}' | '\u{0e31}' |
        '\u{0e34}'..='\u{0e3a}' | // thainese
        '\u{0e47}'..='\u{0e4e}' | // thainese
        '\u{fe20}'..='\u{fe2f}' |
        '\u{feff}' => 0,
        '\u{09dc}'..='\u{09dd}' |
        '\u{09df}' |
        '\u{0958}'..='\u{095f}' |
        '\u{1100}'..='\u{115f}' |
        '\u{2329}'..='\u{232a}' |
        '\u{2adc}' |
        '\u{2e80}'..='\u{a4cf}' |
        '\u{ac00}'..='\u{d7a3}' |
        '\u{0e5b}' | '\u{0edc}' | '\u{0edd}' | // thainese
        '\u{f900}'..='\u{fa6b}' |
        '\u{fa6d}'..='\u{face}' |
        '\u{fad2}'..='\u{fad4}' |
        '\u{fad8}'..='\u{faff}' |
        '\u{fb1d}' |
        '\u{fb1f}' |
        '\u{fb2a}'..='\u{fb2b}' |
        '\u{fb2e}'..='\u{fb36}' |
        '\u{fb38}'..='\u{fb3c}' |
        '\u{fb3e}' |
        '\u{fb40}'..='\u{fb41}' |
        '\u{fb43}'..='\u{fb44}' |
        '\u{fb46}'..='\u{fb4e}' |
        '\u{fe10}'..='\u{fe19}' |
        '\u{fe30}'..='\u{fe6f}' |
        '\u{ff00}'..='\u{ff60}' |
        '\u{ffe0}'..='\u{ffe6}' |
        '\u{10000}'..='\u{fffff}' => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use crate::libbc::args::init_args;
    use crate::libbc::playlist::PlayList;
    use crate::libbc::shared_data::SharedState;
    use tokio::runtime::Runtime;
    pub(crate) fn runtime() -> &'static Runtime {
        static RUNTIME: once_cell::sync::OnceCell<Runtime> = once_cell::sync::OnceCell::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        })
    }
    #[test]
    fn test_url_escape() {
        let s = "all r&b/soul";
        let s = super::slug(s);
        assert_eq!(s, String::from("all-r-b-soul"));
    }

    #[test]
    fn test_menu() {
        runtime().block_on(async {
            init_args();
            let s = SharedState::default();
            let aa = s.choice().unwrap();
            println!("{:?}", aa);
        });
    }
}
