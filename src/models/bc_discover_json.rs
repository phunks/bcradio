use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverJsonRequest {
    pub results: Vec<Results>,
    result_count: i32,
    batch_result_count: i32,
    pub cursor: Option<String>,
    discover_spec_id: i32,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Results {
    pub id: i64,
    pub title: String,
    pub item_url: String,
    pub item_price: f32,
    pub item_currency: String,
    pub item_image_id: Option<i64>,
    pub result_type: String,
    pub band_id: i64,
    pub album_artist: Option<String>,
    pub band_name: String, //labels
    pub band_url: String,
    pub band_bio_image_id: i64,
    pub band_latest_art_id: i64,
    pub band_genre_id: i32,
    pub release_date: String,
    pub total_package_count: Option<i32>,
    pub package_info: Option<Vec<Package>>,
    pub featured_track: FeaturedTrack,
    pub label_name: Option<String>,
    pub label_url: Option<String>,
    pub band_location: Option<String>,
    track_count: i32,
    pub item_duration: f32,
    pub item_tags: Option<String>,
}

impl Clone for Results {
    fn clone(&self) -> Results {
        Results {
            id: self.id,
            title: self.title.clone(),
            item_url: self.item_url.clone(),
            item_price: self.item_price,
            item_currency: self.item_currency.clone(),
            item_image_id: self.item_image_id,
            result_type: self.result_type.clone(),
            band_id: self.band_id,
            album_artist: self.album_artist.clone(),
            band_name: self.band_name.clone(),
            band_url: self.band_url.clone(),
            band_bio_image_id: self.band_bio_image_id,
            band_latest_art_id: self.band_latest_art_id,
            band_genre_id: self.band_genre_id,
            release_date: self.release_date.clone(),
            total_package_count: self.total_package_count,
            package_info: self.package_info.clone(),
            featured_track: self.featured_track.clone(),
            label_name: self.label_name.clone(),
            label_url: self.label_url.clone(),
            band_location: self.band_location.clone(),
            track_count: self.track_count,
            item_duration: self.item_duration,
            item_tags: self.item_tags.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeaturedTrack {
    pub id: i64,
    pub title: String,
    pub band_name: String,
    pub stream_url: String,
}

impl Clone for FeaturedTrack {
    fn clone(&self) -> FeaturedTrack {
        FeaturedTrack {
            id: self.id,
            title: self.title.clone(),
            band_name: self.band_name.clone(),
            stream_url: self.stream_url.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Package {
    pub id: i64,
    pub title: String,
    pub format: String,
    pub image_id: i64,
    pub price: f32,
    pub type_id: i32,
}

impl Clone for Package {
    fn clone(&self) -> Package {
        Package {
            id: self.id,
            title: self.title.clone(),
            format: self.format.clone(),
            image_id: self.image_id,
            price: self.price,
            type_id: self.type_id,
        }
    }
}