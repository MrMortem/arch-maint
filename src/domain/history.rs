use super::TransactionKind;
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageAction {
    Installed,
    Upgraded,
    Downgraded,
    Reinstalled,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageChange {
    pub action: PackageAction,
    pub name: String,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryTransaction {
    pub started_at: DateTime<FixedOffset>,
    pub completed: bool,
    pub kind: TransactionKind,
    pub command_line: Option<String>,
    pub changes: Vec<PackageChange>,
}

impl HistoryTransaction {
    pub fn count(&self, action: PackageAction) -> usize {
        self.changes
            .iter()
            .filter(|change| change.action == action)
            .count()
    }
}
