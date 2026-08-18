use super::{PackageSource, PackageUpdate, Snapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    SystemUpgrade,
    Install,
    Remove,
    Upgrade,
    Mixed,
}

impl TransactionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemUpgrade => "System upgrade",
            Self::Install => "Package install",
            Self::Remove => "Package removal",
            Self::Upgrade => "Package upgrade",
            Self::Mixed => "Mixed transaction",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionResult {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub cancelled: bool,
    pub hooks: Vec<HookExecution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionStage {
    PreTransaction,
    PostTransaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookExecution {
    pub description: String,
    pub stage: HookExecutionStage,
    pub status: HookExecutionStatus,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionRequest {
    SystemUpgrade,
    OfficialUpdate {
        package: String,
    },
    OfficialInstall {
        packages: Vec<String>,
    },
    OfficialRemove {
        packages: Vec<String>,
        remove_unused: bool,
    },
    AurInstall {
        packages: Vec<String>,
    },
    AurRemove {
        packages: Vec<String>,
    },
    AurUpgrade,
}

impl TransactionRequest {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SystemUpgrade => "Full system upgrade",
            Self::OfficialUpdate { .. } => "Update selected package with full system sync",
            Self::OfficialInstall { .. } => "Install official packages with full upgrade",
            Self::OfficialRemove { .. } => "Remove official packages",
            Self::AurInstall { .. } => "Install AUR packages",
            Self::AurRemove { .. } => "Remove AUR packages",
            Self::AurUpgrade => "Upgrade AUR packages",
        }
    }

    pub fn requires_privilege(&self) -> bool {
        true
    }

    pub fn supports_pre_snapshot(&self) -> bool {
        matches!(
            self,
            Self::SystemUpgrade
                | Self::OfficialUpdate { .. }
                | Self::OfficialInstall { .. }
                | Self::AurInstall { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub enum TransactionEvent {
    SnapshotStarted { backend: String },
    SnapshotCreated(Snapshot),
    Started { command: Vec<String> },
    Output { stream: OutputStream, chunk: String },
    Finished(Box<TransactionResult>),
    FailedToStart(String),
}

#[derive(Debug, Clone)]
pub enum TransactionControl {
    Input(String),
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub summary: String,
    pub relevant_errors: Vec<String>,
    pub completed_packages: Vec<String>,
    pub package_database_lock_present: bool,
    pub suggested_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalCandidate {
    pub name: String,
    pub version: String,
    pub installed_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalPlan {
    pub requested: Vec<String>,
    pub direct_removals: Vec<RemovalCandidate>,
    pub dependencies_becoming_unused: Vec<RemovalCandidate>,
    pub affected_packages: Vec<String>,
    pub space_reclaimed: Option<u64>,
    pub blocked: bool,
    pub evidence_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSet {
    pub official: Vec<PackageUpdate>,
    pub aur: Vec<PackageUpdate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedAction {
    Upgrade,
    Install,
    Replace,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedPackage {
    pub name: String,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    pub source: PackageSource,
    pub action: PlannedAction,
    pub download_size: Option<u64>,
    pub installed_size_delta: Option<i64>,
    pub ignored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    KernelUpdate,
    BootloaderPackage,
    GraphicsDriver,
    SystemdUpdate,
    GlibcUpdate,
    PacmanUpdate,
    PythonAbiChange,
    DkmsInvolved,
    PackageReplacement,
    PackageRemoval,
    ModifiedConfiguration,
    AurRuntimeDependency,
    IgnoredPackage,
}

impl AttentionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::KernelUpdate => "Kernel update",
            Self::BootloaderPackage => "Bootloader-related package",
            Self::GraphicsDriver => "Graphics driver or stack",
            Self::SystemdUpdate => "systemd update",
            Self::GlibcUpdate => "glibc update",
            Self::PacmanUpdate => "pacman update",
            Self::PythonAbiChange => "Python interpreter change",
            Self::DkmsInvolved => "DKMS modules involved",
            Self::PackageReplacement => "Package replacement",
            Self::PackageRemoval => "Package removal",
            Self::ModifiedConfiguration => "Locally modified configuration",
            Self::AurRuntimeDependency => "AUR runtime dependency affected",
            Self::IgnoredPackage => "Ignored package",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionFinding {
    pub kind: AttentionKind,
    pub packages: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookStage {
    PreTransaction,
    PostTransaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedHook {
    pub name: String,
    pub description: String,
    pub stage: HookStage,
    pub command: Option<String>,
    pub matched_packages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDefinition {
    pub name: String,
    pub description: String,
    pub stage: HookStage,
    pub command: Option<String>,
    pub operations: Vec<String>,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacmanPolicy {
    pub ignore_packages: Vec<String>,
    pub ignore_groups: Vec<String>,
    pub hold_packages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightPlan {
    pub generated_at: DateTime<Utc>,
    pub packages: Vec<PlannedPackage>,
    pub download_size: Option<u64>,
    pub installed_size_delta: Option<i64>,
    pub attention: Vec<AttentionFinding>,
    pub expected_hooks: Vec<ExpectedHook>,
    pub aur_rebuild_candidates: Vec<String>,
    pub separate_aur_updates: Vec<PackageUpdate>,
    pub policy: PacmanPolicy,
    pub evidence_notes: Vec<String>,
}

impl FlightPlan {
    pub fn count(&self, action: PlannedAction) -> usize {
        self.packages
            .iter()
            .filter(|package| package.action == action)
            .count()
    }

    pub fn ignored_count(&self) -> usize {
        self.packages
            .iter()
            .filter(|package| package.ignored)
            .count()
    }
}

impl UpdateSet {
    pub fn total(&self) -> usize {
        self.official.len() + self.aur.len()
    }

    pub fn demo() -> Self {
        Self {
            official: vec![
                PackageUpdate {
                    name: "linux".into(),
                    current_version: "6.12.8.arch1-1".into(),
                    new_version: "6.12.9.arch1-1".into(),
                    source: PackageSource::Official("core".into()),
                    ignored: false,
                },
                PackageUpdate {
                    name: "mesa".into(),
                    current_version: "24.3.3-1".into(),
                    new_version: "24.3.4-1".into(),
                    source: PackageSource::Official("extra".into()),
                    ignored: false,
                },
            ],
            aur: vec![PackageUpdate {
                name: "visual-studio-code-bin".into(),
                current_version: "1.96.2-1".into(),
                new_version: "1.96.3-1".into(),
                source: PackageSource::Aur,
                ignored: false,
            }],
        }
    }
}
