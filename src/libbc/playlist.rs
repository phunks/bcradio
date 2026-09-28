use std::collections::VecDeque;
use std::sync::LazyLock;

use crate::libbc::ai;
use crate::libbc::http_client::{get_request, post_request};
use crate::libbc::player;
use crate::libbc::progress_bar::{destroy, GenerationStatus};
use crate::libbc::search::parse_doc;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal;
use crate::models::bc_discover_index::{DiscoverIndexRequest, Element, PostData};
use crate::models::bc_discover_json::{DiscoverJsonRequest, Results};
use crate::models::bc_discover_tags::{DiscoverTagsJson, Struct, TagsPostData};
use crate::models::bc_error::BcradioError;
use crate::models::shared_data_models::{ResultsJson, Track};
use crate::{ceil, format_duration, lazy_regex};
use anyhow::{Context, Error, Result};
use bytes::BytesMut;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
use inquire::{InquireError, Select};
use itertools::Itertools;
use regex::Regex;
use scraper::Html;
use tracing::debug;

#[derive(Debug)]
pub enum Selection {
    Discover(PostData),
    AiInput,
}

const AI_INPUT: &str = "AI input";

fn genre_options(genres: &[Element]) -> Vec<String> {
    std::iter::once(AI_INPUT.to_owned())
        .chain(genres.iter().map(|genre| genre.label.clone()))
        .collect()
}

fn cached_genres_or_error(
    cached: (Vec<Element>, Vec<Element>),
    error: anyhow::Error,
) -> Result<(Vec<Element>, Vec<Element>)> {
    if cached.0.is_empty() {
        Err(error).context("failed to load Bandcamp genres (no cached genres available)")
    } else {
        tracing::warn!("failed to refresh Bandcamp genres; using cached genres: {error:#}");
        Ok(cached)
    }
}

pub trait PlayList {
    async fn ask(&self) -> Result<Selection>;
    fn silent(&self, genre: Option<String>, sub_genre: Option<String>) -> Result<PostData>;
    async fn store_results(&self, post_data: &PostData) -> Result<()>;
    async fn fill_playlist(&self) -> Result<()>;
    async fn discover_index(&self, url: &str) -> Result<DiscoverIndexRequest>;
    async fn discover_json(&self, post_data: &PostData) -> Result<Vec<Results>>;
    async fn discover_tags_json(&self, post_data: &TagsPostData) -> Result<Vec<Element>>;
    async fn choice(&self) -> Result<Selection>;
    fn gen_track_list(&self, items: &[Results]) -> Result<VecDeque<Track>>;
    async fn top_menu(&self) -> Result<()>;
}

impl PlayList for SharedState {
    async fn ask(&self) -> Result<Selection> {
        let _input = self.input_gate.pause();
        self.choice().await
    }

    fn silent(&self, genre: Option<String>, sub_genre: Option<String>) -> Result<PostData> {
        let v = [genre, sub_genre]
            .into_iter()
            .flatten()
            .map(|x| slug(&x))
            .filter(|i| !i.is_empty())
            .collect::<Vec<_>>();

        let post_data = PostData {
            tag_norm_names: v,
            ..Default::default()
        };

        Ok(post_data)
    }

    async fn store_results(&self, post_data: &PostData) -> Result<()> {
        let res = self.discover_json(post_data).await?;
        let aa = self.gen_track_list(&res)?;
        self.append_tracklist(aa);
        Ok(())
    }

    async fn fill_playlist(&self) -> Result<()> {
        let l = self.queue_length_from_truck_list();
        if l < 2 {
            if self.is_ai_playlist() {
                if let Some((description, terms, generation)) = self.claim_ai_refill() {
                    let state = self.clone();
                    tokio::spawn(async move {
                        let _status = GenerationStatus::new(true);
                        let result =
                            ai::generate_playlist(&description, &terms, &state.recent_songs())
                                .await;
                        state.finish_ai_refill(generation, result);
                    });
                }
                return Ok(());
            }
            match self.next_post().cursor {
                Some(_) => {
                    let post_data = &self.next_post();
                    let res = self.discover_json(post_data).await?;
                    self.append_tracklist(self.gen_track_list(&res)?);
                }
                None => {
                    // The final queued track must play before prompting for another
                    // selection (notably when an AI playlist has no discover cursor).
                    if l > 0 {
                        return Ok(());
                    }
                    destroy();
                    terminal::clear_screen();
                    println!("playlist is empty.\r");

                    match self.ask().await {
                        Ok(Selection::Discover(post_data)) => {
                            self.store_results(&post_data).await?
                        }
                        Ok(Selection::AiInput) => {
                            if !player::ai_playlist(self, false).await? {
                                return Err(Error::from(BcradioError::Quit));
                            }
                        }
                        _ => return Err(Error::from(BcradioError::Quit)),
                    }
                }
            }
        }
        Ok(())
    }

