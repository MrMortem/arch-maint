use super::HygieneBackend;
use crate::domain::{CacheEntry, HygieneReport, InstallReason, Package, PackageSource};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PackageHygieneBackend {
    cache_directories: Vec<PathBuf>,
}

impl Default for PackageHygieneBackend {
    fn default() -> Self {
        Self {
            cache_directories: vec![PathBuf::from("/var/cache/pacman/pkg")],
        }
    }
}

#[async_trait]
impl HygieneBackend for PackageHygieneBackend {
    async fn inspect(&self, installed: &[Package]) -> Result<HygieneReport> {
        let mut evidence_notes = Vec::new();
        let cache_entries =
            scan_cache(&self.cache_directories, installed, &mut evidence_notes).await;
        let cache_size = cache_entries.iter().map(|entry| entry.size).sum();
        let old_cached_versions_size = cache_entries
            .iter()
            .filter(|entry| entry.package.is_some() && !entry.current_installed_version)
            .map(|entry| entry.size)
            .sum();
        let mut explicit_packages = Vec::new();
        let mut dependency_packages = Vec::new();
        let mut orphaned_packages = Vec::new();
        let mut foreign_packages = Vec::new();
        for package in installed {
            match package.install_reason {
                InstallReason::Explicit => explicit_packages.push(package.name.clone()),
                InstallReason::Dependency => {
                    dependency_packages.push(package.name.clone());
                    if package.reverse_dependencies.is_empty() {
                        orphaned_packages.push(package.name.clone());
                    }
                }
                InstallReason::Unknown => {}
            }
            if matches!(package.source, PackageSource::Aur | PackageSource::Foreign) {
                foreign_packages.push(package.name.clone());
            }
        }
        for packages in [
            &mut explicit_packages,
            &mut dependency_packages,
            &mut orphaned_packages,
            &mut foreign_packages,
        ] {
            packages.sort();
            packages.dedup();
        }
        evidence_notes.push(
            "Old cache size includes only entries matched to installed package names; unclassified cache files are not called old."
                .into(),
        );
        Ok(HygieneReport {
            explicit_packages,
            dependency_packages,
            orphaned_packages,
            foreign_packages,
            cache_entries,
            cache_size,
            old_cached_versions_size,
            evidence_notes,
        })
    }
}

async fn scan_cache(
    directories: &[PathBuf],
    installed: &[Package],
    notes: &mut Vec<String>,
) -> Vec<CacheEntry> {
    let mut entries = Vec::new();
    let mut packages = installed.iter().collect::<Vec<_>>();
    packages.sort_by_key(|package| std::cmp::Reverse(package.name.len()));
    for directory in directories {
        let mut reader = match tokio::fs::read_dir(directory).await {
            Ok(reader) => reader,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                notes.push(format!("Could not read {}: {error}", directory.display()));
                continue;
            }
        };
        loop {
            let entry = match reader.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(error) => {
                    notes.push(format!("Cache enumeration failed: {error}"));
                    break;
                }
            };
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if filename.ends_with(".sig") || !filename.contains(".pkg.tar.") {
                continue;
            }
            let metadata = match entry.metadata().await {
                Ok(metadata) if metadata.is_file() => metadata,
                _ => continue,
            };
            let matched = packages.iter().find_map(|package| {
                filename
                    .strip_prefix(&format!("{}-", package.name))
                    .map(|remainder| (*package, remainder))
            });
            let (package, version, current) =
                matched.map_or((None, None, false), |(package, rest)| {
                    let version = cached_version(rest);
                    let current = version.as_deref() == Some(package.version.as_str());
                    (Some(package.name.clone()), version, current)
                });
            entries.push(CacheEntry {
                path: path.display().to_string(),
                package,
                version,
                size: metadata.len(),
                current_installed_version: current,
            });
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn cached_version(remainder: &str) -> Option<String> {
    let (package_part, extension) = remainder.split_once(".pkg.tar.")?;
    if extension.is_empty() {
        return None;
    }
    let (version, _architecture) = package_part.rsplit_once('-')?;
    Some(version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_version_without_guessing_package_name() {
        assert_eq!(
            cached_version("6.13.1.arch1-1-x86_64.pkg.tar.zst").as_deref(),
            Some("6.13.1.arch1-1")
        );
        assert!(cached_version("not-a-package").is_none());
    }
}
