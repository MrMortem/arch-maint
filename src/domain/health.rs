use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Healthy,
    Advisory,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCategory {
    PackageDatabase,
    SystemServices,
    UserServices,
    Configuration,
    Dkms,
    Packages,
    Kernel,
    ServiceRestarts,
}

impl HealthCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::PackageDatabase => "Package database",
            Self::SystemServices => "System services",
            Self::UserServices => "User services",
            Self::Configuration => "Configuration",
            Self::Dkms => "DKMS",
            Self::Packages => "Packages",
            Self::Kernel => "Kernel",
            Self::ServiceRestarts => "Service restarts",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthFinding {
    pub category: HealthCategory,
    pub severity: FindingSeverity,
    pub title: String,
    pub detail: String,
    pub suggested_check: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigArtifactKind {
    Pacnew,
    Pacsave,
    Pacorig,
}

impl ConfigArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pacnew => "PACNEW",
            Self::Pacsave => "PACSAVE",
            Self::Pacorig => "PACORIG",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigArtifact {
    pub kind: ConfigArtifactKind,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigReview {
    pub artifact: ConfigArtifact,
    pub current_path: String,
    pub current_content: Option<String>,
    pub artifact_content: String,
    pub unified_diff: Option<String>,
    pub evidence_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthReport {
    pub checked_at: DateTime<Utc>,
    pub findings: Vec<HealthFinding>,
    pub config_artifacts: Vec<ConfigArtifact>,
    pub orphaned_packages: Vec<String>,
    pub foreign_packages: Vec<String>,
    pub evidence_notes: Vec<String>,
}

impl HealthReport {
    pub fn issue_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| {
                matches!(
                    finding.severity,
                    FindingSeverity::Warning | FindingSeverity::Error
                )
            })
            .count()
    }
}
