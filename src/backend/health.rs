use super::{CommandRunner, CommandSpec, HealthBackend};
use crate::domain::{
    ConfigArtifact, ConfigArtifactKind, FindingSeverity, HealthCategory, HealthFinding,
    HealthReport,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Default)]
pub struct SystemHealthBackend {
    runner: CommandRunner,
}

#[async_trait]
impl HealthBackend for SystemHealthBackend {
    async fn check(&self) -> Result<HealthReport> {
        let (
            database,
            package_files,
            system_units,
            user_units,
            dkms,
            orphans,
            foreign,
            configs,
            kernel,
            stale_libraries,
        ) = tokio::join!(
            self.package_database(),
            self.package_files(),
            self.failed_units(false),
            self.failed_units(true),
            self.dkms(),
            self.package_list(["-Qdtq"]),
            self.package_list(["-Qmq"]),
            self.config_artifacts(),
            self.kernel_state(),
            self.stale_library_services(),
        );
        let mut findings = Vec::new();
        let mut notes = Vec::new();
        collect_check(database, &mut findings, &mut notes);
        collect_check(package_files, &mut findings, &mut notes);
        collect_check(system_units, &mut findings, &mut notes);
        collect_check(user_units, &mut findings, &mut notes);
        collect_check(dkms, &mut findings, &mut notes);
        collect_check(kernel, &mut findings, &mut notes);
        collect_check(stale_libraries, &mut findings, &mut notes);
        let orphaned_packages = collect_list(orphans, "orphan package check", &mut notes);
        let foreign_packages = collect_list(foreign, "foreign package check", &mut notes);
        let config_artifacts = match configs {
            Ok(value) => value,
            Err(error) => {
                notes.push(format!("Configuration artifact scan unavailable: {error}"));
                Vec::new()
            }
        };
        if config_artifacts.is_empty() {
            findings.push(healthy(
                HealthCategory::Configuration,
                "No unresolved Pacman configuration artifacts found",
            ));
        } else {
            findings.push(HealthFinding {
                category: HealthCategory::Configuration,
                severity: FindingSeverity::Warning,
                title: format!(
                    "{} .pacnew/.pacsave/.pacorig files require review",
                    config_artifacts.len()
                ),
                detail:
                    "Pacman configuration artifacts were found under /etc. No files were changed."
                        .into(),
                suggested_check: Some("Review each file in the Config tab or with pacdiff.".into()),
            });
        }
        if orphaned_packages.is_empty() {
            findings.push(healthy(
                HealthCategory::Packages,
                "No orphaned packages found",
            ));
        } else {
            findings.push(HealthFinding {
                category: HealthCategory::Packages,
                severity: FindingSeverity::Advisory,
                title: format!("{} orphaned packages found", orphaned_packages.len()),
                detail: orphaned_packages.join(", "),
                suggested_check: Some(
                    "Review why each package is installed before considering removal.".into(),
                ),
            });
        }
        if !foreign_packages.is_empty() {
            findings.push(HealthFinding {
                category: HealthCategory::Packages,
                severity: FindingSeverity::Advisory,
                title: format!("{} foreign/AUR packages installed", foreign_packages.len()),
                detail: "Foreign packages are not maintained by the official repositories and may need rebuild review after runtime changes.".into(),
                suggested_check: Some("Compare foreign dependencies with the Transaction Flight Plan.".into()),
            });
        }
        Ok(HealthReport {
            checked_at: Utc::now(),
            findings,
            config_artifacts,
            orphaned_packages,
            foreign_packages,
            evidence_notes: notes,
        })
    }
}

impl SystemHealthBackend {
    async fn package_database(&self) -> Result<Vec<HealthFinding>> {
        let output = self
            .runner
            .run(CommandSpec::read_only("pacman", ["-Dk"]).with_timeout(Duration::from_secs(60)))
            .await?;
        if output.status.success() {
            Ok(vec![healthy(
                HealthCategory::PackageDatabase,
                "Package database appears consistent",
            )])
        } else {
            Ok(vec![HealthFinding {
                category: HealthCategory::PackageDatabase,
                severity: FindingSeverity::Error,
                title: "Package database consistency check failed".into(),
                detail: concise(&output.stderr, &output.stdout),
                suggested_check: Some("Inspect `pacman -Dk` output; do not remove the database lock unless no package manager is running.".into()),
            }])
        }
    }

