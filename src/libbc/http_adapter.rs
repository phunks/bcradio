use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use bytes::BytesMut;
use curl::easy::{Easy2, HttpVersion, List};
use curl::multi::{Easy2Handle, Multi};
use scraper::Html;
use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};
use simd_json::OwnedValue as Value;

use crate::debug_println;
use crate::libbc::args::args_no_ssl_verify;
use crate::libbc::http_client::{ACCEPT_HEADER, USER_AGENT, Collector};
use crate::libbc::search::{base_url, parse_doc};
use crate::models::search_models::{Current, ItemPage, TrackInfo};
use crate::models::shared_data_models::{ResultsJson, Track};

fn download<T>(multi: &mut Multi, token: usize, url: &str) -> Result<Easy2Handle<Collector<T>>> {
    let mut easy = Easy2::new(Collector {
        res: Vec::new(),
        dat: Default::default(),
    });
    easy.url(url)?;
    easy.useragent(USER_AGENT)?;
    easy.timeout(Duration::new(30, 0))?;
    easy.pipewait(true)?;
    easy.http_version(HttpVersion::V2TLS).unwrap();
    if args_no_ssl_verify() {
        easy.ssl_verify_peer(false)?;
    }

    let mut list = List::new();
    list.append(ACCEPT_HEADER)?;
    list.append("Accept-Encoding: gzip")?;
    list.append("Content-Encoding: gzip")?;
    easy.http_headers(list)?;

    let mut handle = multi.add2(easy)?;
    handle.set_token(token)?;

    Ok(handle)
}

pub fn http_adapter<R, T>(
    urls: Vec<String>,
    mut a: impl FnMut(&mut Easy2Handle<Collector<T>>),
    b: impl FnOnce(HashMap<usize, Easy2Handle<Collector<T>>>) -> R,
) -> Result<R> {
    let mut multi = Multi::new();
    let mut handles = urls
        .iter()
        .enumerate()
        .map(|(token, url)|
            Ok((token, download(&mut multi, token, url).unwrap())))
        .collect::<Result<HashMap<_, _>>>().unwrap();

    let mut still_alive = true;
    while still_alive {
        if multi.perform().unwrap() == 0 {
            still_alive = false;
        }

        multi.messages(|message| {
            let token = message.token().expect("failed to get the token");
            let handle = handles
                .get_mut(&token)
                .expect("the download value should exist in the HashMap");

            match message
                .result_for2(handle)
                .expect("token mismatch with the `EasyHandle`")
            {
                Ok(()) => {
                    let _http_status = handle
                        .response_code()
                        .expect("HTTP request finished without status code");

                    debug_println!(
                        "R: Transfer succeeded (Status: {}) {} (Download length: {})\r",
                        _http_status,
                        urls[token],
                        handle.get_ref().res.len()
                    );

                    a(handle);
                }
                Err(_e) => {
                    debug_println!("E: {} - <{}>\r", _e, urls[token]);
                }
            }
        });

        if still_alive {
            multi.wait(&mut [], Duration::from_secs(6)).unwrap();
        }
    }

    Ok(b(handles))
}

pub fn html_to_json(res: Vec<u8>) -> Result<Value> {
    let html = String::from_utf8(res)?;
    let doc = Html::parse_document(&html);

    let c = parse_doc(doc.clone(),
                      "script[data-tralbum]",
                      "data-tralbum")?;

    let mut b = BytesMut::new();
    b.extend_from_slice(c.as_ref());
    Ok(simd_json::from_slice(&mut b)?)
}

fn j2t(json: Value) -> Result<Vec<Track>> {
    let item_url = json["url"].to_string();
    let base_item_url = base_url(item_url.clone());
    let item_path = match json.get("album_url") {
        Some(a) => {
            if !a.to_string().is_empty() && !base_item_url.is_empty() {
                format!("{}{}", base_item_url, a)
            } else {
                String::from("")
            }
        }
        None => String::from(""),
    };

    let tracks: ItemPage = ItemPage {
        current: Current {
            title: json["current"]["title"].to_string(),
            art_id: json["art_id"].as_i64(),
            band_id: json["current"]["band_id"].as_i64().unwrap(),
            release_date: json["current"]["publish_date"].to_string(),
        },
        artist: json["artist"].to_string(),
        trackinfo: simd_json::serde::from_refowned_value::<Vec<TrackInfo>>(
            &json["trackinfo"]).unwrap(),
        album_url: Option::from(item_path),
        item_url: Option::from(item_url),
    };

    let mut v: Vec<Track> = Vec::new();

    for i in tracks.trackinfo.iter() {
        if i.clone().file.is_none() {
            continue;
        };
        let t = Track {
            album_title: tracks.current.title.to_owned(),
            artist_name: tracks.artist.to_owned(),
            art_id: tracks.current.art_id,
            band_id: tracks.current.band_id,
            url: i.clone().file.unwrap().mp3_128.to_owned().unwrap(),
            duration: i.duration,
            track: i.title.to_owned().unwrap(),
            buffer: vec![],
            results: ResultsJson::Search(Box::new(tracks.clone())),
            genre: None,
            subgenre: None,
        };
        v.push(t);
    }
    Ok(v)
}

pub fn json_to_track(json: Value) -> Result<Vec<Track>> {
    j2t(json)
}
