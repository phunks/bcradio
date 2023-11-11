
use serde_json::{from_slice, Value};
use scraper::{Html, Selector};
use inquire::Select;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
use std::collections::VecDeque;
use anyhow::Result;
use async_trait::async_trait;
use rand::thread_rng;
use rand::seq::SliceRandom;
use reqwest::Response;
use crate::models::index_models::{MessageForIndexPage, IndexG, IndexT};
use crate::libbc::shared_data::{get_request, SharedState, Track};
use crate::libbc::stream_adapter::StreamAdapter;
use crate::models::bc_models::{BandCampJsonMessage, Item};
use crate::debug_println;

#[async_trait]
pub trait PlayList<'a> {
    async fn generate_playlist_url(&self) -> Result<()>;
    async fn text_to_json(res: Response) -> Result<Value>;
    async fn excerpt_json_from_message(self, json: Value) -> Result<Vec<String>>;
    async fn append_url_parameter(&self, url: String, n: i32) -> String;
    async fn ask_url(&self) -> Result<String>;
    fn select_url(&self, t: Value) -> Result<String>;
    fn get_render_config(&self) -> RenderConfig;
    fn calculate_page_number(&self, n: usize) -> usize;
    fn generating_shuffled_pagination(&self, a: usize);
    async fn get_json_bc_message(&self, url: String) -> Result<(VecDeque<Track>, i64)>;
    async fn get_message_index(&self, url: String) -> Result<(VecDeque<Track>, i64)>;
    fn append_track_list(&self, items: Vec<Item>) -> Result<VecDeque<Track>>;
}

#[async_trait]
impl<'a> PlayList<'a> for SharedState {
    async fn generate_playlist_url(&self) -> Result<()> {
        let l = self.get_queue_length_from_truck_list();
        Ok(if l < 1 { // add playlist
            if self.get_selected_url_list_length() > 0_usize {
                let mut pv: VecDeque<Track> = VecDeque::new();
                if self.get_random_page_list_length() > 0_usize {
                    for i in 0..self.get_selected_url_list_length() {
                        let url = self.append_url_parameter(self.get_selected_url(),
                                                            self.get_rnd_pages(0) as i32).await;
                        let (mut p, _) = self.get_json_bc_message(url.clone()).await?;
                        pv.append(&mut p);
                        self.drain_rnd_page_list_element(1);
                        if i > 1 { break }
                    }
                    pv.make_contiguous().shuffle(&mut thread_rng());
                    self.append_tracklist(pv);
                } else {
                    #[cfg(not(debug))]
                        let url = self.get_selected_url();
                    #[cfg(debug)]
                        let url = String::from("http://localhost:8080/get_web?gn=0");
                    debug_println!("debug: url {:?}", url);
                    let t = self.append_url_parameter(url.clone(), 0).await;
                    let (_, tcnt) = self.get_json_bc_message(t.clone()).await?;
                    debug_println!("debug: total_count {:?}", tcnt);
                    let page_max = self.calculate_page_number(tcnt as usize);
                    self.generating_shuffled_pagination(page_max);
                    let mut url_list = Vec::new();
                    for i in 0..page_max {
                        let p = self.append_url_parameter(url.clone(),
                                                          self.get_rnd_pages(0) as i32).await;
                        url_list.append(&mut vec![p]);
                        self.drain_rnd_page_list_element(1);
                        if i > 1 { break }
                    }

                    let content_type = "application/json";
                    let client = self.gh_client(content_type).unwrap();
                    let r = self.clone().bulk_url(client, url_list,
                                                  <SharedState as PlayList>::text_to_json,
                                                  <SharedState as PlayList>::excerpt_json_from_message).await;
                    for i in r? {
                        let a = serde_json::from_str::<Track>(&i)?;
                        pv.push_front(a);
                    }
                    pv.make_contiguous().shuffle(&mut thread_rng());
                    self.set_total_count(tcnt);
                    self.append_tracklist(pv);
                }
            }
        })
    }

    async fn text_to_json(res: Response) -> Result<Value> {
        let a = res.text().await?;
        Ok(from_slice(a.as_ref())?)
    }

    async fn excerpt_json_from_message(self, json: Value) -> Result<Vec<String>> {
        let message = serde_json::from_str::<BandCampJsonMessage>(&json.to_string())?;
        let items = message.items;
        debug_println!("debug: get_message => {}", items.len());
        let test_list = self.append_track_list(items)?;
        let mut v: Vec<String> = Vec::new();
        for t in test_list {
            v.append(&mut vec![serde_json::to_string(&t)?]);
        }
        Ok(v)
    }

    async fn append_url_parameter(&self, url: String, n: i32) -> String {
        format!("{}{}", url, format_args!("&p={}", n))
    }

