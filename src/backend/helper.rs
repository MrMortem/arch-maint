use super::{CommandRunner, CommandSpec};
use crate::{
    domain::{PackageSource, PackageUpdate},
    parser::parse_updates,
};
use anyhow::{Context, Result, bail};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperKind {
    Paru,
    Yay,
}

impl HelperKind {
    pub fn command(self) -> &'static str {
        match self {
            Self::Paru => "paru",
            Self::Yay => "yay",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AurHelperBackend {
    kind: HelperKind,
    runner: CommandRunner,
}

impl AurHelperBackend {
    pub fn new(kind: HelperKind) -> Self {
        Self {
            kind,
            runner: CommandRunner,
        }
    }

    pub fn kind(&self) -> HelperKind {
        self.kind
    }

    pub async fn check_updates(&self) -> Result<Vec<PackageUpdate>> {
        let spec = CommandSpec::read_only(self.kind.command(), ["-Qua", "--color", "never"])
            .with_timeout(Duration::from_secs(120));
        let output = self
            .runner
            .run(spec)
            .await
            .context("AUR update check failed")?;
        if !output.status.success() {
            bail!(
                "{} update check failed: {}",
                self.kind.command(),
                output.stderr.trim()
            );
        }
        Ok(parse_updates(&output.stdout, PackageSource::Aur))
    }
}
