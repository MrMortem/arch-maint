use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PkgbuildChangeKind {
    Dependencies,
    Sources,
    NewSourceDomain,
    Checksums,
    BuildCommands,
    InstallCommands,
    CheckCommands,
    InstallScript,
    ShellCommands,
}

impl PkgbuildChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dependencies => "Dependencies changed",
            Self::Sources => "Source URLs/files changed",
            Self::NewSourceDomain => "New source domain",
            Self::Checksums => "Checksums changed",
            Self::BuildCommands => "Build commands changed",
            Self::InstallCommands => "Package/install commands changed",
            Self::CheckCommands => "Test commands changed",
            Self::InstallScript => ".install script changed",
            Self::ShellCommands => "Shell commands changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkgbuildFinding {
    pub kind: PkgbuildChangeKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkgbuildReview {
    pub package: String,
    pub baseline_source: Option<String>,
    pub current_pkgbuild: String,
    pub unified_diff: String,
    pub findings: Vec<PkgbuildFinding>,
    pub related_files: Vec<ReviewedAurFile>,
    pub evidence_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedAurFile {
    pub path: String,
    pub current_content: String,
    pub unified_diff: Option<String>,
}

impl PkgbuildReview {
    pub fn has_baseline(&self) -> bool {
        self.baseline_source.is_some()
    }
}
