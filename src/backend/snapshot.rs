use super::SnapshotBackend;
use crate::domain::Snapshot;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::Utc;
use std::{ffi::OsString, process::Stdio};
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    Snapper,
    Timeshift,
    Disabled,
}

#[derive(Debug, Clone)]
pub struct SystemSnapshotBackend {
    kind: SnapshotKind,
}

impl SystemSnapshotBackend {
    pub fn new(kind: SnapshotKind) -> Self {
        Self { kind }
    }

    fn create_command(&self, description: &str) -> Result<(OsString, Vec<OsString>)> {
        if description.is_empty() || description.chars().any(|character| character.is_control()) {
            bail!("invalid snapshot description");
        }
        let (program, args): (&str, Vec<&str>) = match self.kind {
            SnapshotKind::Snapper => (
                "snapper",
                vec![
                    "create",
                    "--type",
                    // This is created immediately before the package transaction,
                    // but it is deliberately a standalone Snapper snapshot. A
                    // Snapper `pre` snapshot is semantically incomplete until a
                    // corresponding `post` snapshot is linked to it.
                    "single",
                    "--cleanup-algorithm",
                    "number",
                    "--description",
                    description,
                    "--print-number",
                ],
            ),
            SnapshotKind::Timeshift => (
                "timeshift",
                vec!["--create", "--comments", description, "--scripted"],
            ),
            SnapshotKind::Disabled => bail!("snapshot support is unavailable"),
        };
        let mut sudo_args = vec![OsString::from("-n"), OsString::from("--"), program.into()];
        sudo_args.extend(args.into_iter().map(OsString::from));
        Ok(("sudo".into(), sudo_args))
    }
}

#[async_trait]
impl SnapshotBackend for SystemSnapshotBackend {
    fn name(&self) -> Option<&'static str> {
        match self.kind {
            SnapshotKind::Snapper => Some("Snapper"),
            SnapshotKind::Timeshift => Some("Timeshift"),
            SnapshotKind::Disabled => None,
        }
    }

    async fn available(&self) -> bool {
        self.name().is_some()
    }

    async fn create_pre_transaction(&self, description: &str) -> Result<Snapshot> {
        let (program, args) = self.create_command(description)?;
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .context("failed to start snapshot backend")?;
        if !output.status.success() {
            bail!(
                "{} snapshot failed: {}",
                self.name().unwrap_or("unknown"),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let created_at = Utc::now();
        let id = match self.kind {
            SnapshotKind::Snapper => {
                let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if id.is_empty() {
                    bail!("Snapper succeeded but returned no snapshot identifier");
                }
                Some(id)
            }
            SnapshotKind::Timeshift => None,
            SnapshotKind::Disabled => unreachable!("disabled handled by create_command"),
        };
        Ok(Snapshot {
            backend: self.name().unwrap_or("unknown").into(),
            id,
            description: description.into(),
            created_at: Some(created_at),
        })
    }

    async fn list(&self) -> Result<Vec<Snapshot>> {
        let (program, args): (&str, &[&str]) = match self.kind {
            SnapshotKind::Snapper => (
                "snapper",
                &[
                    "--jsonout",
                    "--utc",
                    "--iso",
                    "list",
                    "--columns",
                    "number,date,description",
                ],
            ),
            SnapshotKind::Timeshift => ("timeshift", &["--list", "--scripted"]),
            SnapshotKind::Disabled => return Ok(Vec::new()),
        };
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .with_context(|| {
                format!(
                    "failed to list {} snapshots",
                    self.name().unwrap_or(program)
                )
            })?;
        if !output.status.success() {
            bail!(
                "{} snapshot listing failed without privilege escalation: {}",
                self.name().unwrap_or(program),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let stdout = String::from_utf8(output.stdout).context("snapshot list was not UTF-8")?;
        match self.kind {
            SnapshotKind::Snapper => parse_snapper_json(&stdout),
            SnapshotKind::Timeshift => Ok(parse_timeshift_list(&stdout)),
            SnapshotKind::Disabled => Ok(Vec::new()),
        }
    }
}

fn parse_snapper_json(input: &str) -> Result<Vec<Snapshot>> {
    let value: serde_json::Value = serde_json::from_str(input).context("invalid Snapper JSON")?;
    let records = value
        .as_array()
        .or_else(|| value.get("snapshots").and_then(serde_json::Value::as_array))
        .context("Snapper JSON did not contain a snapshot array")?;
    Ok(records
        .iter()
        .filter_map(|record| {
            let id = json_field(record, &["number", "Number"])?;
            if id == "0" {
                return None;
            }
            let description = json_field(record, &["description", "Description"])
                .unwrap_or_else(|| "Snapper snapshot".into());
            let created_at = json_field(record, &["date", "Date"])
                .and_then(|date| chrono::DateTime::parse_from_rfc3339(&date).ok())
                .map(|date| date.with_timezone(&Utc));
            Some(Snapshot {
                backend: "Snapper".into(),
                id: Some(id),
                description,
                created_at,
            })
        })
        .collect())
}

fn json_field(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let value = value.get(*name)?;
        value
            .as_str()
            .map(ToOwned::to_owned)
            .or_else(|| value.as_u64().map(|number| number.to_string()))
    })
}

fn parse_timeshift_list(input: &str) -> Vec<Snapshot> {
    input
        .lines()
        .filter_map(|line| {
            let id = line
                .split_whitespace()
                .find(|field| parse_timeshift_date(field).is_some())?;
            Some(Snapshot {
                backend: "Timeshift".into(),
                id: Some(id.into()),
                description: line
                    .split_once(id)
                    .map(|(_, rest)| rest.trim().to_owned())
                    .filter(|description| !description.is_empty())
                    .unwrap_or_else(|| "Timeshift snapshot".into()),
                created_at: parse_timeshift_date(id),
            })
        })
        .collect()
}

fn parse_timeshift_date(value: &str) -> Option<chrono::DateTime<Utc>> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d_%H-%M-%S")
        .ok()
        .map(|date| date.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_commands_are_privilege_scoped_and_argument_safe() {
        let backend = SystemSnapshotBackend::new(SnapshotKind::Snapper);
        let (program, args) = backend
            .create_command("arch-maint pre-upgrade")
            .expect("command");
        assert_eq!(program, "sudo");
        assert_eq!(&args[..3], ["-n", "--", "snapper"]);
        assert!(args.windows(2).any(|pair| pair == ["--type", "single"]));
        assert_eq!(args.last(), Some(&OsString::from("--print-number")));
    }

    #[test]
    fn parses_snapper_machine_output_and_timeshift_identifiers() {
        let snapper =
            r#"[{"number":42,"date":"2026-08-17T12:00:00Z","description":"pre upgrade"}]"#;
        let snapshots = parse_snapper_json(snapper).expect("snapper JSON");
        assert_eq!(snapshots[0].id.as_deref(), Some("42"));
        let timeshift = parse_timeshift_list("0 > 2026-08-17_12-00-00 O before upgrade\n");
        assert_eq!(timeshift[0].id.as_deref(), Some("2026-08-17_12-00-00"));
    }
}
