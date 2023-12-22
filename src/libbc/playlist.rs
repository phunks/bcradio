use std::collections::VecDeque;
use std::process;

use anyhow::Result;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use inquire::{InquireError, Select};
use inquire::ui::{Attributes, Color, RenderConfig, Styled, StyleSheet};
use rand::seq::SliceRandom;
use rand::thread_rng;
use reqwest::Response;
use scraper::{Html, Selector};
use simd_json::OwnedValue as Value;

use crate::debug_println;
use crate::libbc::http_client::{Client, get_request};
use crate::libbc::shared_data::SharedState;
use crate::libbc::stream_adapter::StreamAdapter;
use crate::models::bc_models::{BandCampJsonMessage, Item};
use crate::models::index_models::{IndexG, IndexT, MessageForIndexPage};
use crate::models::shared_data_models::Track;


#[async_trait]
pub trait PlayList {
    async fn generate_playlist_url(&self) -> Result<()>;
    async fn resp_to_json(res: Response) -> Result<Value>;
    async fn vec_to_json(vec: Vec<u8>) -> Result<Value>;
    async fn excerpt_json_from_message(self, json: Value) -> Result<Vec<Track>>;
    async fn append_url_parameter(&self, url: String, n: i32) -> String;
    async fn ask_url(&self) -> Result<String>;
    fn select_url(&self, t: serde_json::Value) -> Result<String>;
    fn get_render_config(&self) -> RenderConfig;
    fn calculate_page_number(&self, n: usize) -> usize;
    fn generating_shuffled_pagination(&self, a: usize);
    async fn get_json_bc_message(&self, url: String) -> Result<(Vec<Track>, i64)>;
    async fn get_json_bc_message_with_curl(&self, url: String) -> Result<(Vec<Track>, i64)>;
    async fn get_message_index(&self, url: String) -> Result<(Vec<Track>, i64)>;
    fn append_track_list(&self, items: Vec<Item>) -> Result<Vec<Track>>;
}

#[async_trait]
impl PlayList for SharedState {
    async fn generate_playlist_url(&self) -> Result<()> {
        let l = self.queue_length_from_truck_list();
        if l < 1 {
            // add playlist
            if self.get_selected_url_list_length() > 0_usize {
                let mut pv: VecDeque<Track> = VecDeque::new();
                if self.get_random_page_list_length() > 0_usize {
                    for i in 0..self.get_selected_url_list_length() {
                        let url = self
                            .append_url_parameter(
                                self.get_selected_url(),
                                self.get_rnd_pages(0) as i32,
                            )
                            .await;
                        let (p, _) = self.get_json_bc_message_with_curl(url.to_owned()).await?;
                        pv.append(&mut VecDeque::from(p));
                        self.drain_rnd_page_list_element(1);
                        if i > 1 {
                            break;
                        }
                    }
                    pv.make_contiguous().shuffle(&mut thread_rng());
                    self.append_tracklist(pv);
                } else {
                    #[cfg(not(debug))]
                        let url = self.get_selected_url();
                    #[cfg(debug)]
                        let url = String::from("http://localhost:8080/get_web?gn=0");
                    debug_println!("debug: url {:?}", url);

                    let url = self.append_url_parameter(url.to_owned(), 0).await;
                    let (_, tcnt) = self.get_json_bc_message_with_curl(url.to_owned()).await?;
                    debug_println!("debug: total_count {:?}", tcnt);

                    let page_max = self.calculate_page_number(tcnt as usize);
                    self.generating_shuffled_pagination(page_max);
                    let mut url_list = Vec::new();
                    for i in 0..page_max {
                        let p = self
                            .append_url_parameter(url.to_owned(), self.get_rnd_pages(0) as i32)
                            .await;
                        url_list.append(&mut vec![p]);
                        self.drain_rnd_page_list_element(1);
                        if i > 1 {
                            break;
                        }
                    }

                    let r = self
                        .to_owned()
                        .bulk_url(
                            url_list,
                            <SharedState as PlayList>::vec_to_json,
                            <SharedState as PlayList>::excerpt_json_from_message,
                        )
                        .await;
                    for i in r? { pv.push_front(i) }
                    pv.make_contiguous().shuffle(&mut thread_rng());
                    self.set_total_count(tcnt);
                    self.append_tracklist(pv);
                }
            }
        }
        Ok(())
    }

