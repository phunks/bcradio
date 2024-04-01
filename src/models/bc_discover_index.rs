
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverIndexRequest {
    #[serde(rename="appData")]
    pub app_data: AppData,
    #[serde(rename="pageContext")]
    pub page_context: PageContext,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppData {
    #[serde(rename="initialState")]
    pub initial_state: InitialState,
    #[serde(rename="seoData")]
    seo_data: Option<SeoData>,
    #[serde(rename="includeResultTypes")]
    pub include_result_types: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InitialState {
    pub categories: Vec<Element>,
    pub genres: Vec<Element>,
    pub subgenres: Vec<Element>,
    pub slices: Vec<Element>,
    pub locations: Vec<Element>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Element {
    pub id: i64,
    pub label: String,
    pub slug: String,
    pub selected: Option<bool>,
    #[serde(rename="parentId")]
    pub parent_id: Option<i64>,
}

impl Clone for Element {
    fn clone(&self) -> Self {
        Element {
            id: self.id,
            label: self.label.clone(),
            slug: self.slug.clone(),
            selected: self.selected,
            parent_id: self.parent_id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SeoData {
    title: String,
    description: String,
    canonical_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PageContext {
    #[serde(rename="fanId")]
    fan_id: Option<i64>,
    #[serde(rename="isLoggedIn")]
    is_logged_in: bool,
    #[serde(rename="isAdmin")]
    is_admin: bool,
    #[serde(rename="isMobile")]
    is_mobile: bool,
    languages: Languages,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Languages {
    en: String,
    de: String,
    es: String,
    fr: String,
    pt: String,
    ja: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostData {
    pub category_id: i16,
    pub tag_norm_names: Vec<String>,
    pub geoname_id: i16,
    pub slice: String,
    pub cursor: Option<String>,
    pub size: i16,
    pub include_result_types: Vec<String>,
}

impl Clone for PostData {
    fn clone(&self) -> Self {
        PostData {
            category_id: self.category_id,
            tag_norm_names: self.tag_norm_names.clone(),
            geoname_id: self.geoname_id,
            slice: self.slice.clone(),
            cursor: self.cursor.clone(),
            size: self.size,
            include_result_types: self.include_result_types.clone(),
        }
    }
}

impl Default for PostData {
    fn default() -> Self {
        PostData {
            category_id: 0,
            tag_norm_names: Vec::new(),
            geoname_id: 0,
            slice: "rand".to_string(),
            cursor: Option::from("*".to_string()),
            size: 60,
            include_result_types: vec!["a".to_string()],
        }
    }
}