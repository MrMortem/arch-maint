use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAvailability {
    pub pacman: bool,
    pub checkupdates: bool,
    pub pacdiff: bool,
    pub paru: bool,
    pub yay: bool,
    pub snapper: bool,
    pub timeshift: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemProfile {
    pub is_arch: bool,
    pub distro_name: String,
    pub running_as_root: bool,
    pub tools: ToolAvailability,
    pub selected_aur_helper: Option<String>,
}
