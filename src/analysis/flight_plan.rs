use crate::{
    domain::{
        AttentionFinding, AttentionKind, ExpectedHook, FlightPlan, Package, PackageSource,
        PackageUpdate, PacmanPolicy, PlannedAction, PlannedPackage,
    },
    parser::{AlpmHook, TransactionCandidate},
};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub struct FlightPlanInput<'a> {
    pub official_updates: &'a [PackageUpdate],
    pub aur_updates: &'a [PackageUpdate],
    pub installed: &'a [Package],
    pub transaction_candidates: &'a [TransactionCandidate],
    pub sync_details: &'a [Package],
    pub policy: PacmanPolicy,
    pub modified_backups: &'a [(String, String)],
    pub hooks: &'a [AlpmHook],
    pub evidence_notes: Vec<String>,
}

pub fn build_flight_plan(input: FlightPlanInput<'_>) -> FlightPlan {
    let installed: HashMap<_, _> = input
        .installed
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect();
    let details: HashMap<_, _> = input
        .sync_details
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect();
    let updates: HashMap<_, _> = input
        .official_updates
        .iter()
        .map(|update| (update.name.as_str(), update))
        .collect();

    let mut candidates = input.transaction_candidates.to_vec();
    if candidates.is_empty() {
        candidates.extend(
            input
                .official_updates
                .iter()
                .map(|update| TransactionCandidate {
                    name: update.name.clone(),
                    version: update.new_version.clone(),
                    repository: update.source.label().to_owned(),
                    download_size: None,
                    installed_size: None,
                }),
        );
    }

    let mut packages = Vec::new();
    for candidate in candidates {
        let local = installed.get(candidate.name.as_str()).copied();
        let detail = details.get(candidate.name.as_str()).copied();
        let update = updates.get(candidate.name.as_str()).copied();
        let replaced = detail.and_then(|package| {
            package.replaces.iter().find_map(|replacement| {
                let base = dependency_name(replacement);
                installed.get(base).copied()
            })
        });
        let action = if replaced.is_some() && local.is_none() {
            PlannedAction::Replace
        } else if local.is_some() || update.is_some() {
            PlannedAction::Upgrade
        } else {
            PlannedAction::Install
        };
        let old_package = local.or(replaced);
        let new_installed_size = candidate
            .installed_size
            .or_else(|| detail.and_then(|package| package.installed_size));
        let installed_size_delta = match (
            old_package.and_then(|p| p.installed_size),
            new_installed_size,
        ) {
            (Some(old), Some(new)) => Some(new as i64 - old as i64),
            (None, Some(new)) if action == PlannedAction::Install => Some(new as i64),
            _ => None,
        };
        let source = if candidate.repository.is_empty() {
            PackageSource::Official("repo".into())
        } else {
            PackageSource::Official(candidate.repository)
        };
        packages.push(PlannedPackage {
            name: candidate.name,
            old_version: old_package.map(|package| package.version.clone()),
            new_version: Some(candidate.version),
            source,
            action,
            download_size: candidate
                .download_size
                .or_else(|| detail.and_then(|package| package.download_size)),
            installed_size_delta,
            ignored: update.is_some_and(|update| update.ignored)
                || policy_matches(
                    &input.policy.ignore_packages,
                    update.map(|u| u.name.as_str()).unwrap_or_default(),
                )
                || detail.is_some_and(|package| {
                    package
                        .groups
                        .iter()
                        .any(|group| policy_matches(&input.policy.ignore_groups, group))
                }),
        });
    }

    packages.sort_by(|a, b| {
        a.source
            .label()
            .cmp(b.source.label())
            .then_with(|| a.name.cmp(&b.name))
    });

    let official_packages = packages
        .iter()
        .filter(|package| !matches!(package.source, PackageSource::Aur));
    let download_size = sum_if_complete(
        official_packages
            .clone()
            .map(|package| package.download_size),
    );
    let installed_size_delta =
        sum_if_complete(official_packages.map(|package| package.installed_size_delta));
    let updated_names: HashSet<_> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    let aur_rebuild_candidates = aur_rebuild_candidates(input.installed, &updated_names);
    let attention = classify_attention(
        &packages,
        input.installed,
        input.modified_backups,
        &aur_rebuild_candidates,
    );
    let expected_hooks = match_hooks(input.hooks, &packages);
    let mut evidence_notes = input.evidence_notes;
    if packages.iter().any(|package| {
        package.download_size.is_none() && !matches!(package.source, PackageSource::Aur)
    }) {
        evidence_notes.push("Download total is unavailable because Pacman did not provide a size for every official package.".into());
    }
    if packages.iter().any(|package| {
        package.installed_size_delta.is_none() && !matches!(package.source, PackageSource::Aur)
    }) {
        evidence_notes.push(
            "Disk delta is unavailable because installed-size metadata is incomplete.".into(),
        );
    }
    evidence_notes.push("Pacman print-format does not expose every conflict-driven removal; removals not evidenced by package metadata remain unknown.".into());
    evidence_notes.sort();
    evidence_notes.dedup();

    FlightPlan {
        generated_at: Utc::now(),
        packages,
        download_size,
        installed_size_delta,
        attention,
        expected_hooks,
        aur_rebuild_candidates,
        separate_aur_updates: input.aur_updates.to_vec(),
        policy: input.policy,
        evidence_notes,
    }
}

