use crate::domain::{InstallReason, Package, PackageSource, PackageUpdate, RemovalCandidate};
use anyhow::{Result, bail};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionCandidate {
    pub name: String,
    pub version: String,
    pub repository: String,
    pub download_size: Option<u64>,
    pub installed_size: Option<u64>,
}

pub fn validate_search_query(query: &str) -> Result<&str> {
    let query = query.trim();
    if query.is_empty() {
        bail!("search query cannot be empty");
    }
    if query.len() > 200 {
        bail!("search query is too long");
    }
    if query.starts_with('-') || query.contains('\0') || query.chars().any(char::is_control) {
        bail!("search query contains unsupported characters");
    }
    Ok(query)
}

pub fn parse_updates(output: &str, source: PackageSource) -> Vec<PackageUpdate> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let old = fields.next()?;
            let separator_or_new = fields.next()?;
            let new = if separator_or_new == "->" {
                fields.next()?
            } else {
                separator_or_new
            };
            Some(PackageUpdate {
                name: name.to_owned(),
                current_version: old.to_owned(),
                new_version: new.to_owned(),
                source: source.clone(),
                ignored: false,
            })
        })
        .collect()
}

pub fn parse_transaction_print(output: &str) -> Vec<TransactionCandidate> {
    output
        .lines()
        .filter_map(|line| {
            let fields = line.split('|').collect::<Vec<_>>();
            if fields.len() != 4 {
                return None;
            }
            Some(TransactionCandidate {
                name: fields[0].to_owned(),
                version: fields[1].to_owned(),
                repository: fields[2].to_owned(),
                download_size: fields[3].parse().ok(),
                installed_size: None,
            })
        })
        .collect()
}

pub fn parse_removal_print(output: &str) -> Vec<RemovalCandidate> {
    output
        .lines()
        .filter_map(|line| {
            let fields = line.split('|').collect::<Vec<_>>();
            if fields.len() != 3 {
                return None;
            }
            Some(RemovalCandidate {
                name: fields[0].to_owned(),
                version: fields[1].to_owned(),
                installed_size: fields[2].parse().ok(),
            })
        })
        .collect()
}

pub fn parse_search(output: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut lines = output.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with(char::is_whitespace) || line.trim().is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let repo_name = match fields.next() {
            Some(value) => value,
            None => continue,
        };
        let version = match fields.next() {
            Some(value) => value,
            None => continue,
        };
        let Some((repo, name)) = repo_name.split_once('/') else {
            continue;
        };
        let installed = line.contains("[installed") || line.contains("[Installed");
        let has_description = lines
            .peek()
            .is_some_and(|next| next.starts_with(char::is_whitespace));
        let description =
            has_description.then(|| lines.next().unwrap_or_default().trim().to_owned());
        let mut package = Package::summary(name, version, PackageSource::Official(repo.to_owned()));
        package.installed = installed;
        package.description = description;
        packages.push(package);
    }
    packages
}

pub fn parse_info_records(output: &str, default_source: PackageSource) -> Vec<Package> {
    split_records(output)
        .into_iter()
        .filter_map(|record| package_from_record(&record, &default_source))
        .collect()
}

fn split_records(output: &str) -> Vec<HashMap<String, String>> {
    let mut records = Vec::new();
    let mut current = HashMap::new();
    let mut active_key: Option<String> = None;
    for line in output.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if !current.is_empty() {
                records.push(std::mem::take(&mut current));
            }
            active_key = None;
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(key) = &active_key {
                let value = current.entry(key.clone()).or_insert_with(String::new);
                value.push('\n');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_owned();
            current.insert(key.clone(), value.trim().to_owned());
            active_key = Some(key);
        }
    }
    records
}

