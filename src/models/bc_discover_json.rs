use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverJsonRequest {
    pub results: Vec<Results>,
    #[serde(alias = "resultCount")]
    result_count: i32,
    #[serde(alias = "batchResultCount")]
    batch_result_count: i32,
    pub cursor: Option<String>,
    #[serde(alias = "discoverSpecId")]
    discover_spec_id: Option<i32>,
}

impl Clone for DiscoverJsonRequest {
    fn clone(&self) -> DiscoverJsonRequest {
        DiscoverJsonRequest {
            results: self.results.clone(),
            result_count: self.result_count,
            batch_result_count: self.batch_result_count,
            cursor: self.cursor.clone(),
            discover_spec_id: self.discover_spec_id,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub title: String,
    #[serde(alias = "item_url")]
    pub item_url: String,
    pub price: Price,
    #[serde(alias = "result_type")]
    pub result_type: String,
    #[serde(alias = "band_id")]
    pub band_id: i64, //label id
    #[serde(alias = "album_artist")]
    pub album_artist: Option<String>,
    #[serde(alias = "band_name")]
    pub band_name: String, //labels
    #[serde(alias = "band_url")]
    pub band_url: String, //label url
    #[serde(alias = "band_genre_id")]
    pub band_genre_id: i32,
    #[serde(alias = "release_date")]
    pub release_date: String,
    #[serde(alias = "package_info")]
    pub package_info: Option<Vec<Package>>,
    #[serde(alias = "featured_track")]
    pub featured_track: Option<FeaturedTrack>,
    #[serde(alias = "player_data")]
    pub player_data: Option<FeaturedTrack>,
    #[serde(alias = "band_location")]
    pub band_location: Option<String>,
    #[serde(alias = "track_count")]
    track_count: Option<i32>,
    pub duration: Option<f32>,
    #[serde(alias = "primary_image")]
    pub primary_image: PrimaryImage,
}

impl Clone for Results {
    fn clone(&self) -> Results {
        Results {
            title: self.title.clone(),
            item_url: self.item_url.clone(),
            price: self.price.clone(),
            result_type: self.result_type.clone(),
            band_id: self.band_id,
            album_artist: self.album_artist.clone(),
            band_name: self.band_name.clone(),
            band_url: self.band_url.clone(),
            band_genre_id: self.band_genre_id,
            release_date: self.release_date.clone(),
            package_info: self.package_info.clone(),
            featured_track: self.featured_track.clone(),
            player_data: self.player_data.clone(),
            band_location: self.band_location.clone(),
            track_count: self.track_count,
            duration: self.duration,
            primary_image: self.primary_image.clone(),
        }
    }
}

impl Results {
    pub(crate) fn playback_track(&self) -> Option<&FeaturedTrack> {
        self.featured_track
            .as_ref()
            .filter(|track| !track.stream_url.trim().is_empty())
            .or_else(|| {
                self.player_data
                    .as_ref()
                    .filter(|track| !track.stream_url.trim().is_empty())
            })
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturedTrack {
    #[serde(alias = "band_id")]
    pub band_id: i64,
    pub title: String,
    #[serde(alias = "band_name", alias = "artistName", alias = "artist_name")]
    pub band_name: String,
    #[serde(alias = "stream_url")]
    pub stream_url: String,
    pub duration: Option<f32>,
}

impl Clone for FeaturedTrack {
    fn clone(&self) -> FeaturedTrack {
        FeaturedTrack {
            band_id: self.band_id,
            title: self.title.clone(),
            band_name: self.band_name.clone(),
            stream_url: self.stream_url.clone(),
            duration: self.duration,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Package {
    pub title: String,
    pub format: String,
    #[serde(alias = "imageId")]
    pub image_id: i64,
    price: Price,
    #[serde(alias = "typeId")]
    pub type_id: i32,
}

impl Clone for Package {
    fn clone(&self) -> Package {
        Package {
            title: self.title.clone(),
            format: self.format.clone(),
            image_id: self.image_id,
            price: self.price.clone(),
            type_id: self.type_id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Price {
    pub(crate) amount: i64,
    pub(crate) currency: String,
    #[serde(alias = "isMoney")]
    is_money: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryImage {
    #[serde(alias = "image_id")]
    pub image_id: Option<i64>,
    #[serde(alias = "is_art")]
    is_art: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // The API mixes camelCase result/track/image keys with price.is_money.
    fn discover_result() -> serde_json::Value {
        json!({
            "itemId": 860295383,
            "itemType": "a",
            "resultType": "a",
            "title": "Brandon Hombre - Cover Up (pwrep258 - Power House Records)",
            "itemUrl": "https://powerhouserecords.bandcamp.com/album/brandon-hombre-cover-up-pwrep258-power-house-records?from=discover_page",
            "primaryImage": {"imageId": 4206576781_i64, "isArt": true},
            "bandId": 2764668053_i64,
            "albumArtist": null,
            "bandName": "Power House Records",
            "bandUrl": "https://powerhouserecords.bandcamp.com?from=discover_page",
            "price": {"amount": 100, "currency": "EUR", "is_money": true},
            "bandGenreId": 10,
            "bandLocation": "San Francisco, California",
            "featuredTrack": {
                "id": 4078273823_i64,
                "title": "Cover Up",
                "bandName": "Power House Records",
                "bandId": 2764668053_i64,
                "streamUrl": "https://example.com/song.mp3",
                "duration": 258.7
            },
            "releaseDate": "2019-10-25 00:00:00 UTC",
            "trackCount": 1,
            "duration": 258.7001953125,
            "packageInfo": null,
            "playerData": null
        })
    }

    #[test]
    fn decodes_camel_case_discover_response_with_snake_case_price() {
        let body = serde_json::to_vec(&json!({
            "results": [discover_result()],
            "result_count": 1,
            "batch_result_count": 1,
            "cursor": null
        }))
        .unwrap();
        let response: DiscoverJsonRequest =
            crate::libbc::json::decode(&body, "Bandcamp Discover regression fixture").unwrap();
        let result = &response.results[0];
        assert_eq!(result.band_id, 2764668053);
        assert_eq!(result.primary_image.image_id, Some(4206576781));
        assert_eq!(result.primary_image.is_art, Some(true));
        assert!(result.price.is_money);
        let featured = result.featured_track.as_ref().unwrap();
        assert_eq!(featured.band_id, result.band_id);
        assert_eq!(featured.title, "Cover Up");
        assert_eq!(featured.stream_url, "https://example.com/song.mp3");
        assert_eq!(featured.duration, Some(258.7));
    }

    #[test]
    fn decodes_current_player_data_and_legacy_featured_track() {
        for legacy_null in [false, true] {
            let mut value = discover_result();
            value.as_object_mut().unwrap().remove("featuredTrack");
            if legacy_null {
                value["featuredTrack"] = serde_json::Value::Null;
            }
            value["playerData"] = json!({
                "bandId": 42,
                "artistName": "Current Artist",
                "title": "Current Song",
                "streamUrl": "https://example.com/current.mp3",
                "duration": 123.5
            });
            let result: Results = serde_json::from_value(value).unwrap();
            let track = result.playback_track().unwrap();
            assert_eq!(track.band_name, "Current Artist");
            assert_eq!(track.title, "Current Song");
            assert_eq!(track.duration, Some(123.5));
            let tracks = crate::libbc::playlist::PlayList::gen_track_list(
                &crate::libbc::shared_data::SharedState::default(),
                &[result],
            )
            .unwrap();
            assert_eq!(tracks[0].url, "https://example.com/current.mp3");
            assert_eq!(tracks[0].artist_name, "Current Artist");
        }
    }

    #[test]
    fn camel_case_optional_fields_are_not_silently_discarded() {
        let mut value = discover_result();
        value["albumArtist"] = json!("Artist");
        let result: Results = serde_json::from_value(value).unwrap();
        assert_eq!(result.album_artist.as_deref(), Some("Artist"));
        assert_eq!(result.track_count, Some(1));
        assert_eq!(
            result.band_location.as_deref(),
            Some("San Francisco, California")
        );
    }

    #[test]
    fn missing_required_band_id_still_returns_an_error() {
        let mut value = discover_result();
        value["featuredTrack"]
            .as_object_mut()
            .unwrap()
            .remove("bandId");
        let error = serde_json::from_value::<Results>(value).unwrap_err();
        assert!(error.to_string().contains("missing field `bandId`"));
    }

    /// Opt-in contract check against current Bandcamp data; no playback or AI calls.
    #[tokio::test]
    #[ignore = "requires live Bandcamp access; see README for proxy and invocation"]
    async fn live_bandcamp_discover_json_contract() -> anyhow::Result<()> {
        use crate::libbc::http_client::USER_AGENT;
        use crate::models::bc_discover_index::PostData;
        use anyhow::{ensure, Context};

        const URL: &str = "https://bandcamp.com/api/discover/1/discover_web";
        // Do not initialize CLI args, audio devices, or a global logger in this test.
        // reqwest honors HTTP(S)_PROXY / ALL_PROXY without hard-coding a local proxy.
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .gzip(true)
            .build()?;

        for genre in [None, Some("jazz"), Some("electronic")] {
            let label = genre.unwrap_or("all genres");
            let request = PostData {
                size: 10,
                tag_norm_names: genre.into_iter().map(str::to_owned).collect(),
                ..Default::default()
            };
            let body = client
                .post(URL)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_vec(&request)?)
                .send()
                .await
                .with_context(|| format!("Bandcamp Discover request failed ({label}; {URL})"))?
                .error_for_status()
                .with_context(|| format!("Bandcamp Discover HTTP error ({label}; {URL})"))?
                .bytes()
                .await?;
            // Keep the raw response out of errors: stream URLs contain signed tokens.
            let response: DiscoverJsonRequest =
                serde_json::from_slice(&body).with_context(|| {
                    format!(
                        "Bandcamp Discover JSON contract changed ({label}; {URL}; {} bytes)",
                        body.len()
                    )
                })?;
            ensure!(
                !response.results.is_empty(),
                "Bandcamp returned no results ({label}); contract was not exercised"
            );

            // Option fields can silently become None after a key rename. Compare
            // their decoded values with the actual API keys, not just parse success.
            let raw: serde_json::Value = serde_json::from_slice(&body)?;
            let raw_results = raw["results"]
                .as_array()
                .context("results must be an array")?;
            let mut playable = 0;
            let mut images = 0;
            for (index, (item, raw_item)) in response.results.iter().zip(raw_results).enumerate() {
                let location = format!("{label}: results[{index}]");
                ensure!(!item.title.is_empty(), "{location}: empty title");
                ensure!(!item.item_url.is_empty(), "{location}: empty itemUrl");
                ensure!(item.band_id > 0, "{location}: invalid bandId");
                let image = raw_item
                    .get("primaryImage")
                    .context("missing primaryImage")?;
                ensure!(
                    image.get("imageId").is_some(),
                    "{location}: missing primaryImage.imageId"
                );
                ensure!(
                    item.primary_image.image_id == image["imageId"].as_i64(),
                    "{location}: imageId was not preserved"
                );
                images += usize::from(item.primary_image.image_id.is_some());

                for (key, decoded) in [
                    ("featuredTrack", &item.featured_track),
                    ("playerData", &item.player_data),
                ] {
                    let raw_track = raw_item.get(key).filter(|value| !value.is_null());
                    ensure!(
                        decoded.is_some() == raw_track.is_some(),
                        "{location}: {key} was not preserved"
                    );
                    if let (Some(track), Some(raw_track)) = (decoded, raw_track) {
                        ensure!(
                            Some(track.stream_url.as_str()) == raw_track["streamUrl"].as_str(),
                            "{location}: {key}.streamUrl was not preserved"
                        );
                        let artist = raw_track
                            .get("artistName")
                            .or_else(|| raw_track.get("bandName"));
                        ensure!(
                            Some(track.band_name.as_str())
                                == artist.and_then(serde_json::Value::as_str),
                            "{location}: {key} artist was not preserved"
                        );
                    }
                }
                if let Some(track) = item.playback_track() {
                    ensure!(
                        track.band_id > 0,
                        "{location}: invalid featuredTrack.bandId"
                    );
                    ensure!(
                        !track.title.is_empty(),
                        "{location}: empty featuredTrack.title"
                    );
                    ensure!(
                        !track.stream_url.is_empty(),
                        "{location}: empty featuredTrack.streamUrl"
                    );
                    playable += 1;
                }
            }
            ensure!(
                playable > 0,
                "{label}: no featured tracks; nested track contract was not exercised"
            );
            ensure!(
                images > 0,
                "{label}: no image IDs; image contract was not exercised"
            );
            let tracks = crate::libbc::playlist::PlayList::gen_track_list(
                &crate::libbc::shared_data::SharedState::default(),
                &response.results,
            )?;
            ensure!(
                tracks.len() == playable,
                "{label}: playable tracks were lost during playlist conversion"
            );
            println!("Bandcamp Discover ({label}): {} results, {playable} playable tracks, {images} images decoded and converted", response.results.len());
        }
        Ok(())
    }
}
