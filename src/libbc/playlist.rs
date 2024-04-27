use std::collections::VecDeque;
use std::io;

use anyhow::{Error, Result};
use bytes::BytesMut;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::{cursor, execute};
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures::executor::block_on;
use inquire::{InquireError, Select};
use inquire::ui::{Attributes, Color, RenderConfig, Styled, StyleSheet};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui::widgets::Borders;
use scraper::Html;
use tui_textarea::TextArea;
use regex::Regex;

use crate::libbc::player::PARK;
use crate::libbc::http_client::HttpClient;
use crate::libbc::progress_bar::Progress;
use crate::libbc::search::parse_doc;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal;
use crate::libbc::terminal::quit;
use crate::models::bc_discover_index::{DiscoverIndexRequest, Element, PostData};
use crate::models::bc_discover_json::{DiscoverJsonRequest, Results};
use crate::models::bc_discover_tags::{DiscoverTagsJson, Struct, TagsPostData};
use crate::models::bc_error::BcradioError;
use crate::models::shared_data_models::{ResultsJson, Track};

pub trait PlayList {
    fn ask(&self) -> Result<PostData>;
    async fn store_results(&self, post_data: PostData);
    async fn fill_playlist(&self) -> Result<()>;
    fn discover_index(&self, url: String) -> Result<DiscoverIndexRequest>;
    async fn discover_json(&self, post_data: PostData) -> Result<Vec<Results>>;
    async fn discover_tags_json(&self, post_data: TagsPostData) -> Result<Vec<Element>>;
    fn choice(&self) -> Result<PostData>;
    fn gen_track_list(&self, items: Vec<Results>) -> Result<VecDeque<Track>>;
    fn top_menu(&self) -> Result<()>;
}

impl PlayList for SharedState {
    fn ask(&self) -> Result<PostData> {
        *PARK.lock().unwrap() = false;
        let post_data = self.choice();
        let post_data = match post_data {
            Ok(choice) => choice,
            Err(e) => return Err(e),
        };

        *PARK.lock().unwrap() = true;
        Ok(post_data)
    }

    async fn store_results(&self, post_data: PostData) {
        let res = self.discover_json(post_data).await.unwrap();
        let aa = self.gen_track_list(res).unwrap();
        self.append_tracklist(aa);
    }

    async fn fill_playlist(&self) -> Result<()> {
        let l = self.queue_length_from_truck_list();
        if l < 1 {
            match self.next_post().cursor{
                Some(_) => {
                    let post_data = self.next_post().clone();
                    let res = self.discover_json(post_data).await?;
                    self.append_tracklist(self.gen_track_list(res)?);
                },
                None => {
                    self.bar.destroy();
                    terminal::clear_screen();
                    println!("playlist is empty.\r");

                    match self.ask() {
                        Ok(post_data)
                            => self.store_results(post_data).await,
                        _ => quit(Error::from(BcradioError::Quit)),
                    }
                }
            }
        }
        Ok(())
    }

    fn discover_index(&self, url: String) -> Result<DiscoverIndexRequest>{

        let client= HttpClient::default();
        let buf = client
            .get_blocking_request(url);

        let slice = String::from_utf8(buf?.res).unwrap();
        let doc = Html::parse_document(&slice);

        let c = parse_doc(doc,
                          "div[id='DiscoverApp']",
                          "data-blob")?;

        let json: Result<DiscoverIndexRequest, serde_json::Error> =
                serde_json::from_slice(&bytes_mut(c.as_bytes())?);

        match json {
            Ok(r) => Ok(r),
            Err(e) => Err(Error::from(e)),
        }
    }

    async fn discover_json(&self, post_data: PostData) -> Result<Vec<Results>>{
        let url = String::from("https://bandcamp.com/api/discover/1/discover_web");

        let client = HttpClient::default();
        let a = client.post_request(url, post_data.clone()).await;

        let json: Result<DiscoverJsonRequest, serde_json::Error> =
                serde_json::from_slice(&bytes_mut(a?.res.as_slice())?);
        let cursor = json.as_ref().unwrap().cursor.clone();
        self.set_next_postdata(PostData {
            cursor,
            ..post_data
        });
        Ok(json?.results)
    }