    async fn package_files(&self) -> Result<Vec<HealthFinding>> {
        let output = self
            .runner
            .run(CommandSpec::read_only("pacman", ["-Qk"]).with_timeout(Duration::from_secs(120)))
            .await?;
        let missing = output
            .stdout
            .lines()
            .chain(output.stderr.lines())
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("missing file")
                    || lower.contains("warning: ") && lower.contains("missing")
            })
            .take(50)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if output.status.success() && missing.is_empty() {
            Ok(vec![healthy(
                HealthCategory::Packages,
                "No missing package-owned files reported by pacman -Qk",
            )])
        } else if !missing.is_empty() {
            Ok(vec![HealthFinding {
                category: HealthCategory::Packages,
                severity: FindingSeverity::Warning,
                title: "Missing package-owned filesystem entries detected".into(),
                detail: missing.join("; "),
                suggested_check: Some(
                    "Inspect affected packages with `pacman -Qkk <package>` before deciding whether files should be restored."
                        .into(),
                ),
            }])
        } else {
            anyhow::bail!(
                "package-owned file check failed: {}",
                concise(&output.stderr, &output.stdout)
            )
        }
    }

    async fn failed_units(&self, user: bool) -> Result<Vec<HealthFinding>> {
        let mut args = Vec::new();
        if user {
            args.push("--user");
        }
        args.extend(["--failed", "--no-legend", "--plain"]);
        let output = self
            .runner
            .run(CommandSpec::read_only("systemctl", args))
            .await?;
        let category = if user {
            HealthCategory::UserServices
        } else {
            HealthCategory::SystemServices
        };
        let scope = if user { "user" } else { "system" };
        if output.status.success() && output.stdout.trim().is_empty() {
            Ok(vec![healthy(
                category,
                &format!("No failed {scope} services"),
            )])
        } else if output.status.success() {
            let units = output
                .stdout
                .lines()
                .filter_map(|line| line.split_whitespace().next())
                .collect::<Vec<_>>();
            Ok(vec![HealthFinding {
                category,
                severity: FindingSeverity::Warning,
                title: format!("{} failed {scope} services", units.len()),
                detail: units.join(", "),
                suggested_check: Some(format!(
                    "Inspect with `systemctl {}status <unit>` and the journal.",
                    if user { "--user " } else { "" }
                )),
            }])
        } else {
            anyhow::bail!(
                "systemctl {scope} check failed: {}",
                concise(&output.stderr, &output.stdout)
            )
        }
    }

    async fn dkms(&self) -> Result<Vec<HealthFinding>> {
        let output = self
            .runner
            .run(CommandSpec::read_only("dkms", ["status"]))
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "dkms status failed: {}",
                concise(&output.stderr, &output.stdout)
            );
        }
        let problematic = output
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.ends_with(": installed"))
            .collect::<Vec<_>>();
        if problematic.is_empty() {
            Ok(vec![healthy(
                HealthCategory::Dkms,
                "No obvious DKMS status failures",
            )])
        } else {
            Ok(vec![HealthFinding {
                category: HealthCategory::Dkms,
                severity: FindingSeverity::Warning,
                title: format!("{} DKMS entries are not installed", problematic.len()),
                detail: problematic.join("; "),
                suggested_check: Some(
                    "Inspect DKMS build logs and verify matching kernel headers.".into(),
                ),
            }])
        }
    }

    async fn package_list<const N: usize>(&self, args: [&str; N]) -> Result<Vec<String>> {
        let output = self
            .runner
            .run(CommandSpec::read_only("pacman", args))
            .await?;
        if output.status.success() || output.status.code() == Some(1) {
            Ok(output.stdout.lines().map(ToOwned::to_owned).collect())
        } else {
            anyhow::bail!(
                "pacman query failed: {}",
                concise(&output.stderr, &output.stdout)
            )
        }
    }

    async fn config_artifacts(&self) -> Result<Vec<ConfigArtifact>> {
        let output = self
            .runner
            .run(
                CommandSpec::read_only(
                    "find",
                    [
                        "/etc",
                        "-xdev",
                        "-type",
                        "f",
                        "(",
                        "-name",
                        "*.pacnew",
                        "-o",
                        "-name",
                        "*.pacsave",
                        "-o",
                        "-name",
                        "*.pacorig",
                        ")",
                        "-print",
                    ],
                )
                .with_timeout(Duration::from_secs(30)),
            )
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "find /etc failed: {}",
                concise(&output.stderr, &output.stdout)
            );
        }
        let mut artifacts = output
            .stdout
            .lines()
            .filter_map(|path| {
                let kind = if path.ends_with(".pacnew") {
                    ConfigArtifactKind::Pacnew
                } else if path.ends_with(".pacsave") {
                    ConfigArtifactKind::Pacsave
                } else if path.ends_with(".pacorig") {
                    ConfigArtifactKind::Pacorig
                } else {
                    return None;
                };
                Some(ConfigArtifact {
                    kind,
                    path: path.to_owned(),
                })
            })
            .collect::<Vec<_>>();
        artifacts.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(artifacts)
    }

    async fn kernel_state(&self) -> Result<Vec<HealthFinding>> {
        let output = self
            .runner
            .run_checked(CommandSpec::read_only("uname", ["-r"]))
            .await?;
        let running = output.stdout.trim();
        if Path::new("/usr/lib/modules").join(running).is_dir() {
            Ok(vec![healthy(
                HealthCategory::Kernel,
                "Running kernel has a matching installed module tree",
            )])
        } else {
            Ok(vec![HealthFinding {
                category: HealthCategory::Kernel,
                severity: FindingSeverity::Warning,
                title: "Running kernel differs from installed module trees".into(),
                detail: format!("The running kernel is {running}, but /usr/lib/modules/{running} is absent."),
                suggested_check: Some("Reboot advisable after confirming the newly installed kernel and boot artifacts are healthy.".into()),
            }])
        }
    }

    async fn stale_library_services(&self) -> Result<Vec<HealthFinding>> {
        let mut processes = Vec::new();
        let mut unreadable = 0_usize;
        let mut entries = tokio::fs::read_dir("/proc").await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .filter(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
            else {
                continue;
            };
            let maps = match tokio::fs::read_to_string(entry.path().join("maps")).await {
                Ok(maps) => maps,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    unreadable += 1;
                    continue;
                }
                Err(_) => continue,
            };
            if !maps
                .lines()
                .any(|line| line.contains(".so") && line.contains(" (deleted)"))
            {
                continue;
            }
            let service = tokio::fs::read_to_string(entry.path().join("cgroup"))
                .await
                .ok()
                .and_then(|cgroup| service_from_cgroup(&cgroup));
            processes.push(service.unwrap_or_else(|| format!("pid {pid}")));
        }
        processes.sort();
        processes.dedup();
        if processes.is_empty() {
            Ok(vec![HealthFinding {
                category: HealthCategory::ServiceRestarts,
                severity: FindingSeverity::Healthy,
                title: "No obvious stale shared-library mappings detected".into(),
                detail: if unreadable == 0 {
                    String::new()
                } else {
                    format!("{unreadable} process map(s) were not readable as this user.")
                },
                suggested_check: None,
            }])
        } else {
            Ok(vec![HealthFinding {
                category: HealthCategory::ServiceRestarts,
                severity: FindingSeverity::Advisory,
                title: format!(
                    "{} service/process entries map deleted shared libraries",
                    processes.len()
                ),
                detail: processes.join(", "),
                suggested_check: Some(
                    "A service restart may be required. Inspect each process and restart it only when operationally appropriate."
                        .into(),
                ),
            }])
        }
    }
}

