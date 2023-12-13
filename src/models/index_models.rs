use serde::{Deserialize, Serialize};

use crate::models::bc_models::Item;

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageForIndexPage {
    #[serde(rename = "server_items")]
    pub items: Vec<Item>,
    pub total_count: i64,
    pub colors: Colors,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Colors {
    pub g: Option<String>,
    pub t: Option<String>,
    pub r: Option<String>,
    pub s: Option<String>,
    pub l: Option<String>,
    pub f: Option<String>,
    pub w: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexG {
    pub id: i32,
    pub name: String,
    pub norm_name: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexT {
    pub name: String,
    pub norm_name: String,
    pub value: String,
}
