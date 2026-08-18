use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub backend: String,
    pub id: Option<String>,
    pub description: String,
    pub created_at: Option<DateTime<Utc>>,
}