    async fn resp_to_json(res: Response) -> Result<Value> {
        let bytes = res.bytes().await?;
        let val = simd_json::from_slice(&mut bytes_mut(&bytes)?)?;
        Ok(val)
    }

    async fn vec_to_json(mut vec: Vec<u8>) -> Result<Value> {
        let bytes = vec.as_mut_slice();
        let val = simd_json::from_slice(bytes)?;
        Ok(val)
    }

    async fn excerpt_json_from_message(self, json: Value) -> Result<Vec<Track>> {
        let message = simd_json::serde::from_refowned_value::<BandCampJsonMessage>(&json)?;
        let items = message.items;
        debug_println!("debug: get_message => {}", items.len());
        let track_list = self.append_track_list(items)?;
        let mut v: Vec<Track> = Vec::new();
        for t in track_list {
            v.append(&mut vec![t]);
        }
        Ok(v)
    }

    async fn append_url_parameter(&self, url: String, n: i32) -> String {
        format!("{}{}", url, format_args!("&p={}", n))
    }

    async fn ask_url(&self) -> Result<String> {
        #[cfg(not(debug))]
            let burl = String::from("https://bandcamp.com");
        #[cfg(debug)]
            let burl = String::from("http://localhost:8080");

        let client:Client = Default::default();
        let buf = client.get_curl_request(burl)?.to_vec()?;
        let slice = String::from_utf8(buf)?;
        let doc = Html::parse_document(&slice);

        let c = doc
            .select(&Selector::parse("div[id='pagedata']").unwrap())
            .next()
            .unwrap()
            .value()
            .attr("data-blob")
            .unwrap();
        let json: Result<serde_json::Value, serde_json::Error> = serde_json::from_slice(c.as_ref());
        let t = &json?["discover_2015"]["options"];
        let url = self.select_url(t.to_owned())?;

        Ok(url)
    }

    fn select_url(&self, t: serde_json::Value) -> Result<String> {
        inquire::set_global_render_config(self.get_render_config());

        let map = t.as_object().unwrap();
        let g: Vec<IndexG> = serde_json::from_str(&map["g"].to_string())?;

        let index_genre: Vec<&String> = g.iter().map(|x| &x.name).collect();
        let mut _url = String::new();
        loop {
            let genre_ans = Select::new("genre?", index_genre.to_owned()).prompt();
            match genre_ans {
                Ok(choice) => choice,
                Err(e) => match e {
                    InquireError::OperationCanceled => continue,
                    InquireError::OperationInterrupted => process::exit(0),
                    other_error => panic!("inquire error: {:?}", other_error),
                }
            };
            let genre_value = g.iter().find(|&x| &&x.name == genre_ans.as_ref().unwrap());
            let genre_type_len = &map["t"][&genre_value.unwrap().value];

            if genre_type_len.as_array().is_none() {

                _url = format!("https://bandcamp.com/api/discover/3/get_web?g={g}&s=top&gn=0&f=all&lo=true&lo_action_url=https%3A%2F%2Fbandcamp.com",
                               g = genre_value.unwrap().value);
                break;
            } else {
                let _t: Vec<IndexT> =
                    serde_json::from_value(map["t"][&genre_value.unwrap().value].to_owned())?;
                let index_music_types = _t
                    .iter()
                    .map(|x| &x.name)
                    .collect();
                let music_type_ans = Select::new("music types?", index_music_types).prompt();
                match music_type_ans {
                    Ok(choice) => choice,
                    Err(e) => match e {
                        InquireError::OperationCanceled => continue,
                        InquireError::OperationInterrupted => process::exit(0),
                        other_error => panic!("inquire error: {:?}", other_error),
                    }
                };
                let music_type_value = _t
                    .iter()
                    .find(|&x| &&x.name == music_type_ans.as_ref().unwrap());
                let t_opt: String = if music_type_value.unwrap().value.starts_with("all-") {
                    String::from("&w=0")
                } else {
                    format!("&t={t}", t = &music_type_value.unwrap().value)
                };

                _url = format!("https://bandcamp.com/api/discover/3/get_web?g={g}&s=top{t}&gn=0&f=all&lo=true&lo_action_url=https%3A%2F%2Fbandcamp.com",
                               g = genre_value.unwrap().value,
                               t = t_opt);
                break;
            }
        }
        Ok(_url)
    }

