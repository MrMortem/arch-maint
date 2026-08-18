use super::InstallReason;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyNode {
    pub name: String,
    pub cycle: bool,
    pub depth_limited: bool,
    pub children: Vec<DependencyNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyReport {
    pub package: String,
    pub install_reason: InstallReason,
    pub why_paths: Vec<Vec<String>>,
    pub dependencies: DependencyNode,
    pub reverse_dependencies: DependencyNode,
    pub orphan_candidates_after_removal: Vec<String>,
    pub max_depth: usize,
}
