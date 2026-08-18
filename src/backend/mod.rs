mod aur;
mod command;
mod config_files;
mod demo;
mod health;
mod helper;
mod hygiene;
mod news;
mod pacman;
mod snapshot;
mod system;
mod transaction;

pub use aur::AurRpcBackend;
pub use command::{CommandFailure, CommandOutput, CommandRunner, CommandSpec};
pub use config_files::PacdiffBackend;
pub use demo::{
    DemoAurBackend, DemoHealthBackend, DemoHistoryBackend, DemoPackageBackend,
    DemoTransactionBackend, demo_system_profile,
};
pub use health::SystemHealthBackend;
pub use helper::{AurHelperBackend, HelperKind};
pub use hygiene::PackageHygieneBackend;
pub use news::ArchNewsBackend;
pub use pacman::PacmanBackend;
pub use snapshot::{SnapshotKind, SystemSnapshotBackend};
pub use system::probe_system;
pub use transaction::{PacmanTransactionBackend, TransactionCommand};

use crate::domain::{
    ArchNewsItem, ConfigArtifact, ConfigReview, FlightPlan, HealthReport, HistoryTransaction,
    HookDefinition, HygieneReport, Package, PackageUpdate, PkgbuildReview, RemovalPlan, Snapshot,
    TransactionControl, TransactionEvent, TransactionRequest, TransactionResult,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait PackageBackend: Send + Sync {
    async fn installed_packages(&self) -> Result<Vec<Package>>;
    async fn search_official(&self, query: &str) -> Result<Vec<Package>>;
    async fn package_details(&self, package: &Package) -> Result<Package>;
    async fn check_updates(&self) -> Result<Vec<PackageUpdate>>;
}

#[async_trait]
pub trait AurBackend: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<Package>>;
    async fn info(&self, package: &str) -> Result<Option<Package>>;
    async fn check_updates(&self) -> Result<Vec<PackageUpdate>>;
    async fn fetch_pkgbuild(&self, package: &str) -> Result<String>;

    async fn review_pkgbuild(&self, package: &Package) -> Result<PkgbuildReview> {
        let current = self.fetch_pkgbuild(&package.name).await?;
        Ok(crate::parser::review_pkgbuild(
            package.name.clone(),
            None,
            current,
        ))
    }

    async fn install(&self, _packages: &[String]) -> Result<TransactionResult> {
        bail!(
            "AUR metadata backends do not execute transactions; use the configured transaction backend"
        )
    }

    async fn remove(&self, _packages: &[String]) -> Result<TransactionResult> {
        bail!(
            "AUR metadata backends do not execute transactions; use the configured transaction backend"
        )
    }
}

#[async_trait]
pub trait HistoryBackend: Send + Sync {
    async fn transactions(&self) -> Result<Vec<HistoryTransaction>>;
}

#[async_trait]
pub trait FlightPlanBackend: Send + Sync {
    async fn build_flight_plan(
        &self,
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan>;

    async fn build_install_flight_plan(
        &self,
        targets: &[String],
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan>;
}

#[async_trait]
pub trait HealthBackend: Send + Sync {
    async fn check(&self) -> Result<HealthReport>;
}

#[async_trait]
pub trait TransactionBackend: Send + Sync {
    fn command_preview(&self, request: &TransactionRequest) -> Result<Vec<String>>;

    async fn execute(
        &self,
        request: TransactionRequest,
        events: tokio::sync::mpsc::UnboundedSender<TransactionEvent>,
        controls: tokio::sync::mpsc::UnboundedReceiver<TransactionControl>,
    ) -> Result<()>;
}

#[async_trait]
pub trait RemovalBackend: Send + Sync {
    async fn simulate_removal(
        &self,
        packages: &[String],
        remove_unused: bool,
    ) -> Result<RemovalPlan>;
}

#[async_trait]
pub trait ConfigBackend: Send + Sync {
    async fn review(&self, artifact: &ConfigArtifact) -> Result<ConfigReview>;
}

#[async_trait]
pub trait SnapshotBackend: Send + Sync {
    fn name(&self) -> Option<&'static str>;
    async fn available(&self) -> bool;
    async fn create_pre_transaction(&self, description: &str) -> Result<Snapshot>;
    async fn list(&self) -> Result<Vec<Snapshot>>;
}

#[async_trait]
pub trait HygieneBackend: Send + Sync {
    async fn inspect(&self, installed: &[Package]) -> Result<HygieneReport>;
}

#[async_trait]
pub trait NewsBackend: Send + Sync {
    async fn latest(&self) -> Result<Vec<ArchNewsItem>>;
}

#[async_trait]
pub trait HookBackend: Send + Sync {
    async fn hooks(&self) -> Result<Vec<HookDefinition>>;
}

#[derive(Clone)]
pub struct Services {
    pub packages: Arc<dyn PackageBackend>,
    pub aur: Arc<dyn AurBackend>,
    pub history: Arc<dyn HistoryBackend>,
    pub planner: Arc<dyn FlightPlanBackend>,
    pub health: Arc<dyn HealthBackend>,
    pub transactions: Arc<dyn TransactionBackend>,
    pub removal: Arc<dyn RemovalBackend>,
    pub config_files: Arc<dyn ConfigBackend>,
    pub snapshots: Arc<dyn SnapshotBackend>,
    pub hygiene: Arc<dyn HygieneBackend>,
    pub news: Arc<dyn NewsBackend>,
    pub hooks: Arc<dyn HookBackend>,
}
