use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverJsonRequest {
    pub results: Vec<Results>,
    result_count: i32,
    batch_result_count: i32,
    pub cursor: Option<String>,
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
pub struct Results {
    pub title: String,
    pub item_url: String,
    pub price: Price,
    pub result_type: String,
    pub band_id: i64, //label id
    pub album_artist: Option<String>,
    pub band_name: String, //labels
    pub band_url: String, //label url
    pub band_genre_id: i32,
    pub release_date: String,
    pub package_info: Option<Vec<Package>>,
    pub featured_track: FeaturedTrack,
    pub band_location: Option<String>,
    track_count: Option<i32>,
    pub duration: Option<f32>,
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
            band_location: self.band_location.clone(),
            track_count: self.track_count,
            duration: self.duration,
            primary_image: self.primary_image.clone(),
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct FeaturedTrack {
    pub band_id: i64,
    pub title: String,
    pub band_name: String,
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
    pub image_id: i64,
    price: Price,
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
    is_money: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PrimaryImage {
    pub image_id: Option<i64>,
    is_art: bool,
}