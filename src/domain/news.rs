use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchNewsItem {
    pub title: String,
    pub link: String,
    pub published: Option<String>,
    pub summary: Option<String>,
}
