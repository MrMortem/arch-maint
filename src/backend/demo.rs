use super::{
    AurBackend, ConfigBackend, FlightPlanBackend, HealthBackend, HistoryBackend, HookBackend,
    HygieneBackend, NewsBackend, PackageBackend, RemovalBackend, TransactionBackend,
};
use crate::{
    analysis::{FlightPlanInput, build_flight_plan},
    domain::*,
    parser::{TransactionCandidate, parse_alpm_hook},
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::DateTime;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Default)]
pub struct DemoPackageBackend;

#[derive(Debug, Clone, Default)]
pub struct DemoAurBackend;

#[derive(Debug, Clone, Default)]
pub struct DemoHistoryBackend;

#[derive(Debug, Clone, Default)]
pub struct DemoHealthBackend;

#[derive(Debug, Clone, Default)]
pub struct DemoTransactionBackend;

fn demo_packages() -> Vec<Package> {
    let mut linux = Package::summary(
        "linux",
        "6.12.8.arch1-1",
        PackageSource::Official("core".into()),
    );
    linux.description = Some("The Linux kernel and modules".into());
    linux.installed = true;
    linux.install_reason = InstallReason::Explicit;
    linux.installed_size = Some(137 * 1024 * 1024);
    linux.dependencies = vec!["coreutils".into(), "kmod".into(), "initramfs".into()];
    linux.optional_dependencies = vec!["wireless-regdb: wireless regulatory database".into()];
    linux.reverse_dependencies = vec!["virtualbox-host-modules-arch".into()];
    linux.url = Some("https://github.com/archlinux/linux".into());
    linux.packager = Some("Arch Linux Team".into());
    linux.install_date = Some("Sun 12 Jan 2025 10:30:00 AM EST".into());

    let mut mesa = Package::summary("mesa", "24.3.3-1", PackageSource::Official("extra".into()));
    mesa.description = Some("Open-source OpenGL drivers".into());
    mesa.installed = true;
    mesa.install_reason = InstallReason::Dependency;
    mesa.installed_size = Some(45 * 1024 * 1024);
    mesa.dependencies = vec!["libdrm".into(), "wayland".into(), "glibc".into()];
    mesa.reverse_dependencies = vec!["sway".into(), "firefox".into(), "obs-studio".into()];
    mesa.url = Some("https://www.mesa3d.org/".into());

    let mut ripgrep = Package::summary(
        "ripgrep",
        "14.1.1-1",
        PackageSource::Official("extra".into()),
    );
    ripgrep.description = Some("A search tool that combines grep with sensible defaults".into());
    ripgrep.installed = true;
    ripgrep.install_reason = InstallReason::Explicit;
    ripgrep.installed_size = Some(6_420_000);
    ripgrep.dependencies = vec!["gcc-libs".into(), "pcre2".into()];
    ripgrep.licenses = vec!["MIT".into(), "Unlicense".into()];
    vec![linux, mesa, ripgrep]
}

#[async_trait]
impl PackageBackend for DemoPackageBackend {
    async fn installed_packages(&self) -> Result<Vec<Package>> {
        Ok(demo_packages())
    }

    async fn search_official(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_ascii_lowercase();
        Ok(demo_packages()
            .into_iter()
            .filter(|package| {
                package.name.contains(&query)
                    || package
                        .description
                        .as_deref()
                        .is_some_and(|value| value.to_ascii_lowercase().contains(&query))
            })
            .collect())
    }

    async fn package_details(&self, package: &Package) -> Result<Package> {
        Ok(demo_packages()
            .into_iter()
            .find(|candidate| candidate.name == package.name)
            .unwrap_or_else(|| package.clone()))
    }

    async fn check_updates(&self) -> Result<Vec<PackageUpdate>> {
        Ok(UpdateSet::demo().official)
    }
}