    async fn discover_tags_json(&self, post_data: TagsPostData) -> Result<Vec<Element>> {
        let url = String::from("https://bandcamp.com/api/tag_search/2/related_tags");

        let client = HttpClient::default();
        let a = client.post_request(url, post_data).await;

        let json: Result<DiscoverTagsJson, serde_json::Error> =
            serde_json::from_slice(&bytes_mut(a?.res.as_slice())?);
        let s = match json.unwrap_or_else(|_| DiscoverTagsJson::default()).single_results.first() {
            None => {Struct::default()}
            Some(a) => a.clone()
        };

        let bb = s.to_owned().related_tags;

        Ok(bb.iter().map(|x| Element {
            id: x.id,
            label: x.clone().name,
            slug: x.clone().norm_name,
            selected: None,
            parent_slug: None,
        }).collect::<Vec::<Element>>())
    }

    fn choice(&self) -> Result<PostData> {
        *PARK.lock().unwrap() = false;
        inquire::set_global_render_config(render_config());
        let url = String::from("https://bandcamp.com/discover/");
        let mut _genre_ans = Ok(String::new());

        loop {
            let r = self.discover_index(url.to_string());
            let (g, t) = match r {
                Ok(mut t) => {
                    let mut g = vec!(Element {
                        label: "all genres".to_string(),
                        ..Default::default()
                    });
                    g.append(&mut t.app_data.initial_state.genres);
                    let t = t.app_data.initial_state.subgenres;
                    self.save_genres(g.clone(), t.clone());

                    (g, t)
                },
                Err(_) => self.get_genres(),
            };

            _genre_ans = Select::new("genre?",
                                     g.iter().map(|x| x.label.clone()).collect())
                .with_raw_return(true)
                .prompt();

            match _genre_ans {
                Ok(ref choice) => self.set_genre(choice),
                Err(e) => match e {
                    InquireError::OperationCanceled
                        => return Err(Error::from(BcradioError::Cancel)),
                    InquireError::OperationInterrupted
                        => return Err(Error::from(BcradioError::OperationInterrupted)),
                    other_error
                        => panic!("inquire error: {:?}", other_error),
                }
            }

            let element = pick_element(g.clone(), _genre_ans.as_ref());
            match element {
                Some(ref genre) => {
                    if genre.label.starts_with("all genres") {
                        return Ok(PostData {
                            tag_norm_names: Vec::new(),
                            ..Default::default()
                        })
                    }
                },
                None => { // tag request
                    let parent_labels = genre_list(t, g, _genre_ans.as_ref().unwrap().clone());
                    let tags = _genre_ans.as_ref().unwrap().clone();

                    return match parent_labels.len() {
                        0 => {
                            let mut subgenres = Vec::<Element>::new();
                            let mut a = block_on(self.discover_tags_json(
                                TagsPostData {
                                    tag_names: vec![slug(tags.clone())],
                                    ..Default::default()
                                }))?;
                            if !a.is_empty() {
                                subgenres = vec!(Element {
                                    label: format!("all \"{}\"", _genre_ans.as_ref().unwrap()),
                                    ..Default::default()
                                });
                                subgenres.append(&mut a);
                            }

                            let mut tags = vec!(slug(_genre_ans.as_ref().unwrap().to_string()));
                            match Select::new("sub genre?",
                                        subgenres.iter().map(|x| x.label.clone()).collect())
                                .with_raw_return(false)
                                .prompt() {
                                Ok(ref choice) => {
                                    self.set_subgenre("");
                                    if !choice.starts_with("all") {
                                        tags.append(&mut vec![slug(choice.to_owned())]);
                                        self.set_subgenre(choice);
                                    }
                                },
                                Err(e) => match e {
                                    InquireError::OperationInterrupted
                                        => return Err(Error::from(BcradioError::OperationInterrupted)),
                                    _ => continue,
                                }
                            };

                            Ok(PostData {
                                tag_norm_names: tags,
                                ..Default::default()
                            })
                        },
                        1 => { // subgenre found, redirect
                            self.set_genre(&parent_labels[0]);
                            self.set_subgenre(&tags);
                            Ok(PostData {
                                tag_norm_names: vec![
                                    slug(parent_labels[0].clone()),
                                    slug(tags.to_string()),
                                ],
                                ..Default::default()
                            })
                        },
                        2.. => {
                            let ans = Select::new("which genre?",
                                    parent_labels)
                                .with_raw_return(false)
                                .prompt();

                            let ans = match ans {
                                Ok(ref choice) => choice,
                                Err(e) => match e {
                                    InquireError::OperationCanceled
                                    => return Err(Error::from(BcradioError::Cancel)),
                                    InquireError::OperationInterrupted
                                    => return Err(Error::from(BcradioError::OperationInterrupted)),
                                    _ => continue,
                                }
                            };
                            self.set_genre(ans);
                            self.set_subgenre(&tags);
                            Ok(PostData {
                                tag_norm_names: vec![
                                    slug(ans.to_string()),
                                    slug(tags.to_string()),
                                ],
                                ..Default::default()
                            })
                        },
                    }
                },
            };

            let mut a = t
                .iter()
                .filter(|&x| x.to_owned().parent_slug.unwrap() == element.clone().unwrap().slug)
                .cloned()
                .collect::<Vec<Element>>();

            return if a.is_empty() { // audiobooks, podcasts..
                self.set_subgenre("");

                Ok(PostData {
                    tag_norm_names: vec!(element.unwrap().slug.to_string()),
                    ..Default::default()
                })
            } else {
                let mut _subg = Vec::<Element>::new();
                _subg = vec!(Element {
                    label: format!("all \"{}\"", _genre_ans.as_ref().unwrap()),
                    ..Default::default()
                });

                _subg.append(&mut a);
                let mut tags = vec!(slug(_genre_ans.as_ref().unwrap().to_string()));
                match Select::new("sub genre?",
                                  _subg.iter().map(|x| x.label.clone()).collect())
                    .with_raw_return(false)
                    .prompt() {
                    Ok(ref choice) => {
                        if !choice.starts_with("all") {
                            tags.append(&mut vec![slug(choice.to_owned())]);
                            self.set_subgenre(choice);
                        }
                    },
                    Err(e) => match e {
                        InquireError::OperationInterrupted
                            => return Err(Error::from(BcradioError::OperationInterrupted)),
                        _ => continue,
                    }
                };

                Ok(PostData {
                    tag_norm_names: tags,
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
                art_id: i.item_image_id,
                band_id: i.band_id,
                url: i.featured_track.stream_url.to_owned(),
                duration: i.item_duration,
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
            cursor::MoveTo(0, 0))?;

        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;
        let mut textarea = TextArea::default();
        textarea.set_block(
            ratatui::widgets::block::Block::default()
                .borders(Borders::NONE)
                .title("menu")
        );

        match self.ask() {
            Ok(post_data) => {
                self.clear_all_tracklist();
                block_on(self.store_results(post_data));
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
                            quit(e)
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

fn genre_list(t: Vec<Element>, g: Vec<Element>, tag: String) -> Vec<String> {
    let tt = slug(tag);
    t.iter()
        .filter(|&x| x.slug == tt).cloned()
        .collect::<Vec<Element>>()
        .iter()
        .map(|x| g
            .iter()
            .filter(|&b| b.slug == x.to_owned().parent_slug.unwrap()).cloned()
            .map(|x| x.label)
            .collect::<String>()
        ).collect::<Vec<_>>()
}

fn render_config() -> RenderConfig<'static> {
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

fn pick_element(g: Vec<Element>, key: Result<&String, &InquireError>) -> Option<Element> {
    return match g.iter()
        .find(|&x| &x.label == key.unwrap()).cloned() {
        None => g.iter()
                .find(|&x| &x.slug == key.unwrap()).cloned(),
        Some(a) => Option::from(a)
    }
}

fn bytes_mut(a: &[u8]) -> Result<BytesMut> {
    let mut b = BytesMut::new();
    b.extend_from_slice(a);
    Ok(b)
}

fn slug(mut s: String) -> String {
    s = s.trim().parse().unwrap();
    [(r"[?#].*", ""),
        (r"[\[\]@!$'\(\)\*\+,:;=]", ""),
        (r"[/ _~&]", "-"),
        (r"-+", "-"),
    ].iter().for_each(|i| {
        let re = Regex::new(i.0).expect("Invalid regex");
        s = re.replace_all(&s.clone(), i.1).to_string();
    });
    s
}

#[cfg(test)]
mod tests {
    use scraper::Html;
    use tokio::runtime::Runtime;
    use crate::libbc::args::init_args;
    use crate::libbc::http_client::HttpClient;
    use crate::libbc::playlist::PlayList;
    use crate::libbc::search::parse_doc;
    use crate::libbc::shared_data::SharedState;
    use crate::models::bc_discover_json::DiscoverJsonRequest;
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
        let s = String::from("all r&b/soul");
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
