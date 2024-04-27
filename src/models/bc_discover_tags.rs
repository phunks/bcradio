use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DiscoverTagsJson {
    pub single_results: Vec<Struct>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelatedTags {
    pub id: i64,
    pub relation: f64,
    pub name: String,
    pub norm_name: String,
    pub isloc: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Tag {
    pub name: String,
    pub norm_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Struct {
    pub tag: Tag,
    pub related_tags: Vec<RelatedTags>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TagsPostData {
    pub tag_names: Vec<String>,
    pub size: i8,
    pub combo: bool,
}

impl Default for TagsPostData {
    fn default() -> Self {
        TagsPostData {
            tag_names: vec![],
            size: 20,
            combo: false,
        }
    }
}

impl Clone for TagsPostData {
    fn clone(&self) -> Self {
        TagsPostData {
            tag_names: self.tag_names.clone(),
            size: self.size,
            combo: self.combo,
        }
    }
}