fn service_from_cgroup(cgroup: &str) -> Option<String> {
    cgroup.lines().find_map(|line| {
        line.split('/')
            .find(|component| component.ends_with(".service"))
            .map(ToOwned::to_owned)
    })
}

fn healthy(category: HealthCategory, title: &str) -> HealthFinding {
    HealthFinding {
        category,
        severity: FindingSeverity::Healthy,
        title: title.into(),
        detail: String::new(),
        suggested_check: None,
    }
}

fn collect_check(
    result: Result<Vec<HealthFinding>>,
    findings: &mut Vec<HealthFinding>,
    notes: &mut Vec<String>,
) {
    match result {
        Ok(mut value) => findings.append(&mut value),
        Err(error) => notes.push(error.to_string()),
    }
}

fn collect_list(result: Result<Vec<String>>, label: &str, notes: &mut Vec<String>) -> Vec<String> {
    match result {
        Ok(value) => value,
        Err(error) => {
            notes.push(format!("{label} unavailable: {error}"));
            Vec::new()
        }
    }
}

fn concise(stderr: &str, stdout: &str) -> String {
    let value = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    value.chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_systemd_service_from_cgroup_without_guessing() {
        let cgroup = "0::/system.slice/sshd.service\n";
        assert_eq!(service_from_cgroup(cgroup).as_deref(), Some("sshd.service"));
        assert!(service_from_cgroup("0::/user.slice/user-1000.slice").is_none());
    }
}
