use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchJsonRequest {
    pub search_text: String,
    pub search_filter: String,
    pub full_page: bool,
    pub fan_id: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchJsonResponse {
    pub auto: Results,
    pub tag: Tag,
    pub genre: Genre,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Tag {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Genre {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Results {
    pub results: Vec<SearchItem>,
    pub stat_params_for_tag: Option<String>,
    pub time_ms: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchItem {
    #[serde(rename = "type")]
    types: String,
    pub id: i64,
    pub art_id: Option<i64>,
    pub img_id: Option<i64>,
    pub name: String,
    pub band_id: i64,
    pub band_name: Option<String>,
    pub album_name: Option<String>,
    pub item_url_root: Option<String>,
    pub item_url_path: Option<String>,
    pub img: Option<String>,
    pub album_id: Option<i64>,
    tag_names: Option<String>,
    stat_params: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemPage {
    pub current: Current,
    pub artist: String,
    pub trackinfo: Vec<TrackInfo>,
    pub album_url: Option<String>,
    #[serde(rename = "url")]
    pub item_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Current {
    pub title: String,
    pub art_id: Option<i64>,
    pub band_id: i64,
    pub release_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    id: i64,
    track_id: i64,
    pub file: Mp3Url,
    pub artist: Option<String>,
    pub title: Option<String>,
    encodings_id: i64,
    license_type: i32,
    private: Option<bool>,
    track_num: Option<i32>,
    album_preorder: Option<bool>,
    unreleased_track: Option<bool>,
    title_link: Option<String>,
    has_lyrics: Option<bool>,
    has_info: Option<bool>,
    streaming: i32,
    is_downloadable: Option<bool>,
    has_free_download: Option<bool>,
    free_album_download: Option<bool>,
    pub duration: f32,
    lyrics: Option<String>,
    sizeof_lyrics: Option<i64>,
    is_draft: Option<bool>,
    video_source_type: Option<String>,
    video_source_id: Option<String>,
    video_mobile_url: Option<String>,
    video_poster_url: Option<String>,
    video_id: Option<i64>,
    video_caption: Option<String>,
    video_featured: Option<i32>,
    alt_link: Option<String>,
    encoding_error: Option<String>,
    encoding_pending: Option<String>,
    play_count: Option<i64>,
    is_capped: Option<bool>,
    track_license_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mp3Url {
    #[serde(rename = "mp3-128")]
    pub(crate) mp3_128: Option<String>,
}
