use super::ConfigBackend;
use crate::{
    domain::{ConfigArtifact, ConfigArtifactKind, ConfigReview},
    parser::unified_diff,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct PacdiffBackend;

#[async_trait]
impl ConfigBackend for PacdiffBackend {
    async fn review(&self, artifact: &ConfigArtifact) -> Result<ConfigReview> {
        let artifact_path = validated_artifact_path(artifact)?;
        let current_path = base_path(&artifact_path, artifact.kind)?;
        let artifact_content = read_bounded(&artifact_path).await?;
        let mut evidence_notes = Vec::new();
        let current_content = match read_bounded(&current_path).await {
            Ok(content) => Some(content),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                evidence_notes.push(format!(
                    "The current path {} does not exist.",
                    current_path.display()
                ));
                None
            }
            Err(error) => return Err(error),
        };
        let diff = current_content.as_ref().map(|current| {
            unified_diff(
                current,
                &artifact_content,
                &current_path.display().to_string(),
                &artifact_path.display().to_string(),
            )
        });
        evidence_notes.push(
            "This view is read-only. Use pacdiff for package-aware reconciliation; no files have been changed."
                .into(),
        );
        Ok(ConfigReview {
            artifact: artifact.clone(),
            current_path: current_path.display().to_string(),
            current_content,
            artifact_content,
            unified_diff: diff,
            evidence_notes,
        })
    }
}

fn validated_artifact_path(artifact: &ConfigArtifact) -> Result<PathBuf> {
    let path = PathBuf::from(&artifact.path);
    if !path.is_absolute() || !path.starts_with("/etc") || base_path(&path, artifact.kind).is_err()
    {
        bail!(
            "invalid Pacman configuration artifact path `{}`",
            artifact.path
        );
    }
    Ok(path)
}

fn base_path(path: &Path, kind: ConfigArtifactKind) -> Result<PathBuf> {
    let suffix = match kind {
        ConfigArtifactKind::Pacnew => ".pacnew",
        ConfigArtifactKind::Pacsave => ".pacsave",
        ConfigArtifactKind::Pacorig => ".pacorig",
    };
    let value = path
        .to_str()
        .context("configuration path is not valid UTF-8")?;
    let base = value
        .strip_suffix(suffix)
        .context("configuration path does not match its artifact type")?;
    Ok(PathBuf::from(base))
}

async fn read_bounded(path: &Path) -> Result<String> {
    const LIMIT: u64 = 5 * 1024 * 1024;
    let metadata = tokio::fs::metadata(path).await?;
    if metadata.len() > LIMIT {
        bail!("{} is larger than the 5 MiB review limit", path.display());
    }
    tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read {} as text", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_current_path_only_from_matching_suffix() {
        assert_eq!(
            base_path(
                Path::new("/etc/pacman.conf.pacnew"),
                ConfigArtifactKind::Pacnew
            )
            .expect("path"),
            PathBuf::from("/etc/pacman.conf")
        );
        assert!(
            base_path(
                Path::new("/etc/pacman.conf.pacsave"),
                ConfigArtifactKind::Pacnew
            )
            .is_err()
        );
    }
}
