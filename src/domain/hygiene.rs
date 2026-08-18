use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub path: String,
    pub package: Option<String>,
    pub version: Option<String>,
    pub size: u64,
    pub current_installed_version: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HygieneReport {
    pub explicit_packages: Vec<String>,
    pub dependency_packages: Vec<String>,
    pub orphaned_packages: Vec<String>,
    pub foreign_packages: Vec<String>,
    pub cache_entries: Vec<CacheEntry>,
    pub cache_size: u64,
    pub old_cached_versions_size: u64,
    pub evidence_notes: Vec<String>,
}
