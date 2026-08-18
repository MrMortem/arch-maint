use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSource {
    Official(String),
    Aur,
    Foreign,
    Local,
}

impl PackageSource {
    pub fn label(&self) -> &str {
        match self {
            Self::Official(repo) => repo,
            Self::Aur => "AUR",
            Self::Foreign => "foreign",
            Self::Local => "local",
        }
    }
}

impl fmt::Display for PackageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallReason {
    Explicit,
    Dependency,
    #[default]
    Unknown,
}

impl fmt::Display for InstallReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Explicit => "explicit",
            Self::Dependency => "dependency",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AurMetadata {
    pub package_base: Option<String>,
    pub maintainer: Option<String>,
    pub votes: u64,
    pub popularity: f64,
    pub first_submitted: Option<DateTime<Utc>>,
    pub last_modified: Option<DateTime<Utc>>,
    pub out_of_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
    pub description: Option<String>,
    pub architecture: Option<String>,
    pub installed: bool,
    pub install_reason: InstallReason,
    pub installed_size: Option<u64>,
    pub download_size: Option<u64>,
    pub dependencies: Vec<String>,
    pub optional_dependencies: Vec<String>,
    pub reverse_dependencies: Vec<String>,
    pub conflicts: Vec<String>,
    pub provides: Vec<String>,
    pub replaces: Vec<String>,
    pub url: Option<String>,
    pub packager: Option<String>,
    pub install_date: Option<String>,
    pub licenses: Vec<String>,
    pub groups: Vec<String>,
    pub aur: Option<AurMetadata>,
}

impl Package {
    pub fn summary(
        name: impl Into<String>,
        version: impl Into<String>,
        source: PackageSource,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            source,
            description: None,
            architecture: None,
            installed: false,
            install_reason: InstallReason::Unknown,
            installed_size: None,
            download_size: None,
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            reverse_dependencies: Vec::new(),
            conflicts: Vec::new(),
            provides: Vec::new(),
            replaces: Vec::new(),
            url: None,
            packager: None,
            install_date: None,
            licenses: Vec::new(),
            groups: Vec::new(),
            aur: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageUpdate {
    pub name: String,
    pub current_version: String,
    pub new_version: String,
    pub source: PackageSource,
    pub ignored: bool,
}
