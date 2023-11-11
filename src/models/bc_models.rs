
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct BandCampJsonMessage {
    pub items: Vec<Item>,
    pub args: Args,
    pub timestamp: String,
    pub ui_sig: String,
    pub data_sig: String,
    pub total_count: i64,
    pub spec_id: i64
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Args {
    pub g: Option<String>,
    pub s: Option<String>,
    pub f: Option<String>,
    pub r: Option<String>,
    pub w: i64,
    pub p: i64,
    pub following: bool
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Item {
    pub r#type: String,
    pub id: i64,
    pub category: String,
    pub extras: Option<String>,
    pub score: f32,
    pub band_id: i64,
    pub item_type_id: String,
    pub is_preorder: Option<i32>,
    pub publish_date: String,
    pub genre_text: String,
    pub primary_text: String,
    pub secondary_text: String,
    pub art_id: i64,
    pub alt_art_image_id: Option<i64>,
    pub url_hints: UrlHints,
    pub featured_track: FeaturedTrack,
    pub location_text: Option<String>,
    pub package_title: Option<String>,
    pub bio_image: BioImage,
    pub package_art1: Option<String>,
    pub package_art2: Option<String>,
    pub package_art3: Option<String>,
    pub package_art4: Option<String>,
    pub recommendations: Option<String>,
    pub license_id: Option<i64>,
    pub territories: Option<String>,
    pub lo_querystr: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UrlHints {
    pub subdomain: String,
    pub custom_domain: Option<String>,
    pub custom_domain_verified: Option<i32>,
    pub slug: String,
    pub item_type: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BioImage {
    pub image_id: Option<i64>,
    pub height: Option<i32>,
    pub width: Option<i32>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeaturedTrack {
    pub file: File,
    pub duration: f32,
    pub id: i64,
    pub title: String,
    pub encodings_id: i64
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    #[serde(rename = "mp3-128")]
    pub mp3_128: String
}