#[async_trait]
impl FlightPlanBackend for DemoPackageBackend {
    async fn build_flight_plan(
        &self,
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan> {
        let candidates = official_updates
            .iter()
            .map(|update| TransactionCandidate {
                name: update.name.clone(),
                version: update.new_version.clone(),
                repository: if update.name == "linux" {
                    "core"
                } else {
                    "extra"
                }
                .into(),
                download_size: Some(if update.name == "linux" {
                    132 * 1024 * 1024
                } else {
                    18 * 1024 * 1024
                }),
                installed_size: Some(if update.name == "linux" {
                    140 * 1024 * 1024
                } else {
                    48 * 1024 * 1024
                }),
            })
            .collect::<Vec<_>>();
        let hook = parse_alpm_hook(
            "60-mkinitcpio-remove.hook",
            "[Trigger]\nOperation=Upgrade\nType=Package\nTarget=linux*\n[Action]\nDescription=Creating temporary files\nWhen=PreTransaction\nExec=/usr/bin/true\n",
        )
        .into_iter()
        .collect::<Vec<_>>();
        Ok(build_flight_plan(FlightPlanInput {
            official_updates,
            aur_updates,
            installed,
            transaction_candidates: &candidates,
            sync_details: &[],
            policy: PacmanPolicy {
                ignore_packages: vec!["linux-lts".into()],
                ignore_groups: Vec::new(),
                hold_packages: vec!["pacman".into()],
            },
            modified_backups: &[("linux".into(), "/etc/mkinitcpio.conf".into())],
            hooks: &hook,
            evidence_notes: vec!["Demo plan uses deterministic package metadata.".into()],
        }))
    }

    async fn build_install_flight_plan(
        &self,
        targets: &[String],
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan> {
        let mut plan = self
            .build_flight_plan(official_updates, aur_updates, installed)
            .await?;
        let mut added_targets = 0_u64;
        for target in targets {
            // An installed package that is already in the sync plan is an upgrade,
            // not an additional install. This mirrors the real pacman preview and
            // keeps demo-mode transaction totals honest.
            if plan.packages.iter().any(|package| package.name == *target)
                || installed.iter().any(|package| package.name == *target)
            {
                continue;
            }
            plan.packages.push(PlannedPackage {
                name: target.clone(),
                old_version: None,
                new_version: Some("latest-demo".into()),
                source: PackageSource::Official("extra".into()),
                action: PlannedAction::Install,
                download_size: Some(8 * 1024 * 1024),
                installed_size_delta: Some(20 * 1024 * 1024),
                ignored: false,
            });
            added_targets += 1;
        }
        plan.download_size = plan
            .download_size
            .map(|size| size + added_targets * 8 * 1024 * 1024);
        plan.installed_size_delta = plan
            .installed_size_delta
            .map(|size| size + added_targets as i64 * 20 * 1024 * 1024);
        Ok(plan)
    }
}

#[async_trait]
impl RemovalBackend for DemoPackageBackend {
    async fn simulate_removal(
        &self,
        packages: &[String],
        remove_unused: bool,
    ) -> Result<RemovalPlan> {
        let installed = demo_packages();
        let direct_removals = packages
            .iter()
            .filter_map(|name| installed.iter().find(|package| package.name == *name))
            .map(|package| RemovalCandidate {
                name: package.name.clone(),
                version: package.version.clone(),
                installed_size: package.installed_size,
            })
            .collect::<Vec<_>>();
        let dependencies_becoming_unused =
            if remove_unused && packages.iter().any(|name| name == "ripgrep") {
                vec![RemovalCandidate {
                    name: "pcre2".into(),
                    version: "10.44-1".into(),
                    installed_size: Some(2 * 1024 * 1024),
                }]
            } else {
                Vec::new()
            };
        let space_reclaimed = direct_removals
            .iter()
            .chain(&dependencies_becoming_unused)
            .map(|package| package.installed_size)
            .collect::<Option<Vec<_>>>()
            .map(|sizes| sizes.into_iter().sum());
        Ok(RemovalPlan {
            requested: packages.to_vec(),
            blocked: direct_removals.len() != packages.len(),
            direct_removals,
            dependencies_becoming_unused,
            affected_packages: Vec::new(),
            space_reclaimed,
            evidence_notes: vec!["Demo removal plan; no changes have been made.".into()],
        })
    }
}

#[async_trait]
impl HygieneBackend for DemoPackageBackend {
    async fn inspect(&self, installed: &[Package]) -> Result<HygieneReport> {
        let explicit_packages = installed
            .iter()
            .filter(|package| package.install_reason == InstallReason::Explicit)
            .map(|package| package.name.clone())
            .collect();
        let dependency_packages = installed
            .iter()
            .filter(|package| package.install_reason == InstallReason::Dependency)
            .map(|package| package.name.clone())
            .collect();
        Ok(HygieneReport {
            explicit_packages,
            dependency_packages,
            orphaned_packages: Vec::new(),
            foreign_packages: vec!["visual-studio-code-bin".into()],
            cache_entries: vec![CacheEntry {
                path: "/var/cache/pacman/pkg/linux-6.12.7.arch1-1-x86_64.pkg.tar.zst".into(),
                package: Some("linux".into()),
                version: Some("6.12.7.arch1-1".into()),
                size: 128 * 1024 * 1024,
                current_installed_version: false,
            }],
            cache_size: 128 * 1024 * 1024,
            old_cached_versions_size: 128 * 1024 * 1024,
            evidence_notes: vec!["Demo hygiene report; cleanup is preview-only.".into()],
        })
    }
}

fn aur_packages() -> Vec<Package> {
    let mut paru = Package::summary("paru", "2.0.4-1", PackageSource::Aur);
    paru.description = Some("Feature packed AUR helper".into());
    paru.dependencies = vec!["git".into(), "pacman".into()];
    paru.url = Some("https://github.com/Morganamilo/paru".into());
    paru.aur = Some(AurMetadata {
        package_base: Some("paru".into()),
        maintainer: Some("Morganamilo".into()),
        votes: 2460,
        popularity: 17.4,
        last_modified: Some(
            DateTime::parse_from_rfc3339("2025-01-10T12:00:00Z")
                .expect("valid fixture date")
                .into(),
        ),
        ..AurMetadata::default()
    });
    let mut code = Package::summary("visual-studio-code-bin", "1.96.3-1", PackageSource::Aur);
    code.description = Some("Visual Studio Code (binary release)".into());
    code.installed = true;
    code.dependencies = vec!["libx11".into(), "gtk3".into()];
    code.aur = Some(AurMetadata {
        maintainer: Some("microsoft".into()),
        votes: 1320,
        popularity: 11.2,
        ..AurMetadata::default()
    });
    vec![paru, code]
}

#[async_trait]
impl AurBackend for DemoAurBackend {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_ascii_lowercase();
        Ok(aur_packages()
            .into_iter()
            .filter(|package| {
                package.name.contains(&query)
                    || package
                        .description
                        .as_deref()
                        .is_some_and(|value| value.to_ascii_lowercase().contains(&query))
            })
            .collect())
    }

    async fn info(&self, package: &str) -> Result<Option<Package>> {
        Ok(aur_packages()
            .into_iter()
            .find(|candidate| candidate.name == package))
    }

    async fn check_updates(&self) -> Result<Vec<PackageUpdate>> {
        Ok(UpdateSet::demo().aur)
    }

    async fn fetch_pkgbuild(&self, package: &str) -> Result<String> {
        Ok(format!("pkgname={package}\npkgver=1.0\npkgrel=1\n"))
    }

    async fn review_pkgbuild(&self, package: &Package) -> Result<PkgbuildReview> {
        let previous = format!(
            "pkgname={}\npkgver=0.9\ndepends=('git')\nsource=('https://old.example/{}.tar.gz')\nsha256sums=('old')\nbuild() {{\n  make\n}}\n",
            package.name, package.name
        );
        let current = format!(
            "pkgname={}\npkgver=1.0\ndepends=('git' 'rust')\nsource=('https://downloads.example.org/{}.tar.gz')\nsha256sums=('new')\nbuild() {{\n  cargo build --release\n}}\n",
            package.name, package.name
        );
        Ok(crate::parser::review_pkgbuild(
            package.name.clone(),
            Some(("demo helper cache", &previous)),
            current,
        ))
    }
}

#[async_trait]
impl NewsBackend for DemoAurBackend {
    async fn latest(&self) -> Result<Vec<ArchNewsItem>> {
        Ok(vec![ArchNewsItem {
            title: "Demo Arch news: review manual intervention notices before upgrading".into(),
            link: "https://archlinux.org/news/".into(),
            published: Some("Mon, 17 Aug 2026 12:00:00 +0000".into()),
            summary: Some(
                "Demo content illustrates the read-only news view; follow the linked official notice."
                    .into(),
            ),
        }])
    }
}

#[async_trait]
impl HookBackend for DemoPackageBackend {
    async fn hooks(&self) -> Result<Vec<HookDefinition>> {
        Ok(vec![HookDefinition {
            name: "90-mkinitcpio-install.hook".into(),
            description: "Updating Linux initcpios".into(),
            stage: HookStage::PostTransaction,
            command: Some("/usr/share/libalpm/scripts/mkinitcpio install".into()),
            operations: vec!["Install".into(), "Upgrade".into()],
            targets: vec!["usr/lib/modules/*/vmlinuz".into()],
        }])
    }
}

#[async_trait]
impl HistoryBackend for DemoHistoryBackend {
    async fn transactions(&self) -> Result<Vec<HistoryTransaction>> {
        let tz = chrono::FixedOffset::west_opt(5 * 3600).expect("valid offset");
        let date = DateTime::parse_from_str("2025-01-12T10:30:00-0500", "%Y-%m-%dT%H:%M:%S%z")
            .expect("valid date");
        Ok(vec![HistoryTransaction {
            started_at: date.with_timezone(&tz),
            completed: true,
            kind: TransactionKind::SystemUpgrade,
            command_line: Some("pacman -Syu".into()),
            changes: vec![
                PackageChange {
                    action: PackageAction::Upgraded,
                    name: "linux".into(),
                    old_version: Some("6.12.7.arch1-1".into()),
                    new_version: Some("6.12.8.arch1-1".into()),
                },
                PackageChange {
                    action: PackageAction::Upgraded,
                    name: "mesa".into(),
                    old_version: Some("24.3.2-1".into()),
                    new_version: Some("24.3.3-1".into()),
                },
            ],
        }])
    }
}

#[async_trait]
impl HealthBackend for DemoHealthBackend {
    async fn check(&self) -> Result<HealthReport> {
        Ok(HealthReport {
            checked_at: chrono::Utc::now(),
            findings: vec![
                HealthFinding {
                    category: HealthCategory::PackageDatabase,
                    severity: FindingSeverity::Healthy,
                    title: "Package database appears consistent".into(),
                    detail: String::new(),
                    suggested_check: None,
                },
                HealthFinding {
                    category: HealthCategory::Configuration,
                    severity: FindingSeverity::Warning,
                    title: "2 .pacnew/.pacsave files require review".into(),
                    detail: "Demo configuration artifacts; no files have been changed.".into(),
                    suggested_check: Some("Review each file with pacdiff.".into()),
                },
                HealthFinding {
                    category: HealthCategory::Kernel,
                    severity: FindingSeverity::Advisory,
                    title: "Running kernel has a matching installed module tree".into(),
                    detail: String::new(),
                    suggested_check: None,
                },
            ],
            config_artifacts: vec![
                ConfigArtifact {
                    kind: ConfigArtifactKind::Pacnew,
                    path: "/etc/ssh/sshd_config.pacnew".into(),
                },
                ConfigArtifact {
                    kind: ConfigArtifactKind::Pacsave,
                    path: "/etc/example.conf.pacsave".into(),
                },
            ],
            orphaned_packages: Vec::new(),
            foreign_packages: vec!["visual-studio-code-bin".into()],
            evidence_notes: vec!["Demo health data is deterministic.".into()],
        })
    }
}

#[async_trait]
impl ConfigBackend for DemoHealthBackend {
    async fn review(&self, artifact: &ConfigArtifact) -> Result<ConfigReview> {
        let current = "Port 22\nPermitRootLogin no\n".to_owned();
        let incoming = "Port 22\nPermitRootLogin prohibit-password\n".to_owned();
        let suffix = match artifact.kind {
            ConfigArtifactKind::Pacnew => ".pacnew",
            ConfigArtifactKind::Pacsave => ".pacsave",
            ConfigArtifactKind::Pacorig => ".pacorig",
        };
        let current_path = artifact.path.trim_end_matches(suffix).to_owned();
        Ok(ConfigReview {
            artifact: artifact.clone(),
            unified_diff: Some(crate::parser::unified_diff(
                &current,
                &incoming,
                &current_path,
                &artifact.path,
            )),
            current_path,
            current_content: Some(current),
            artifact_content: incoming,
            evidence_notes: vec!["Demo review is read-only; no files have been changed.".into()],
        })
    }
}

#[async_trait]
impl TransactionBackend for DemoTransactionBackend {
    fn command_preview(&self, request: &TransactionRequest) -> Result<Vec<String>> {
        Ok(vec!["demo-transaction".into(), request.label().into()])
    }

