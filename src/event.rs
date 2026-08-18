use crate::domain::{
    ArchNewsItem, ConfigReview, FlightPlan, HealthReport, HistoryTransaction, HookDefinition,
    HygieneReport, ManifestDrift, Package, PackageUpdate, PkgbuildReview, RemovalPlan, Snapshot,
    TransactionEvent, TransactionRequest,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Installed,
    OfficialUpdates,
    AurUpdates,
    Search,
    Details,
    History,
    FlightPlan,
    Health,
    Transaction,
    RemovalPlan,
    PkgbuildReview,
    ConfigReview,
    Manifest,
    Hygiene,
    News,
    Snapshots,
    Hooks,
}

#[derive(Debug)]
pub enum AppMessage {
    Installed(Result<Vec<Package>, String>),
    OfficialUpdates(Result<Vec<PackageUpdate>, String>),
    AurUpdates(Result<Vec<PackageUpdate>, String>),
    Search {
        generation: u64,
        result: Result<Vec<Package>, String>,
    },
    Details(Box<Result<Package, String>>),
    History(Result<Vec<HistoryTransaction>, String>),
    FlightPlan {
        request: TransactionRequest,
        result: Box<Result<FlightPlan, String>>,
    },
    Health(Box<Result<HealthReport, String>>),
    TransactionPrepared {
        request: TransactionRequest,
        result: Result<Vec<String>, String>,
    },
    Transaction(TransactionEvent),
    RemovalPlan(Result<RemovalPlan, String>),
    PkgbuildReview(Box<Result<PkgbuildReview, String>>),
    ConfigReview(Box<Result<ConfigReview, String>>),
    ManifestExported(Result<String, String>),
    ManifestCompared(Box<Result<(String, ManifestDrift), String>>),
    Hygiene(Box<Result<HygieneReport, String>>),
    News(Result<Vec<ArchNewsItem>, String>),
    Snapshots(Result<Vec<Snapshot>, String>),
    Hooks(Result<Vec<HookDefinition>, String>),
}