fn package_from_record(
    record: &HashMap<String, String>,
    default_source: &PackageSource,
) -> Option<Package> {
    let name = record.get("Name")?.to_owned();
    let version = record.get("Version")?.to_owned();
    let source = record
        .get("Repository")
        .filter(|value| !value.is_empty() && value.as_str() != "None")
        .map(|repo| PackageSource::Official(repo.clone()))
        .unwrap_or_else(|| default_source.clone());
    let mut package = Package::summary(name, version, source);
    package.description = optional(record, "Description");
    package.architecture = optional(record, "Architecture");
    package.installed =
        record.contains_key("Install Date") || record.contains_key("Install Reason");
    package.install_reason = match record.get("Install Reason").map(String::as_str) {
        Some(value) if value.to_ascii_lowercase().contains("explicit") => InstallReason::Explicit,
        Some(value) if value.to_ascii_lowercase().contains("dependency") => {
            InstallReason::Dependency
        }
        _ => InstallReason::Unknown,
    };
    package.installed_size = record
        .get("Installed Size")
        .and_then(|value| parse_size(value));
    package.download_size = record
        .get("Download Size")
        .and_then(|value| parse_size(value));
    package.dependencies = list(record, "Depends On");
    package.optional_dependencies = line_list(record, "Optional Deps");
    package.reverse_dependencies = list(record, "Required By");
    package.conflicts = list(record, "Conflicts With");
    package.provides = list(record, "Provides");
    package.replaces = list(record, "Replaces");
    package.licenses = list(record, "Licenses");
    package.groups = list(record, "Groups");
    package.url = optional(record, "URL");
    package.packager = optional(record, "Packager");
    package.install_date = optional(record, "Install Date");
    Some(package)
}

fn optional(record: &HashMap<String, String>, key: &str) -> Option<String> {
    record
        .get(key)
        .filter(|value| !value.is_empty() && value.as_str() != "None")
        .cloned()
}

fn list(record: &HashMap<String, String>, key: &str) -> Vec<String> {
    optional(record, key)
        .map(|value| value.split_whitespace().map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

fn line_list(record: &HashMap<String, String>, key: &str) -> Vec<String> {
    optional(record, key)
        .map(|value| {
            value
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number: f64 = parts.next()?.parse().ok()?;
    let multiplier = match parts.next().unwrap_or("B") {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = r#"Name            : ripgrep
Version         : 14.1.1-1
Description     : A search tool
Architecture    : x86_64
URL             : https://github.com/BurntSushi/ripgrep
Licenses        : MIT  Unlicense
Depends On      : gcc-libs  pcre2
Optional Deps   : ripgrep-all: extra file types
                  pcre2-jit: alternate matching engine
Required By     : dev-tools
Installed Size  : 6.12 MiB
Packager        : Arch Builder
Install Date    : Sun 12 Jan 2025 10:00:00 AM EST
Install Reason  : Explicitly installed

"#;

    #[test]
    fn parses_installed_package_info() {
        let packages = parse_info_records(INFO, PackageSource::Local);
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        assert_eq!(package.name, "ripgrep");
        assert_eq!(package.install_reason, InstallReason::Explicit);
        assert_eq!(package.dependencies, ["gcc-libs", "pcre2"]);
        assert_eq!(
            package.optional_dependencies,
            [
                "ripgrep-all: extra file types",
                "pcre2-jit: alternate matching engine"
            ]
        );
        assert!(package.installed_size.unwrap_or_default() > 6_000_000);
    }

    #[test]
    fn parses_repo_search_pairs() {
        let output = "extra/ripgrep 14.1.1-1 [installed]\n    A search tool\nextra/ripgrep-all 0.10.6-2\n    Wrapper around ripgrep\n";
        let packages = parse_search(output);
        assert_eq!(packages.len(), 2);
        assert!(packages[0].installed);
        assert_eq!(
            packages[1].description.as_deref(),
            Some("Wrapper around ripgrep")
        );
    }

    #[test]
    fn rejects_option_like_query() {
        assert!(validate_search_query("--refresh").is_err());
        assert!(validate_search_query("ripgrep").is_ok());
    }

    #[test]
    fn parses_transaction_print_format() {
        let output = "linux|6.13.1-1|core|145000000\nnew-dependency|1.0-1|extra|2048\n";
        let packages = parse_transaction_print(output);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[1].download_size, Some(2048));
    }

    #[test]
    fn parses_checkupdates_arrow_format() {
        let updates = parse_updates(
            "linux 6.12.8.arch1-1 -> 6.12.9.arch1-1\nmesa 24.3.3-1 24.3.4-1\n",
            PackageSource::Official("core".into()),
        );
        assert_eq!(updates[0].new_version, "6.12.9.arch1-1");
        assert_eq!(updates[1].new_version, "24.3.4-1");
    }

    #[test]
    fn parses_removal_simulation_format() {
        let packages = parse_removal_print("obs-studio|31.0-1|300000000\nlibfoo|1.2-1|4096\n");
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[1].name, "libfoo");
        assert_eq!(packages[0].installed_size, Some(300_000_000));
    }
}