    fn get_render_config(&self) -> RenderConfig {
        RenderConfig {
            help_message: StyleSheet::new().with_fg(Color::rgb(150, 150, 140)), // help message
            prompt_prefix: Styled::new("?").with_fg(Color::rgb(150, 150, 140)), // question prompt
            highlighted_option_prefix: Styled::new(">").with_fg(Color::rgb(150, 250, 40)), // cursor
            selected_option: Some(StyleSheet::new().with_fg(Color::rgb(250, 180, 40))), // focus
            answer: StyleSheet::new()
                .with_attr(Attributes::ITALIC)
                .with_attr(Attributes::BOLD)
                .with_fg(Color::rgb(220, 220, 240)),
            ..Default::default()
        }
    }

    fn calculate_page_number(&self, n: usize) -> usize {
        let a = n % 48;
        let b = n / 48;
        match a {
            0 => b,
            _ => b + 1,
        }
    }

    fn generating_shuffled_pagination(&self, a: usize) {
        let mut v = (0..a).collect::<VecDeque<usize>>();
        v.make_contiguous().shuffle(&mut thread_rng());
        self.set_random_pagination(v);
    }

    /// Get a `BandCampJsonMessage` with REST API.
    async fn get_json_bc_message(&self, url: String) -> Result<(Vec<Track>, i64)> {
        use std::time::Instant; //debug
        let start = Instant::now(); //debug
        let res = get_request(url)
            .await?;
        debug_println!("Debug: {:?}", start.elapsed()); //debug
        let val = <SharedState as PlayList>::resp_to_json(res).await?;
        let message = simd_json::serde::from_refowned_value::<BandCampJsonMessage>(&val)?;
        let items = message.items;
        debug_println!("debug: get_bcmessage => {}", items.len());
        let track_list = self.append_track_list(items)?;
        let count = message.total_count;

        Ok((track_list, count))
    }

    async fn get_json_bc_message_with_curl(&self, url: String) -> Result<(Vec<Track>, i64)> {
        use std::time::Instant; //debug
        let start = Instant::now(); //debug
        let client:Client = Default::default();
        let res = client.get_curl_request(url)?.to_json()?;
        debug_println!("Debug: {:?}", start.elapsed()); //debug
        let message = simd_json::serde::from_refowned_value::<BandCampJsonMessage>(&res)?;
        let items = message.items;
        debug_println!("debug: get curl bcmessage => {}", items.len());
        let track_list = self.append_track_list(items)?;
        let count = message.total_count;

        Ok((track_list, count))
    }

    async fn get_message_index(&self, url: String) -> Result<(Vec<Track>, i64)> {
        let index = reqwest::get(url.as_str()).await?;
        let doc = Html::parse_document(&index.text().await?);
        let c = doc
            .select(&Selector::parse("div[id='pagedata']").unwrap())
            .next()
            .unwrap()
            .value()
            .attr("data-blob")
            .unwrap();

        let json: Result<serde_json::Value, serde_json::Error> = serde_json::from_str(c);
        let t = &json.unwrap()["discover_2015"]["initial"];
        let message = serde_json::from_value::<MessageForIndexPage>(t.to_owned())?;
        let items = message.items;
        debug_println!("debug: get_message_index => {}", items.len());
        let track_list = self.append_track_list(items)?;
        let count = message.total_count;
        Ok((track_list, count))
    }

    fn append_track_list(&self, items: Vec<Item>) -> Result<Vec<Track>> {
        let mut track_list = Vec::new();
        for i in items.iter() {
            track_list.append(&mut Vec::from([Track {
                genre_text: i.genre_text.to_owned(),
                album_title: i.primary_text.to_owned(),
                artist_name: i.secondary_text.to_owned(),
                art_id: Option::from(i.art_id),
                band_id: i.band_id,
                url: i.featured_track.file.mp3_128.to_owned(),
                duration: i.featured_track.duration,
                track: i.featured_track.title.to_owned(),
                buffer: vec![],
            }]));
        }
        Ok(track_list)
    }
}

pub fn bytes_mut(a: &Bytes) -> Result<BytesMut> {
    let mut b = BytesMut::new();
    b.extend_from_slice(a);
    Ok(b)
}