    async fn ask_url(&self) -> Result<String>{
        #[cfg(not(debug))]
            let burl = String::from("https://bandcamp.com");
        #[cfg(debug)]
            let burl = String::from("http://localhost:8080");

        let buf = get_request(burl).await?.text().await?;

        let doc = Html::parse_document(&buf);
        let c = doc.select(&Selector::parse("div[id='pagedata']")
                        .unwrap()).next().unwrap().value().attr("data-blob").unwrap();
        let json: Result<Value, serde_json::Error> = from_slice(c.as_ref());
        let t = &json?["discover_2015"]["options"];
        let url = self.select_url(t.clone())?;

        Ok(url)
    }

    fn select_url(&self, t: Value) -> Result<String> {
        inquire::set_global_render_config(self.get_render_config());

        let map = t.as_object().unwrap();
        let g: Vec<IndexG> = serde_json::from_str(&map["g"].to_string())?;
        let index_genre:Vec<&String> = g.iter().map(|x|&x.name).collect();
        let mut url: String = String::new();
        loop {
            let genre_ans = Select::new("genre?", index_genre.clone()).prompt();
            if genre_ans.is_err() { continue }
            let genre_value = g.iter().find(|&x| &&x.name == genre_ans.as_ref().unwrap());
            let genre_type_len = &map["t"][&genre_value.unwrap().value];

            if genre_type_len.as_array().is_none() {
                url = format!("https://bandcamp.com/api/discover/3/get_web?g={g}&s=top&gn=0&f=all&lo=true&lo_action_url=https%3A%2F%2Fbandcamp.com", g = genre_value.unwrap().value);
                break;
            } else {
                let _t: Vec<IndexT> = serde_json::from_str(&map["t"][&genre_value.unwrap().value].to_string())?;
                let index_music_types = _t.iter().map(|x|&x.name).collect();
                let music_type_ans = Select::new("music types?", index_music_types).prompt();
                if music_type_ans.is_err() { continue }
                let music_type_value = _t.iter().find(|&x| &&x.name == music_type_ans.as_ref().unwrap());
                let t_opt: String = if music_type_value.unwrap().value.starts_with("all-") {
                    String::from("&w=0")
                } else {
                    format!("&t={t}", t = &music_type_value.unwrap().value)
                };

                url = format!("https://bandcamp.com/api/discover/3/get_web?g={g}&s=top{t}&gn=0&f=all&lo=true&lo_action_url=https%3A%2F%2Fbandcamp.com", g = genre_value.unwrap().value, t = t_opt);
                break;
            }
        }
        Ok(url)
    }

    fn get_render_config(&self) -> RenderConfig {
        let mut render_config = RenderConfig::default();
        // help message
        render_config.help_message = StyleSheet::new()
            .with_fg(Color::rgb(150,150,140));
        // question prompt
        render_config.prompt_prefix = Styled::new("?")
            .with_fg(Color::rgb(150, 150, 140));
        // cursor
        render_config.highlighted_option_prefix = Styled::new(">")
            .with_fg(Color::rgb(150,250,40));
        // focus
        render_config.selected_option = Some(StyleSheet::new()
            .with_fg(Color::rgb(250,180,40)));

        render_config.answer = StyleSheet::new()
            .with_attr(Attributes::ITALIC)
            .with_attr(Attributes::BOLD)
            .with_fg(Color::rgb(220,220,240));

        render_config
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
    async fn get_json_bc_message(&self, url: String) -> Result<(VecDeque<Track>, i64)> {
        let message = get_request(url).await?.json::<BandCampJsonMessage>().await?;
        let items = message.items;
        debug_println!("debug: get_bcmessage => {}", items.len());
        let test_list = self.append_track_list(items)?;
        let count = message.total_count;
        Ok((test_list, count))
    }

    async fn get_message_index(&self, url: String) -> Result<(VecDeque<Track>, i64)> {
        let index = reqwest::get(url.as_str()).await?;
        let doc = Html::parse_document(&index.text().await?);
        let c = doc.select(&Selector::parse("div[id='pagedata']")
            .unwrap()).next().unwrap().value().attr("data-blob").unwrap();
        let json: Result<Value, serde_json::Error> = from_slice(c.as_ref());
        let t = &json.unwrap()["discover_2015"]["initial"];
        let message: MessageForIndexPage = serde_json::from_str(&t.to_string()).unwrap();
        let items = message.items;
        debug_println!("debug: get_message_index => {}", items.len());
        let test_list = self.append_track_list(items)?;
        let count = message.total_count;
        Ok((test_list, count))
    }

    fn append_track_list(&self, items: Vec<Item>) -> Result<VecDeque<Track>> {
        let mut test_list = VecDeque::new();
        for i in items.iter() {
            test_list.append(&mut VecDeque::from([Track {
                genre_text: i.genre_text.to_string(),
                album_title: i.primary_text.to_string(),
                artist_name: i.secondary_text.to_string(),
                art_id: Option::from(i.art_id),
                band_id: i.band_id,
                url: i.featured_track.file.mp3_128.to_string(),
                duration: i.featured_track.duration,
                track: i.featured_track.title.to_string(),
                buffer: vec![],
            }]));
        }
        Ok(test_list)
    }
}