fn classify_attention(
    packages: &[PlannedPackage],
    installed: &[Package],
    modified_backups: &[(String, String)],
    aur_rebuilds: &[String],
) -> Vec<AttentionFinding> {
    let mut findings = BTreeMap::<AttentionKind, BTreeSet<String>>::new();
    let mut explanations = HashMap::<AttentionKind, String>::new();
    for package in packages {
        let name = package.name.as_str();
        let finding = if is_kernel(name) {
            Some((
                AttentionKind::KernelUpdate,
                "The installed kernel package changes; out-of-tree modules and the running kernel may need follow-up.",
            ))
        } else if matches!(
            name,
            "grub" | "refind" | "limine" | "syslinux" | "shim-signed"
        ) {
            Some((
                AttentionKind::BootloaderPackage,
                "A package used in the boot path changes. Review relevant hooks and boot configuration.",
            ))
        } else if name == "systemd" {
            Some((
                AttentionKind::SystemdUpdate,
                "The system and service manager changes; affected services may need restarting.",
            ))
        } else if name == "glibc" {
            Some((
                AttentionKind::GlibcUpdate,
                "The system C library changes and is used by most dynamically linked software.",
            ))
        } else if name == "pacman" {
            Some((
                AttentionKind::PacmanUpdate,
                "The package manager itself changes during this transaction.",
            ))
        } else if is_graphics_stack(name) {
            Some((
                AttentionKind::GraphicsDriver,
                "The graphics driver or userspace graphics stack changes.",
            ))
        } else {
            None
        };
        if let Some((kind, explanation)) = finding {
            findings
                .entry(kind)
                .or_default()
                .insert(package.name.clone());
            explanations
                .entry(kind)
                .or_insert_with(|| explanation.into());
        }
        if package.action == PlannedAction::Replace {
            findings
                .entry(AttentionKind::PackageReplacement)
                .or_default()
                .insert(package.name.clone());
            explanations.entry(AttentionKind::PackageReplacement).or_insert_with(|| "Pacman metadata indicates that a package in the transaction replaces an installed package.".into());
        }
        if package.action == PlannedAction::Remove {
            findings
                .entry(AttentionKind::PackageRemoval)
                .or_default()
                .insert(package.name.clone());
            explanations
                .entry(AttentionKind::PackageRemoval)
                .or_insert_with(|| {
                    "The transaction explicitly removes an installed package.".into()
                });
        }
        if package.ignored {
            findings
                .entry(AttentionKind::IgnoredPackage)
                .or_default()
                .insert(package.name.clone());
            explanations
                .entry(AttentionKind::IgnoredPackage)
                .or_insert_with(|| {
                    "Pacman policy marks this package or one of its groups as ignored.".into()
                });
        }
    }
    if let Some(package) = packages.iter().find(|package| package.name == "python")
        && python_abi_changed(
            package.old_version.as_deref(),
            package.new_version.as_deref(),
        )
    {
        findings
            .entry(AttentionKind::PythonAbiChange)
            .or_default()
            .insert("python".into());
        explanations.entry(AttentionKind::PythonAbiChange).or_insert_with(|| "The Python major/minor interpreter version changes; native extension packages may need rebuilding.".into());
    }
    let kernel_changes = packages.iter().any(|package| is_kernel(&package.name));
    let dkms = installed
        .iter()
        .filter(|package| package.name.contains("dkms"))
        .map(|package| package.name.clone())
        .chain(
            packages
                .iter()
                .filter(|package| package.name.contains("dkms"))
                .map(|package| package.name.clone()),
        )
        .collect::<BTreeSet<_>>();
    if kernel_changes && !dkms.is_empty() {
        findings.insert(AttentionKind::DkmsInvolved, dkms);
        explanations.insert(AttentionKind::DkmsInvolved, "A kernel changes while DKMS packages are installed; module rebuild hooks should be checked after the transaction.".into());
    }
    if !modified_backups.is_empty() {
        findings.insert(
            AttentionKind::ModifiedConfiguration,
            modified_backups
                .iter()
                .map(|(package, _)| package.clone())
                .collect(),
        );
        let paths = modified_backups
            .iter()
            .map(|(_, path)| path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        explanations.insert(
            AttentionKind::ModifiedConfiguration,
            format!("Pacman reports locally modified backup files: {paths}"),
        );
    }
    if !aur_rebuilds.is_empty() {
        findings.insert(
            AttentionKind::AurRuntimeDependency,
            aur_rebuilds.iter().cloned().collect(),
        );
        explanations.insert(AttentionKind::AurRuntimeDependency, "Installed foreign/AUR packages directly depend on an ABI-sensitive runtime or library changing in this plan. This is a rebuild candidate, not proof that rebuilding is required.".into());
    }
    findings
        .into_iter()
        .map(|(kind, packages)| AttentionFinding {
            kind,
            packages: packages.into_iter().collect(),
            explanation: explanations.remove(&kind).unwrap_or_default(),
        })
        .collect()
}

fn aur_rebuild_candidates(installed: &[Package], updates: &HashSet<&str>) -> Vec<String> {
    const ABI_SENSITIVE: &[&str] = &[
        "python",
        "ruby",
        "perl",
        "nodejs",
        "icu",
        "boost-libs",
        "openssl",
        "ffmpeg",
        "electron",
    ];
    let sensitive_updates: HashSet<_> = updates
        .iter()
        .copied()
        .filter(|name| ABI_SENSITIVE.contains(name))
        .collect();
    let mut candidates = installed
        .iter()
        .filter(|package| matches!(package.source, PackageSource::Aur | PackageSource::Foreign))
        .filter(|package| {
            package
                .dependencies
                .iter()
                .any(|dependency| sensitive_updates.contains(dependency_name(dependency)))
        })
        .map(|package| package.name.clone())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn match_hooks(hooks: &[AlpmHook], packages: &[PlannedPackage]) -> Vec<ExpectedHook> {
    let mut expected = Vec::new();
    for hook in hooks {
        let matched = packages
            .iter()
            .filter(|package| {
                hook.triggers.iter().any(|trigger| {
                    trigger.trigger_type.as_deref() == Some("Package")
                        && trigger.operations.iter().any(|operation| {
                            operation
                                == match package.action {
                                    PlannedAction::Upgrade => "Upgrade",
                                    PlannedAction::Install | PlannedAction::Replace => "Install",
                                    PlannedAction::Remove => "Remove",
                                }
                        })
                        && trigger
                            .targets
                            .iter()
                            .any(|target| wildcard_matches(target, &package.name))
                })
            })
            .map(|package| package.name.clone())
            .collect::<Vec<_>>();
        if !matched.is_empty() {
            expected.push(ExpectedHook {
                name: hook.name.clone(),
                description: hook.description.clone(),
                stage: hook.stage,
                command: hook.command.clone(),
                matched_packages: matched,
            });
        }
    }
    expected.sort_by(|a, b| a.name.cmp(&b.name));
    expected
}

fn sum_if_complete<T>(values: impl Iterator<Item = Option<T>>) -> Option<T>
where
    T: std::iter::Sum<T>,
{
    values
        .collect::<Option<Vec<_>>>()
        .map(IntoIterator::into_iter)
        .map(Iterator::sum)
}

fn dependency_name(dependency: &str) -> &str {
    dependency
        .split(['<', '>', '='])
        .next()
        .unwrap_or(dependency)
        .trim()
}

fn is_kernel(name: &str) -> bool {
    matches!(name, "linux" | "linux-lts" | "linux-zen" | "linux-hardened")
}

fn is_graphics_stack(name: &str) -> bool {
    name == "mesa"
        || name.starts_with("nvidia")
        || name.starts_with("vulkan-")
        || name.starts_with("xf86-video-")
        || matches!(name, "amdvlk" | "lib32-mesa")
}

fn python_abi_changed(old: Option<&str>, new: Option<&str>) -> bool {
    fn major_minor(version: &str) -> Option<(u64, u64)> {
        let version = version.split(':').next_back()?;
        let mut parts = version.split('.');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }
    match (old.and_then(major_minor), new.and_then(major_minor)) {
        (Some(old), Some(new)) => old != new,
        _ => false,
    }
}

fn policy_matches(patterns: &[String], value: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| wildcard_matches(pattern, value))
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut remainder = value;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(position) = remainder.find(part) else {
            return false;
        };
        if index == 0 && !pattern.starts_with('*') && position != 0 {
            return false;
        }
        remainder = &remainder[position + part.len()..];
    }
    pattern.ends_with('*') || remainder.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(name: &str, old: &str, new: &str) -> PackageUpdate {
        PackageUpdate {
            name: name.into(),
            current_version: old.into(),
            new_version: new.into(),
            source: PackageSource::Official("core".into()),
            ignored: false,
        }
    }

    #[test]
    fn produces_explainable_critical_findings_without_scores() {
        let updates = vec![
            update("linux", "6.12-1", "6.13-1"),
            update("python", "3.13.2-1", "3.14.0-1"),
        ];
        let candidates = updates
            .iter()
            .map(|update| TransactionCandidate {
                name: update.name.clone(),
                version: update.new_version.clone(),
                repository: "core".into(),
                download_size: Some(10),
                installed_size: Some(20),
            })
            .collect::<Vec<_>>();
        let installed = updates
            .iter()
            .map(|update| {
                let mut package = Package::summary(
                    &update.name,
                    &update.current_version,
                    PackageSource::Official("core".into()),
                );
                package.installed = true;
                package.install_reason = crate::domain::InstallReason::Explicit;
                package.installed_size = Some(15);
                package
            })
            .collect::<Vec<_>>();
        let plan = build_flight_plan(FlightPlanInput {
            official_updates: &updates,
            aur_updates: &[],
            installed: &installed,
            transaction_candidates: &candidates,
            sync_details: &[],
            policy: PacmanPolicy::default(),
            modified_backups: &[],
            hooks: &[],
            evidence_notes: vec![],
        });
        assert_eq!(plan.download_size, Some(20));
        assert_eq!(plan.installed_size_delta, Some(10));
        assert!(
            plan.attention
                .iter()
                .any(|finding| finding.kind == AttentionKind::KernelUpdate)
        );
        assert!(
            plan.attention
                .iter()
                .any(|finding| finding.kind == AttentionKind::PythonAbiChange)
        );
    }

    #[test]
    fn wildcard_matching_is_anchored() {
        assert!(wildcard_matches("linux*", "linux-lts"));
        assert!(wildcard_matches("*dkms", "nvidia-dkms"));
        assert!(!wildcard_matches("linux", "linux-lts"));
        assert!(!wildcard_matches("vidia*", "nvidia"));
    }

    #[test]
    fn aur_updates_are_disclosed_but_not_counted_in_pacman_transaction() {
        let official = vec![update("linux", "6.12-1", "6.13-1")];
        let aur = vec![PackageUpdate {
            name: "example-bin".into(),
            current_version: "1-1".into(),
            new_version: "2-1".into(),
            source: PackageSource::Aur,
            ignored: false,
        }];
        let plan = build_flight_plan(FlightPlanInput {
            official_updates: &official,
            aur_updates: &aur,
            installed: &[],
            transaction_candidates: &[],
            sync_details: &[],
            policy: PacmanPolicy::default(),
            modified_backups: &[],
            hooks: &[],
            evidence_notes: Vec::new(),
        });
        assert_eq!(plan.packages.len(), 1);
        assert_eq!(plan.packages[0].name, "linux");
        assert_eq!(plan.separate_aur_updates, aur);
    }
}
