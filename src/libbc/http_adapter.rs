
use std::future::Future;
use anyhow::{anyhow, Result};
use bytes::{Bytes, BytesMut};
use reqwest::{Client, header};
use scraper::Html;
use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};
use simd_json::OwnedValue as Value;
use futures::{stream, StreamExt, TryStreamExt};

use crate::libbc::search::{base_url, parse_doc};
use crate::models::search_models::{Current, ItemPage, TrackInfo};
use crate::models::shared_data_models::{ResultsJson, Track};


const PARALLEL_REQUESTS: usize = 4;
type FA<R> = fn(res: Bytes) -> R;

pub async fn http_adapter<R>(
    urls: Vec<String>,
    plug: FA<impl Future<Output = Result<Vec<R>>> + Send + 'static >,
) -> Result<Vec<R>>
    where R: Send + 'static
{
    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", header::HeaderValue::from_static("*/*"));
    headers.insert("Accept-Encoding", header::HeaderValue::from_static("gzip;q=0.4"));
    headers.insert("Content-Encoding", header::HeaderValue::from_static("gzip"));
    let client = Client::builder()
        .use_rustls_tls()
        .default_headers(headers)
        .connection_verbose(true)
        .http2_prior_knowledge()
        .build()?;

    stream::iter(urls)
        .map(|url| {
            let client = client.clone();
            tokio::spawn(async move {
                match client.get(url).send().await {
                    Ok(r) => {
                        match r.error_for_status() {
                            Ok(res) => Ok(res),
                            Err(e) => Err(anyhow!("status: {}", e))
                        }
                    },
                    Err(e) => Err(anyhow!("response: {}", e))
                }
            })
        })
        .buffer_unordered(PARALLEL_REQUESTS)
        .filter_map(|x| async move {
            x.ok()?.ok()
        })
        .map(move |v| {
            tokio::spawn(async move {
                plug(v.bytes().await?).await
            })
        })
        .buffer_unordered(PARALLEL_REQUESTS)
        .filter_map(|x| async move { x.ok() })
        .try_fold(Vec::<R>::new(), |mut acc, x| async move {
            for t in x {
                acc.push(t);
            }
            Result::<Vec<R>>::Ok(acc)
        })
        .await
}

pub async fn html_to_track(v: Bytes) -> Result<Vec<Track>> {
    match ! v.is_empty() {
        true => {
            match html_to_json(v.to_vec()) {
                Ok(t) => j2t(t),
                _ => Ok(Vec::new())
            }
        },
        _ => Ok(Vec::new())
    }
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

pub fn j2t(json: Value) -> Result<Vec<Track>> {
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
