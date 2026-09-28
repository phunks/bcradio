use crate::libbc::http_client::client_builder;
use crate::libbc::search::{base_url, parse_doc};
use crate::models::search_models::{Current, ItemPage, TrackInfo};
use crate::models::shared_data_models::Track;
use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use futures::{stream, StreamExt, TryStreamExt};
use reqwest::header;
use scraper::Html;
use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};
use simd_json::OwnedValue as Value;
use std::future::Future;

const PARALLEL_REQUESTS: usize = 4;
type FA<R> = fn(res: Bytes) -> R;

pub async fn http_adapter<R>(
    urls: Vec<String>,
    plug: FA<impl Future<Output = Result<Vec<R>>> + Send + 'static>,
) -> Result<Vec<R>>
where
    R: Send + 'static,
{
    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", header::HeaderValue::from_static("*/*"));
    headers.insert(
        "Accept-Encoding",
        header::HeaderValue::from_static("gzip;q=0.4"),
    );
    headers.insert("Content-Encoding", header::HeaderValue::from_static("gzip"));
    let client = client_builder(headers)?;

    stream::iter(urls)
        .map(|url| {
            let client = client.clone();
            async move {
                let response = client.get(&url).send().await?.error_for_status()?;
                let body = response.bytes().await?;
                plug(body)
                    .await
                    .with_context(|| format!("failed to parse search result: {url}"))
            }
        })
        .buffer_unordered(PARALLEL_REQUESTS)
        .try_fold(Vec::<R>::new(), |mut acc, x| async move {
            acc.extend(x);
            Result::<Vec<R>>::Ok(acc)
        })
        .await
}

pub async fn html_to_track(v: Bytes) -> Result<Vec<Track>> {
    match !v.is_empty() {
        true => match html_to_json(v.to_vec()) {
            Ok(t) => j2t(t),
            Err(e) => Err(e),
        },
        _ => Ok(Vec::new()),
    }
}

pub fn html_to_json(res: Vec<u8>) -> Result<Value> {
    let html = String::from_utf8(res)?;
    let doc = Html::parse_document(&html);

    let c = parse_doc(doc.clone(), "script[data-tralbum]", "data-tralbum")?;

    let mut b = BytesMut::new();
    b.extend_from_slice(c.as_ref());
    Ok(simd_json::from_slice(&mut b)?)
}

pub fn j2t(json: Value) -> Result<Vec<Track>> {
    let item_url = json["url"].to_string();
    let base_item_url = base_url(&item_url);
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
            band_id: json["current"]["band_id"]
                .as_i64()
                .context("search result missing current.band_id")?,
            release_date: json["current"]["publish_date"].to_string(),
        },
        artist: json["artist"].to_string(),
        trackinfo: simd_json::serde::from_refowned_value::<Vec<TrackInfo>>(&json["trackinfo"])?,
        album_url: Option::from(item_path),
        item_url: Option::from(item_url),
    };

    let mut v: Vec<Track> = Vec::new();

    for i in tracks.trackinfo.iter() {
        let Some(url) = i.file.as_ref().and_then(|file| file.mp3_128.as_ref()) else {
            continue;
        };
        let title = i
            .title
            .as_ref()
            .context("playable search track missing title")?;
        let t = Track {
            album_title: tracks.current.title.to_owned(),
            artist_name: tracks.artist.to_owned(),
            art_id: tracks.current.art_id,
            band_id: tracks.current.band_id,
            url: url.clone(),
            duration: i.duration,
            track: title.clone(),
            // buffer: vec![],
            // results: ResultsJson::Search(Box::new(tracks.clone())),
            // genre: None,
            // subgenre: None,
            ..Default::default()
        };
        v.push(t);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(tracks: &str, band_id: &str) -> Value {
        let text = format!(
            r#"{{"url":"https://example.com/album/test","current":{{"title":"Album","band_id":{band_id},"publish_date":"today"}},"art_id":1,"artist":"Artist","trackinfo":{tracks}}}"#
        );
        simd_json::from_slice(&mut text.into_bytes()).unwrap()
    }

    #[test]
    fn skips_tracks_without_playable_mp3() {
        let tracks = r#"[{"id":1,"track_id":1,"license_type":0,"duration":10.0,"title":"Unavailable","file":{"mp3-128":null}},{"id":2,"track_id":2,"license_type":0,"duration":12.0,"title":"Playable","file":{"mp3-128":"https://example.com/song.mp3"}}]"#;
        let result = j2t(page(tracks, "1")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].track, "Playable");
    }

    #[test]
    fn malformed_playable_track_returns_error_instead_of_panicking() {
        let tracks = r#"[{"id":1,"track_id":1,"license_type":0,"duration":10.0,"file":{"mp3-128":"https://example.com/song.mp3"}}]"#;
        assert!(j2t(page(tracks, "1"))
            .unwrap_err()
            .to_string()
            .contains("title"));
        assert!(j2t(page("[]", "null"))
            .unwrap_err()
            .to_string()
            .contains("band_id"));
    }
}