    async fn discover_index(&self, url: &str) -> Result<DiscoverIndexRequest> {
        let buf = get_request(url).await?;

        let slice = String::from_utf8(buf)?;
        let doc = Html::parse_document(&slice);

        let c = parse_doc(doc, "div[id='DiscoverApp']", "data-blob")?;

        let json: Result<DiscoverIndexRequest, serde_json::Error> =
            serde_json::from_slice(&bytes_mut(c.as_bytes())?);

        match json {
            Ok(r) => Ok(r),
            Err(e) => {
                eprintln!("{e}");
                Err(Error::from(e))
            }
        }
    }

    async fn discover_json(&self, post_data: &PostData) -> Result<Vec<Results>> {
        let url = "https://bandcamp.com/api/discover/1/discover_web";
        let a = post_request(url, post_data).await;
        debug!(
            "discover response: {} bytes",
            a.as_ref().map_or(0, Vec::len)
        );
        let json: DiscoverJsonRequest = serde_json::from_slice(&bytes_mut(a?.as_slice())?)?;

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

        Ok(s.to_owned()
            .related_tags
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

    async fn choice(&self) -> Result<Selection> {
        inquire::set_global_render_config(render_config());
        let url = "https://bandcamp.com/discover/";

        loop {
            let r = self.discover_index(url).await;
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
                Err(e) => cached_genres_or_error(self.get_genres(), e)?,
            };

            let _genre_ans = Select::new("genre?", genre_options(&g))
                .with_raw_return(true)
                .prompt();

            let genre_ans = match _genre_ans {
                Ok(ref choice) => choice,
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

            if genre_ans == AI_INPUT {
                return Ok(Selection::AiInput);
            }
            self.set_genre(genre_ans);

            let element = pick_element(&g, genre_ans);
            match element {
                Some(ref genre) => {
                    if genre.label.starts_with("all genres") {
                        return Ok(Selection::Discover(PostData {
                            tag_norm_names: Vec::new(),
                            ..Default::default()
                        }));
                    }
                }
                None => {
                    // tag request
                    let parent_labels = genre_list(&t, &g, genre_ans);

                    return match parent_labels.len() {
                        0 => {
                            let mut subgenres = Vec::<Element>::new();
                            let mut a = self
                                .discover_tags_json(&TagsPostData {
                                    tag_names: vec![slug(genre_ans)],
                                    ..Default::default()
                                })
                                .await?;
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

                            Ok(Selection::Discover(PostData {
                                tag_norm_names: tags,
                                ..Default::default()
                            }))
                        }
                        1 => {
                            // subgenre found, redirect
                            self.set_genre(&parent_labels[0]);
                            self.set_subgenre(genre_ans);
                            Ok(Selection::Discover(PostData {
                                tag_norm_names: vec![slug(&parent_labels[0]), slug(genre_ans)],
                                ..Default::default()
                            }))
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
                            Ok(Selection::Discover(PostData {
                                tag_norm_names: vec![slug(ans), slug(genre_ans)],
                                ..Default::default()
                            }))
                        }
                    };
                }
            };

            let element = match element {
                Some(element) => element,
                None => return Err(Error::from(BcradioError::PhaseError)),
            };

            let mut a = t
                .iter()
                .filter(|&x| x.parent_slug.as_ref() == Some(&element.slug))
                .cloned()
                .collect::<Vec<Element>>();

            return if a.is_empty() {
                // audiobooks, podcasts..
                self.set_subgenre("");

                Ok(Selection::Discover(PostData {
                    tag_norm_names: vec![element.slug.to_string()],
                    ..Default::default()
                }))
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

                Ok(Selection::Discover(PostData {
                    tag_norm_names: tags,
                    ..Default::default()
                }))
            };
        }
    }

    fn gen_track_list(&self, items: &[Results]) -> Result<VecDeque<Track>> {
        let mut track_list = VecDeque::new();
        for i in items.iter() {
            let Some(featured_track) = i.featured_track.as_ref() else {
                continue;
            };
            track_list.append(&mut VecDeque::from([Track {
                album_title: i.title.to_owned(),
                artist_name: featured_track.band_name.to_owned(),
                art_id: i.primary_image.image_id,
                band_id: i.band_id,
                url: featured_track.stream_url.to_owned(),
                duration: featured_track.duration.unwrap_or_default(),
                track: featured_track.title.to_owned(),
                buffer: vec![],
                results: ResultsJson::Select(Box::new(i.clone())),
                genre: Some(self.get_genre().to_owned()),
                subgenre: Some(self.get_subgenre().to_owned()),
            }]));
        }
        Ok(track_list)
    }

    async fn top_menu(&self) -> Result<()> {
        let selection = {
            let _screen = terminal::AlternateScreen::enter(false)?;
            terminal::clear_screen();
            self.ask().await
        };
        match selection {
            Ok(Selection::Discover(post_data)) => {
                self.clear_all_tracklist();
                self.store_results(&post_data).await?;
            }
            Ok(Selection::AiInput) => {
                player::ai_playlist(self, true).await?;
            }
            Err(e) => match e.downcast_ref() {
                Some(BcradioError::InvalidUrl | BcradioError::Cancel) => {}
                _ => return Err(e),
            },
        }
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
                .filter(|&b| x.parent_slug.as_ref() == Some(&b.slug))
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
    let b = &RE1.replace_all(s.trim(), "");
    let b = &RE2.replace_all(b, "");
    let b = &RE3.replace_all(b, "-");
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
    use crate::models::bc_discover_json::DiscoverJsonRequest;
    use crate::models::shared_data_models::Track;
    use serde_json::json;
    use std::collections::VecDeque;
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
    fn ai_input_is_first_even_without_discover_genres() {
        assert_eq!(super::genre_options(&[]), vec!["AI input"]);
        assert_eq!(
            super::genre_options(&[crate::models::bc_discover_index::Element {
                label: "jazz".into(),
                ..Default::default()
            }]),
            vec!["AI input", "jazz"]
        );
    }

    #[test]
    fn genre_fetch_failure_requires_cached_genres() {
        let cause = anyhow::anyhow!("connection refused");
        let err = super::cached_genres_or_error((vec![], vec![]), cause).unwrap_err();
        assert!(format!("{err:#}").contains("connection refused"));
        assert!(format!("{err:#}").contains("no cached genres"));

        let cached = vec![crate::models::bc_discover_index::Element {
            label: "jazz".into(),
            ..Default::default()
        }];
        let (genres, _) =
            super::cached_genres_or_error((cached.clone(), vec![]), anyhow::anyhow!("offline"))
                .unwrap();
        assert_eq!(genres[0].label, cached[0].label);
    }

    #[tokio::test]
    async fn last_ai_track_is_not_replaced_before_playback() {
        let state = SharedState::default();
        assert!(state.start_ai_playlist(
            "last track".into(),
            vec![],
            VecDeque::from([Track {
                track: "last".into(),
                ..Default::default()
            }])
        ));
        state.fill_playlist().await.unwrap();
        assert_eq!(state.get_tracklist()[0].track, "last");
    }

    #[test]
    fn skips_discover_results_without_featured_track() {
        let playable = json!({
            "title": "Album",
            "item_url": "https://example.com/album",
            "price": {"amount": 0, "currency": "USD", "is_money": false},
            "result_type": "album",
            "band_id": 42,
            "band_name": "Artist",
            "band_url": "https://example.com/artist",
            "band_genre_id": 1,
            "release_date": "2026-01-01",
            "featured_track": {
                "band_id": 42,
                "title": "Song",
                "band_name": "Artist",
                "stream_url": "https://example.com/song.mp3",
                "duration": 319.037
            },
            "primary_image": {"image_id": 7, "is_art": true}
        });
        let mut missing = playable.clone();
        missing.as_object_mut().unwrap().remove("featured_track");
        let mut null_result = playable.clone();
        null_result["featured_track"] = serde_json::Value::Null;
        let response: DiscoverJsonRequest = serde_json::from_value(json!({
            "results": [null_result, playable, missing],
            "result_count": 3,
            "batch_result_count": 3,
            "cursor": null
        }))
        .unwrap();

        let tracks = SharedState::default()
            .gen_track_list(&response.results)
            .unwrap();
        assert_eq!(tracks.len(), 1);
        let track = &tracks[0];
        assert_eq!(track.track, "Song");
        assert_eq!(track.artist_name, "Artist");
        assert_eq!(track.url, "https://example.com/song.mp3");
        assert_eq!(track.duration, 319.037);
    }

    #[test]
    fn test_menu() {
        runtime().block_on(async {
            init_args();
            let s = SharedState::default();
            let aa = s.choice().await.unwrap();
            println!("{:?}", aa);
        });
    }
}
