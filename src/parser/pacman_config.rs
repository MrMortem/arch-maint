use crate::domain::PacmanPolicy;

pub fn parse_pacman_policy(input: &str) -> PacmanPolicy {
    let mut policy = PacmanPolicy::default();
    for raw_line in input.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let values = value.split_whitespace().map(ToOwned::to_owned);
        match key.trim() {
            "IgnorePkg" => policy.ignore_packages.extend(values),
            "IgnoreGroup" => policy.ignore_groups.extend(values),
            "HoldPkg" => policy.hold_packages.extend(values),
            _ => {}
        }
    }
    policy.ignore_packages.sort();
    policy.ignore_packages.dedup();
    policy.ignore_groups.sort();
    policy.ignore_groups.dedup();
    policy.hold_packages.sort();
    policy.hold_packages.dedup();
    policy
}

pub fn parse_modified_backups(input: &str) -> Vec<String> {
    let mut modified = Vec::new();
    let mut in_backups = false;
    for line in input.lines() {
        if let Some((key, value)) = line.split_once(':') {
            in_backups = key.trim() == "Backup Files";
            if in_backups {
                collect_modified(value, &mut modified);
            }
        } else if in_backups && line.starts_with(char::is_whitespace) {
            collect_modified(line, &mut modified);
        } else if !line.starts_with(char::is_whitespace) {
            in_backups = false;
        }
    }
    modified.sort();
    modified.dedup();
    modified
}

pub fn parse_modified_backup_records(input: &str) -> Vec<(String, String)> {
    input
        .split("\n\n")
        .flat_map(|record| {
            let package = record.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == "Name").then(|| value.trim().to_owned())
            });
            parse_modified_backups(record)
                .into_iter()
                .filter_map(move |path| package.clone().map(|package| (package, path)))
        })
        .collect()
}

fn collect_modified(line: &str, output: &mut Vec<String>) {
    if !line.split_whitespace().any(|field| field == "MODIFIED") {
        return;
    }
    if let Some(path) = line.split_whitespace().find(|field| field.starts_with('/')) {
        output.push(path.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repeated_policy_directives_and_comments() {
        let input = "[options]\nIgnorePkg = linux linux-lts # temporary\nIgnorePkg = nvidia\nIgnoreGroup = plasma\nHoldPkg = pacman glibc\n";
        let policy = parse_pacman_policy(input);
        assert_eq!(policy.ignore_packages, ["linux", "linux-lts", "nvidia"]);
        assert_eq!(policy.ignore_groups, ["plasma"]);
        assert_eq!(policy.hold_packages, ["glibc", "pacman"]);
    }

    #[test]
    fn extracts_only_modified_backup_files() {
        let input = "Name : openssh\nBackup Files : /etc/ssh/sshd_config deadbeef MODIFIED\n               /etc/ssh/ssh_config cafe UNMODIFIED\n";
        assert_eq!(parse_modified_backups(input), ["/etc/ssh/sshd_config"]);
    }
}