    async fn execute(
        &self,
        request: TransactionRequest,
        events: mpsc::UnboundedSender<TransactionEvent>,
        mut controls: mpsc::UnboundedReceiver<TransactionControl>,
    ) -> Result<()> {
        let command = self.command_preview(&request)?;
        events
            .send(TransactionEvent::Started {
                command: command.clone(),
            })
            .ok();
        events
            .send(TransactionEvent::Output {
                stream: OutputStream::Stdout,
                chunk: ":: demo transaction started\n(1/2) upgrading linux\n".into(),
            })
            .ok();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let cancelled = matches!(controls.try_recv(), Ok(TransactionControl::Cancel));
        events
            .send(TransactionEvent::Output {
                stream: if cancelled {
                    OutputStream::Stderr
                } else {
                    OutputStream::Stdout
                },
                chunk: if cancelled {
                    "transaction cancelled\n"
                } else {
                    "(2/2) upgrading mesa\n:: demo transaction complete\n"
                }
                .into(),
            })
            .ok();
        events
            .send(TransactionEvent::Finished(Box::new(TransactionResult {
                command,
                exit_code: Some(if cancelled { 130 } else { 0 }),
                stdout: if cancelled {
                    ""
                } else {
                    "upgrading linux\nupgrading mesa\n"
                }
                .into(),
                stderr: if cancelled {
                    "transaction cancelled"
                } else {
                    ""
                }
                .into(),
                cancelled,
                hooks: vec![HookExecution {
                    description: "Reloading system manager configuration".into(),
                    stage: HookExecutionStage::PostTransaction,
                    status: HookExecutionStatus::Succeeded,
                    output: Vec::new(),
                }],
            })))
            .ok();
        Ok(())
    }
}

pub fn demo_system_profile() -> SystemProfile {
    SystemProfile {
        is_arch: true,
        distro_name: "Arch Linux (demo)".into(),
        running_as_root: false,
        tools: ToolAvailability {
            pacman: true,
            checkupdates: true,
            pacdiff: true,
            paru: true,
            yay: false,
            snapper: true,
            timeshift: false,
        },
        selected_aur_helper: Some("paru".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn selected_installed_update_is_not_counted_as_a_new_install() {
        let backend = DemoPackageBackend;
        let updates = UpdateSet::demo();
        let plan = backend
            .build_install_flight_plan(
                &["linux".into()],
                &updates.official,
                &updates.aur,
                &demo_packages(),
            )
            .await
            .expect("demo flight plan should build");

        assert_eq!(
            plan.packages
                .iter()
                .filter(|package| package.name == "linux")
                .count(),
            1
        );
        assert!(
            plan.packages
                .iter()
                .all(|package| package.action != PlannedAction::Install)
        );
    }
}
