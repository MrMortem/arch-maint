use crate::domain::{SystemProfile, ToolAvailability};
use std::{env, path::Path};

pub async fn probe_system() -> SystemProfile {
    let os_release = tokio::fs::read_to_string("/etc/os-release")
        .await
        .unwrap_or_default();
    let id = value_from_os_release(&os_release, "ID").unwrap_or_default();
    let id_like = value_from_os_release(&os_release, "ID_LIKE").unwrap_or_default();
    let distro_name = value_from_os_release(&os_release, "PRETTY_NAME")
        .or_else(|| value_from_os_release(&os_release, "NAME"))
        .unwrap_or_else(|| "Unknown Linux".into());
    let tools = ToolAvailability {
        pacman: command_available("pacman"),
        checkupdates: command_available("checkupdates"),
        pacdiff: command_available("pacdiff"),
        paru: command_available("paru"),
        yay: command_available("yay"),
        snapper: command_available("snapper"),
        timeshift: command_available("timeshift"),
    };
    let selected_aur_helper = if tools.paru {
        Some("paru".into())
    } else if tools.yay {
        Some("yay".into())
    } else {
        None
    };
    SystemProfile {
        is_arch: id == "arch" || id_like.split_whitespace().any(|value| value == "arch"),
        distro_name,
        running_as_root: running_as_root().await,
        tools,
        selected_aur_helper,
    }
}

fn value_from_os_release(input: &str, key: &str) -> Option<String> {
    input.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        (candidate == key).then(|| value.trim_matches(['"', '\'']).to_owned())
    })
}

pub fn command_available(command: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| {
            let candidate = directory.join(command);
            candidate.is_file() && is_executable(&candidate)
        })
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

async fn running_as_root() -> bool {
    tokio::process::Command::new("id")
        .arg("-u")
        .output()
        .await
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_os_release_values() {
        let input = "NAME=Arch Linux\nPRETTY_NAME=\"Arch Linux\"\nID=arch\n";
        assert_eq!(
            value_from_os_release(input, "PRETTY_NAME").as_deref(),
            Some("Arch Linux")
        );
        assert_eq!(value_from_os_release(input, "ID").as_deref(), Some("arch"));
    }
}
