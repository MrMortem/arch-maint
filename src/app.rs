use crate::{
    analysis::{analyze_transaction_failure, build_dependency_report},
    domain::{
        ArchNewsItem, ConfigArtifact, ConfigReview, DependencyReport, FlightPlan, HealthReport,
        HistoryTransaction, HookDefinition, HygieneReport, ManifestDrift, OutputStream, Package,
        PackageUpdate, PkgbuildReview, RecoveryReport, RemovalPlan, Snapshot, SystemProfile,
        TransactionControl, TransactionEvent, TransactionRequest, TransactionResult,
    },
    event::{AppMessage, TaskKind},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Updates,
    Packages,
    Aur,
    Config,
    Health,
    History,
}

impl Tab {
    pub const ALL: [Self; 6] = [
        Self::Updates,
        Self::Packages,
        Self::Aur,
        Self::Config,
        Self::Health,
        Self::History,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Updates => "Updates",
            Self::Packages => "Packages",
            Self::Aur => "AUR",
            Self::Config => "Config",
            Self::Health => "Health",
            Self::History => "History",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PackageFilter {
    #[default]
    All,
    Installed,
    Official,
    Aur,
}

impl PackageFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Installed => "installed",
            Self::Official => "official",
            Self::Aur => "foreign/AUR",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Installed,
            Self::Installed => Self::Official,
            Self::Official => Self::Aur,
            Self::Aur => Self::All,
        }
    }
}

#[derive(Debug)]
pub enum Intent {
    Quit,
    Refresh,
    Search {
        tab: Tab,
        query: String,
        generation: u64,
    },
    Inspect(Box<Package>),
    BuildFlightPlan,
    BuildSelectedUpdateFlightPlan(String),
    BuildInstallFlightPlan(Vec<String>),
    BuildAurFlightPlan(Vec<String>),
    PrepareTransaction(TransactionRequest),
    ExecuteTransaction(TransactionRequest),
    TransactionControl(TransactionControl),
    SimulateRemoval(Vec<String>),
    ReviewPkgbuild(Box<Package>),
    ReviewAurPackageName(String),
    ReviewConfig(ConfigArtifact),
    ExportManifest,
    CompareManifest,
    LaunchPacdiff,
    CopyText(String),
    LoadHygiene,
    LoadSnapshots,
    LoadHooks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceAction {
    Pacdiff,
}

impl MaintenanceAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pacdiff => "Launch privileged pacdiff reconciliation",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingTransaction {
    pub request: TransactionRequest,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionPhase {
    AcquiringPrivilege,
    Running,
    Finished,
    FailedToStart,
}

#[derive(Debug)]
pub struct TransactionView {
    pub request: TransactionRequest,
    pub command: Vec<String>,
    pub phase: TransactionPhase,
    pub stdout: String,
    pub stderr: String,
    pub result: Option<TransactionResult>,
    pub recovery: Option<RecoveryReport>,
    pub follow: bool,
    pub show_recovery: bool,
    pub scroll: u16,
    pub input: String,
    pub output: Vec<(OutputStream, String)>,
    pub show_summary: bool,
    pub search_input: Option<String>,
    pub search_query: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub snapshot_status: Option<String>,
}

#[derive(Debug)]
pub struct App {
    pub profile: SystemProfile,
    pub active_tab: Tab,
    pub input_mode: InputMode,
    pub input: String,
    pub search_query: String,
    pub installed: Vec<Package>,
    pub packages: Vec<Package>,
    pub aur_packages: Vec<Package>,
    pub official_updates: Vec<PackageUpdate>,
    pub aur_updates: Vec<PackageUpdate>,
    pub history: Vec<HistoryTransaction>,
    pub inspected: Option<Package>,
    pub inspector_scroll: u16,
    pub flight_plan: Option<FlightPlan>,
    pub flight_plan_request: Option<TransactionRequest>,
    pub flight_plan_scroll: u16,
    pub health_report: Option<HealthReport>,
    pub pending_transaction: Option<PendingTransaction>,
    pub pending_maintenance: Option<MaintenanceAction>,
    pub transaction: Option<TransactionView>,
    pub removal_plan: Option<RemovalPlan>,
    pub pkgbuild_review: Option<PkgbuildReview>,
    pub pkgbuild_review_scroll: u16,
    pub pkgbuild_show_diff: bool,
    pub dependency_report: Option<DependencyReport>,
    pub dependency_scroll: u16,
    pub dependency_depth: usize,
    pub config_review: Option<ConfigReview>,
    pub config_review_scroll: u16,
    pub config_review_mode: u8,
    pub manifest_drift: Option<(String, ManifestDrift)>,
    pub hygiene_report: Option<HygieneReport>,
    pub hygiene_scroll: u16,
    pub news: Vec<ArchNewsItem>,
    pub show_news: bool,
    pub news_scroll: u16,
    pub snapshots: Option<Vec<Snapshot>>,
    pub snapshots_scroll: u16,
    pub hooks: Option<Vec<HookDefinition>>,
    pub hooks_scroll: u16,
    pub package_filter: PackageFilter,
    pub selected: HashMap<Tab, usize>,
    pub loading: HashSet<TaskKind>,
    pub notices: VecDeque<String>,
    pub show_help: bool,
    pub demo: bool,
    pub snapshot_before_upgrade: bool,
    pub show_arch_news: bool,
    search_generation: u64,
}

impl App {
    pub fn new(profile: SystemProfile, demo: bool) -> Self {
        let mut loading = HashSet::new();
        if demo || profile.tools.pacman {
            loading.extend([
                TaskKind::Installed,
                TaskKind::OfficialUpdates,
                TaskKind::History,
                TaskKind::Health,
            ]);
        }
        if demo || profile.selected_aur_helper.is_some() {
            loading.insert(TaskKind::AurUpdates);
        }
        Self {
            profile,
            active_tab: Tab::Updates,
            input_mode: InputMode::Normal,
            input: String::new(),
            search_query: String::new(),
            installed: Vec::new(),
            packages: Vec::new(),
            aur_packages: Vec::new(),
            official_updates: Vec::new(),
            aur_updates: Vec::new(),
            history: Vec::new(),
            inspected: None,
            inspector_scroll: 0,
            flight_plan: None,
            flight_plan_request: None,
            flight_plan_scroll: 0,
            health_report: None,
            pending_transaction: None,
            pending_maintenance: None,
            transaction: None,
            removal_plan: None,
            pkgbuild_review: None,
            pkgbuild_review_scroll: 0,
            pkgbuild_show_diff: true,
            dependency_report: None,
            dependency_scroll: 0,
            dependency_depth: 5,
            config_review: None,
            config_review_scroll: 0,
            config_review_mode: 0,
            manifest_drift: None,
            hygiene_report: None,
            hygiene_scroll: 0,
            news: Vec::new(),
            show_news: false,
            news_scroll: 0,
            snapshots: None,
            snapshots_scroll: 0,
            hooks: None,
            hooks_scroll: 0,
            package_filter: PackageFilter::All,
            selected: HashMap::new(),
            loading,
            notices: VecDeque::new(),
            show_help: false,
            demo,
            snapshot_before_upgrade: false,
            show_arch_news: false,
            search_generation: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        if self.show_help {
            self.show_help = false;
            return Vec::new();
        }
        if self.pending_maintenance.is_some() {
            return self.handle_maintenance_confirmation(key);
        }
        if self.pending_transaction.is_some() {
            return self.handle_confirmation_key(key);
        }
        if self.transaction.is_some() {
            return self.handle_transaction_key(key);
        }
        if self.dependency_report.is_some() {
            return self.handle_dependency_key(key);
        }
        if self.config_review.is_some() {
            return self.handle_config_review_key(key);
        }
        if self.manifest_drift.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('m')) {
                self.manifest_drift = None;
            }
            return Vec::new();
        }
        if self.hygiene_report.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('c') => self.hygiene_report = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.hygiene_scroll = self.hygiene_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.hygiene_scroll = self.hygiene_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => self.hygiene_scroll = self.hygiene_scroll.saturating_add(10),
                KeyCode::PageUp => self.hygiene_scroll = self.hygiene_scroll.saturating_sub(10),
                KeyCode::Home => self.hygiene_scroll = 0,
                _ => {}
            }
            return Vec::new();
        }
        if self.show_news {
            match key.code {
                KeyCode::Esc | KeyCode::Char('n') => self.show_news = false,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.news_scroll = self.news_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.news_scroll = self.news_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => self.news_scroll = self.news_scroll.saturating_add(10),
                KeyCode::PageUp => self.news_scroll = self.news_scroll.saturating_sub(10),
                KeyCode::Home => self.news_scroll = 0,
                _ => {}
            }
            return Vec::new();
        }
        if self.snapshots.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('s') => self.snapshots = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.snapshots_scroll = self.snapshots_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.snapshots_scroll = self.snapshots_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => {
                    self.snapshots_scroll = self.snapshots_scroll.saturating_add(10)
                }
                KeyCode::PageUp => self.snapshots_scroll = self.snapshots_scroll.saturating_sub(10),
                KeyCode::Home => self.snapshots_scroll = 0,
                _ => {}
            }
            return Vec::new();
        }
        if self.hooks.is_some() {
            match key.code {
                KeyCode::Esc => self.hooks = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.hooks_scroll = self.hooks_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.hooks_scroll = self.hooks_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => self.hooks_scroll = self.hooks_scroll.saturating_add(10),
                KeyCode::PageUp => self.hooks_scroll = self.hooks_scroll.saturating_sub(10),
                KeyCode::Home => self.hooks_scroll = 0,
                _ => {}
            }
            return Vec::new();
        }
        if self.pkgbuild_review.is_some() {
            return self.handle_pkgbuild_review_key(key);
        }
        if self.removal_plan.is_some() {
            return self.handle_removal_plan_key(key);
        }
        if self.inspected.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.inspected = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.inspector_scroll = self.inspector_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.inspector_scroll = self.inspector_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => {
                    self.inspector_scroll = self.inspector_scroll.saturating_add(10)
                }
                KeyCode::PageUp => self.inspector_scroll = self.inspector_scroll.saturating_sub(10),
                KeyCode::Home => self.inspector_scroll = 0,
                _ => {}
            }
            return Vec::new();
        }
        match self.input_mode {
            InputMode::Search | InputMode::Command => self.handle_input_key(key),
            InputMode::Normal => self.handle_normal_key(key),
        }
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Char('y') => {
                if let Some(pending) = self.pending_transaction.take() {
                    let request = pending.request.clone();
                    match &request {
                        TransactionRequest::SystemUpgrade
                        | TransactionRequest::OfficialUpdate { .. }
                        | TransactionRequest::OfficialInstall { .. } => {
                            self.flight_plan = None;
                            self.flight_plan_request = None;
                        }
                        TransactionRequest::OfficialRemove { .. }
                        | TransactionRequest::AurRemove { .. } => {
                            self.removal_plan = None;
                        }
                        TransactionRequest::AurInstall { .. } | TransactionRequest::AurUpgrade => {
                            self.pkgbuild_review = None;
                        }
                    }
                    self.loading.insert(TaskKind::Transaction);
                    self.transaction = Some(TransactionView {
                        request: pending.request,
                        command: pending.command,
                        phase: TransactionPhase::AcquiringPrivilege,
                        stdout: String::new(),
                        stderr: String::new(),
                        result: None,
                        recovery: None,
                        follow: true,
                        show_recovery: false,
                        scroll: 0,
                        input: String::new(),
                        output: Vec::new(),
                        show_summary: false,
                        search_input: None,
                        search_query: None,
                        snapshot: None,
                        snapshot_status: None,
                    });
                    return vec![Intent::ExecuteTransaction(request)];
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => self.pending_transaction = None,
            _ => {}
        }
        Vec::new()
    }

    fn handle_maintenance_confirmation(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Char('y') => match self.pending_maintenance.take() {
                Some(MaintenanceAction::Pacdiff) => {
                    self.config_review = None;
                    return vec![Intent::LaunchPacdiff];
                }
                None => {}
            },
            KeyCode::Char('n') | KeyCode::Esc => self.pending_maintenance = None,
            _ => {}
        }
        Vec::new()
    }

    fn handle_transaction_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        let Some(transaction) = &mut self.transaction else {
            return Vec::new();
        };
        if let Some(input) = &mut transaction.search_input {
            match key.code {
                KeyCode::Esc => transaction.search_input = None,
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => {
                    let query = input.trim().to_owned();
                    transaction.search_input = None;
                    if !query.is_empty() {
                        transaction.show_summary = false;
                        transaction.scroll = transaction_search_offset(transaction, &query);
                        transaction.follow = false;
                        transaction.search_query = Some(query);
                    }
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                }
                _ => {}
            }
            return Vec::new();
        }
        if matches!(
            transaction.phase,
            TransactionPhase::Finished | TransactionPhase::FailedToStart
        ) {
            match key.code {
                KeyCode::Esc => self.transaction = None,
                KeyCode::Char('v') if transaction.recovery.is_some() => {
                    transaction.show_recovery = !transaction.show_recovery;
                }
                KeyCode::Char('o') => transaction.show_summary = !transaction.show_summary,
                KeyCode::Char('/') => transaction.search_input = Some(String::new()),
                KeyCode::Char('y') => {
                    if let Some(text) = transaction_copy_text(transaction) {
                        return vec![Intent::CopyText(text)];
                    }
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    transaction.follow = false;
                    transaction.scroll = transaction.scroll.saturating_add(1);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    transaction.follow = false;
                    transaction.scroll = transaction.scroll.saturating_sub(1);
                }
                KeyCode::End => transaction.follow = true,
                _ => {}
            }
            return Vec::new();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return vec![Intent::TransactionControl(TransactionControl::Cancel)];
        }
        match key.code {
            KeyCode::Backspace => {
                transaction.input.pop();
            }
            KeyCode::Enter => {
                let answer = format!("{}\n", transaction.input);
                transaction.input.clear();
                return vec![Intent::TransactionControl(TransactionControl::Input(
                    answer,
                ))];
            }
            KeyCode::Char('j') | KeyCode::Down => {
                transaction.follow = false;
                transaction.scroll = transaction.scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                transaction.follow = false;
                transaction.scroll = transaction.scroll.saturating_sub(1);
            }
            KeyCode::End => transaction.follow = true,
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                transaction.input.push(character);
            }
            _ => {}
        }
        Vec::new()
    }

    fn handle_removal_plan_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Esc => self.removal_plan = None,
            KeyCode::Char('x') => {
                if let Some(plan) = &self.removal_plan
                    && !plan.blocked
                {
                    return vec![Intent::PrepareTransaction(
                        TransactionRequest::OfficialRemove {
                            packages: plan.requested.clone(),
                            remove_unused: true,
                        },
                    )];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn handle_pkgbuild_review_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Esc => self.pkgbuild_review = None,
            KeyCode::Char('x') => {
                if let Some(review) = &self.pkgbuild_review {
                    let package = review.package.clone();
                    self.loading.insert(TaskKind::FlightPlan);
                    return vec![Intent::BuildAurFlightPlan(vec![package])];
                }
            }
            KeyCode::Char('v') => self.pkgbuild_show_diff = !self.pkgbuild_show_diff,
            KeyCode::Char('j') | KeyCode::Down => {
                self.pkgbuild_review_scroll = self.pkgbuild_review_scroll.saturating_add(1)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.pkgbuild_review_scroll = self.pkgbuild_review_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => {
                self.pkgbuild_review_scroll = self.pkgbuild_review_scroll.saturating_add(10)
            }
            KeyCode::PageUp => {
                self.pkgbuild_review_scroll = self.pkgbuild_review_scroll.saturating_sub(10)
            }
            KeyCode::Home => self.pkgbuild_review_scroll = 0,
            _ => {}
        }
        Vec::new()
    }

    fn handle_dependency_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('d') => self.dependency_report = None,
            KeyCode::Char('j') | KeyCode::Down => {
                self.dependency_scroll = self.dependency_scroll.saturating_add(1)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.dependency_scroll = self.dependency_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => self.dependency_scroll = self.dependency_scroll.saturating_add(10),
            KeyCode::PageUp => self.dependency_scroll = self.dependency_scroll.saturating_sub(10),
            KeyCode::Home => self.dependency_scroll = 0,
            _ => {}
        }
        Vec::new()
    }

    fn handle_config_review_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Esc => self.config_review = None,
            KeyCode::Char('p') => {
                if self.profile.tools.pacdiff {
                    self.pending_maintenance = Some(MaintenanceAction::Pacdiff);
                } else {
                    self.notice("pacdiff is not available on this system.".into());
                }
            }
            KeyCode::Char('v') => {
                self.config_review_mode = (self.config_review_mode + 1) % 3;
                self.config_review_scroll = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.config_review_scroll = self.config_review_scroll.saturating_add(1)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.config_review_scroll = self.config_review_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => {
                self.config_review_scroll = self.config_review_scroll.saturating_add(10)
            }
            KeyCode::PageUp => {
                self.config_review_scroll = self.config_review_scroll.saturating_sub(10)
            }
            KeyCode::Home => self.config_review_scroll = 0,
            _ => {}
        }
        Vec::new()
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input.clear();
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(character)
            }
            KeyCode::Enter => return self.submit_input(),
            _ => {}
        }
        Vec::new()
    }

    fn submit_input(&mut self) -> Vec<Intent> {
        let mode = self.input_mode;
        let value = self.input.trim().to_owned();
        self.input_mode = InputMode::Normal;
        self.input.clear();
        if value.is_empty() {
            return Vec::new();
        }
        match mode {
            InputMode::Search => {
                if !matches!(self.active_tab, Tab::Packages | Tab::Aur) {
                    self.active_tab = Tab::Packages;
                }
                self.search_query = value.clone();
                self.search_generation += 1;
                self.loading.insert(TaskKind::Search);
                vec![Intent::Search {
                    tab: self.active_tab,
                    query: value,
                    generation: self.search_generation,
                }]
            }
            InputMode::Command => self.execute_command(&value),
            InputMode::Normal => Vec::new(),
        }
    }

    fn execute_command(&mut self, command: &str) -> Vec<Intent> {
        match command.trim() {
            "q" | "quit" => vec![Intent::Quit],
            "refresh" | "reload" => vec![Intent::Refresh],
            "manifest-export" => {
                self.loading.insert(TaskKind::Manifest);
                vec![Intent::ExportManifest]
            }
            "manifest-compare" | "manifest-diff" => {
                self.loading.insert(TaskKind::Manifest);
                vec![Intent::CompareManifest]
            }
            "snapshots" => {
                self.loading.insert(TaskKind::Snapshots);
                vec![Intent::LoadSnapshots]
            }
            "hooks" => {
                self.loading.insert(TaskKind::Hooks);
                vec![Intent::LoadHooks]
            }
            "packages" => {
                self.active_tab = Tab::Packages;
                Vec::new()
            }
            "updates" => {
                self.active_tab = Tab::Updates;
                Vec::new()
            }
            "aur" => {
                self.active_tab = Tab::Aur;
                Vec::new()
            }
            "history" => {
                self.active_tab = Tab::History;
                Vec::new()
            }
            value => {
                self.notice(format!("Unknown command: {value}"));
                Vec::new()
            }
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Vec<Intent> {
        if self.flight_plan.is_some() {
            match key.code {
                KeyCode::Char('x') | KeyCode::Enter => {
                    if let Some(request) = self.flight_plan_request.clone() {
                        return vec![Intent::PrepareTransaction(request)];
                    }
                    self.notice("This plan has no associated transaction request; rebuild it before execution.".into());
                }
                KeyCode::Esc | KeyCode::Char('p') => {
                    self.flight_plan = None;
                    self.flight_plan_request = None;
                }
                KeyCode::Char('q') => return vec![Intent::Quit],
                KeyCode::Char('j') | KeyCode::Down => {
                    self.flight_plan_scroll = self.flight_plan_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.flight_plan_scroll = self.flight_plan_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => {
                    self.flight_plan_scroll = self.flight_plan_scroll.saturating_add(10)
                }
                KeyCode::PageUp => {
                    self.flight_plan_scroll = self.flight_plan_scroll.saturating_sub(10)
                }
                KeyCode::Home => self.flight_plan_scroll = 0,
                _ => {}
            }
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('q') => return vec![Intent::Quit],
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                self.input.clear();
            }
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.input.clear();
            }
            KeyCode::Char('R') | KeyCode::F(5) => return vec![Intent::Refresh],
            KeyCode::Char('p' | 'a') if self.active_tab == Tab::Updates => {
                self.loading.insert(TaskKind::FlightPlan);
                return vec![Intent::BuildFlightPlan];
            }
            KeyCode::Char('n') if self.active_tab == Tab::Updates && self.show_arch_news => {
                self.show_news = true;
                self.news_scroll = 0;
            }
            KeyCode::Char('u') if self.active_tab == Tab::Updates => {
                if let Some(update) = self.selected_aur_update() {
                    if self.profile.selected_aur_helper.is_none() && !self.demo {
                        self.notice(
                            "AUR updates require a detected/configured paru or yay backend.".into(),
                        );
                    } else {
                        let name = update.name.clone();
                        self.loading.insert(TaskKind::PkgbuildReview);
                        return vec![Intent::ReviewAurPackageName(name)];
                    }
                } else if let Some(update) = self.selected_official_update() {
                    let name = update.name.clone();
                    self.loading.insert(TaskKind::FlightPlan);
                    return vec![Intent::BuildSelectedUpdateFlightPlan(name)];
                } else {
                    self.notice("No update is selected.".into());
                }
            }
            KeyCode::Char('f') if self.active_tab == Tab::Packages => {
                self.package_filter = self.package_filter.next();
                self.set_selection(0);
            }
            KeyCode::Char('c') if self.active_tab == Tab::Packages => {
                self.loading.insert(TaskKind::Hygiene);
                return vec![Intent::LoadHygiene];
            }
            KeyCode::Char('r') if matches!(self.active_tab, Tab::Packages | Tab::Aur) => {
                let package = self
                    .selected_package()
                    .filter(|package| package.installed)
                    .map(|package| package.name.clone());
                if let Some(package) = package {
                    self.loading.insert(TaskKind::RemovalPlan);
                    return vec![Intent::SimulateRemoval(vec![package])];
                }
                self.notice(
                    "Select an installed package before requesting removal simulation.".into(),
                );
            }
            KeyCode::Char('d') if matches!(self.active_tab, Tab::Packages | Tab::Aur) => {
                let package = self
                    .selected_package()
                    .filter(|package| package.installed)
                    .map(|package| package.name.clone());
                if let Some(package) = package {
                    self.dependency_report =
                        build_dependency_report(&self.installed, &package, self.dependency_depth);
                    self.dependency_scroll = 0;
                    if self.dependency_report.is_none() {
                        self.notice(format!(
                            "Dependency metadata for {package} is not present in the installed package set."
                        ));
                    }
                } else {
                    self.notice("Select an installed package for dependency exploration.".into());
                }
            }
            KeyCode::Char('i') if matches!(self.active_tab, Tab::Packages | Tab::Aur) => {
                let package = self.selected_package().cloned();
                match package {
                    Some(package) if package.installed => {
                        self.notice(format!("{} is already installed.", package.name));
                    }
                    Some(package)
                        if matches!(package.source, crate::domain::PackageSource::Official(_)) =>
                    {
                        self.loading.insert(TaskKind::FlightPlan);
                        return vec![Intent::BuildInstallFlightPlan(vec![package.name])];
                    }
                    Some(package)
                        if matches!(
                            package.source,
                            crate::domain::PackageSource::Aur
                                | crate::domain::PackageSource::Foreign
                        ) =>
                    {
                        if self.profile.selected_aur_helper.is_none() && !self.demo {
                            self.notice(
                                "AUR installation requires a detected/configured paru or yay backend."
                                    .into(),
                            );
                        } else {
                            self.loading.insert(TaskKind::PkgbuildReview);
                            return vec![Intent::ReviewPkgbuild(Box::new(package))];
                        }
                    }
                    Some(package) => self.notice(format!(
                        "{} is not available through an executable package backend.",
                        package.name
                    )),
                    None => self.notice("Select a package before requesting installation.".into()),
                }
            }
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.previous_tab(),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => self.next_tab(),
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => self.previous_tab(),
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Home => self.set_selection(0),
            KeyCode::End => self.set_selection(self.item_count().saturating_sub(1)),
            KeyCode::Enter => {
                if self.active_tab == Tab::Updates {
                    if let Some(update) = self.selected_aur_update() {
                        if self.profile.selected_aur_helper.is_some() || self.demo {
                            let name = update.name.clone();
                            self.loading.insert(TaskKind::PkgbuildReview);
                            return vec![Intent::ReviewAurPackageName(name)];
                        }
                        self.notice(
                            "AUR updates require a detected/configured paru or yay backend.".into(),
                        );
                        return Vec::new();
                    }
                    if let Some(update) = self.selected_official_update() {
                        let name = update.name.clone();
                        self.loading.insert(TaskKind::FlightPlan);
                        return vec![Intent::BuildSelectedUpdateFlightPlan(name)];
                    }
                    self.notice("No update is selected.".into());
                    return Vec::new();
                }
                if self.active_tab == Tab::Config {
                    let artifact = self
                        .health_report
                        .as_ref()
                        .and_then(|report| report.config_artifacts.get(self.selected_index()))
                        .cloned();
                    if let Some(artifact) = artifact {
                        self.loading.insert(TaskKind::ConfigReview);
                        return vec![Intent::ReviewConfig(artifact)];
                    }
                }
                if let Some(package) = self.selected_package().cloned() {
                    self.loading.insert(TaskKind::Details);
                    return vec![Intent::Inspect(Box::new(package))];
                }
            }
            KeyCode::Esc => self.inspected = None,
            _ => {}
        }
        Vec::new()
    }

    pub fn apply(&mut self, message: AppMessage) {
        match message {
            AppMessage::Installed(result) => {
                self.loading.remove(&TaskKind::Installed);
                match result {
                    Ok(packages) => {
                        self.installed = packages;
                        if self.packages.is_empty() {
                            self.packages = self.installed.clone();
                        }
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::OfficialUpdates(result) => {
                self.loading.remove(&TaskKind::OfficialUpdates);
                match result {
                    Ok(updates) => self.official_updates = updates,
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::AurUpdates(result) => {
                self.loading.remove(&TaskKind::AurUpdates);
                match result {
                    Ok(updates) => self.aur_updates = updates,
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::Search { generation, result } => {
                if generation != self.search_generation {
                    return;
                }
                self.loading.remove(&TaskKind::Search);
                match result {
                    Ok(mut packages) => {
                        mark_installed(&mut packages, &self.installed);
                        if self.active_tab == Tab::Aur {
                            self.aur_packages = packages;
                        } else {
                            self.packages = packages;
                        }
                        self.set_selection(0);
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::Details(result) => {
                self.loading.remove(&TaskKind::Details);
                match *result {
                    Ok(mut package) => {
                        mark_installed(std::slice::from_mut(&mut package), &self.installed);
                        self.inspected = Some(package);
                        self.inspector_scroll = 0;
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::History(result) => {
                self.loading.remove(&TaskKind::History);
                match result {
                    Ok(history) => self.history = history,
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::FlightPlan { request, result } => {
                self.loading.remove(&TaskKind::FlightPlan);
                match *result {
                    Ok(plan) => {
                        if matches!(&request, TransactionRequest::AurInstall { .. }) {
                            self.pkgbuild_review = None;
                        }
                        self.flight_plan = Some(plan);
                        self.flight_plan_request = Some(request);
                        self.flight_plan_scroll = 0;
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::Health(result) => {
                self.loading.remove(&TaskKind::Health);
                match *result {
                    Ok(report) => self.health_report = Some(report),
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::TransactionPrepared { request, result } => match result {
                Ok(command) => {
                    self.pending_transaction = Some(PendingTransaction { request, command });
                }
                Err(error) => self.notice(error),
            },
            AppMessage::Transaction(event) => self.apply_transaction_event(event),
            AppMessage::RemovalPlan(result) => {
                self.loading.remove(&TaskKind::RemovalPlan);
                match result {
                    Ok(plan) => self.removal_plan = Some(plan),
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::PkgbuildReview(result) => {
                self.loading.remove(&TaskKind::PkgbuildReview);
                match *result {
                    Ok(review) => {
                        self.pkgbuild_review = Some(review);
                        self.pkgbuild_review_scroll = 0;
                        self.pkgbuild_show_diff = true;
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::ConfigReview(result) => {
                self.loading.remove(&TaskKind::ConfigReview);
                match *result {
                    Ok(review) => {
                        self.config_review = Some(review);
                        self.config_review_scroll = 0;
                        self.config_review_mode = 0;
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::ManifestExported(result) => {
                self.loading.remove(&TaskKind::Manifest);
                match result {
                    Ok(path) => self.notice(format!("Package manifest exported to {path}")),
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::ManifestCompared(result) => {
                self.loading.remove(&TaskKind::Manifest);
                match *result {
                    Ok(value) => self.manifest_drift = Some(value),
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::Hygiene(result) => {
                self.loading.remove(&TaskKind::Hygiene);
                match *result {
                    Ok(report) => {
                        self.hygiene_report = Some(report);
                        self.hygiene_scroll = 0;
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::News(result) => {
                self.loading.remove(&TaskKind::News);
                match result {
                    Ok(news) => self.news = news,
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::Snapshots(result) => {
                self.loading.remove(&TaskKind::Snapshots);
                match result {
                    Ok(snapshots) => {
                        self.snapshots = Some(snapshots);
                        self.snapshots_scroll = 0;
                    }
                    Err(error) => self.notice(error),
                }
            }
            AppMessage::Hooks(result) => {
                self.loading.remove(&TaskKind::Hooks);
                match result {
                    Ok(hooks) => {
                        self.hooks = Some(hooks);
                        self.hooks_scroll = 0;
                    }
                    Err(error) => self.notice(error),
                }
            }
        }
    }

    fn apply_transaction_event(&mut self, event: TransactionEvent) {
        let Some(transaction) = &mut self.transaction else {
            return;
        };
        match event {
            TransactionEvent::SnapshotStarted { backend } => {
                transaction.snapshot_status =
                    Some(format!("Creating pre-transaction snapshot with {backend}…"));
            }
            TransactionEvent::SnapshotCreated(snapshot) => {
                transaction.snapshot_status = Some(format!(
                    "Created {} snapshot{}",
                    snapshot.backend,
                    snapshot
                        .id
                        .as_deref()
                        .map(|id| format!(" {id}"))
                        .unwrap_or_else(|| " (identifier unavailable)".into())
                ));
                transaction.snapshot = Some(snapshot);
            }
            TransactionEvent::Started { command } => {
                transaction.command = command;
                transaction.phase = TransactionPhase::Running;
            }
            TransactionEvent::Output { stream, chunk } => {
                match stream {
                    OutputStream::Stdout => transaction.stdout.push_str(&chunk),
                    OutputStream::Stderr => transaction.stderr.push_str(&chunk),
                }
                transaction.output.push((stream, chunk));
            }
            TransactionEvent::Finished(result) => {
                self.loading.remove(&TaskKind::Transaction);
                let result = *result;
                let succeeded = result.exit_code == Some(0) && !result.cancelled;
                if !succeeded {
                    transaction.recovery = Some(analyze_transaction_failure(
                        &result,
                        std::path::Path::new("/var/lib/pacman/db.lck").exists(),
                    ));
                }
                transaction.result = Some(result);
                transaction.phase = TransactionPhase::Finished;
                transaction.follow = true;
            }
            TransactionEvent::FailedToStart(error) => {
                self.loading.remove(&TaskKind::Transaction);
                transaction.stderr.push_str(&error);
                transaction.output.push((OutputStream::Stderr, error));
                transaction.phase = TransactionPhase::FailedToStart;
            }
        }
    }

    pub fn begin_refresh(&mut self) {
        self.flight_plan = None;
        self.flight_plan_request = None;
        self.flight_plan_scroll = 0;
        self.loading.extend([
            TaskKind::Installed,
            TaskKind::OfficialUpdates,
            TaskKind::History,
            TaskKind::Health,
        ]);
        if self.profile.selected_aur_helper.is_some() || self.demo {
            self.loading.insert(TaskKind::AurUpdates);
        }
        if self.show_arch_news {
            self.loading.insert(TaskKind::News);
        }
        self.notice("Refreshing read-only system data…".into());
    }

    pub fn filtered_packages(&self, aur_only: bool) -> Vec<&Package> {
        let source = if aur_only {
            &self.aur_packages
        } else {
            &self.packages
        };
        let candidates = source.iter().filter(|package| {
            aur_only
                || match self.package_filter {
                    PackageFilter::All => true,
                    PackageFilter::Installed => package.installed,
                    PackageFilter::Official => {
                        matches!(package.source, crate::domain::PackageSource::Official(_))
                    }
                    PackageFilter::Aur => {
                        matches!(
                            package.source,
                            crate::domain::PackageSource::Aur
                                | crate::domain::PackageSource::Foreign
                        )
                    }
                }
        });
        if self.search_query.is_empty() {
            return candidates.collect();
        }
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<_> = candidates
            .filter_map(|package| {
                let name_score = matcher.fuzzy_match(&package.name, &self.search_query);
                let description_score = package
                    .description
                    .as_deref()
                    .and_then(|description| matcher.fuzzy_match(description, &self.search_query));
                name_score
                    .max(description_score)
                    .map(|score| (score, package))
            })
            .collect();
        scored.sort_by_key(|(score, _)| -score);
        scored.into_iter().map(|(_, package)| package).collect()
    }

    pub fn selected_index(&self) -> usize {
        self.selected.get(&self.active_tab).copied().unwrap_or(0)
    }

    pub fn latest_notice(&self) -> Option<&str> {
        self.notices.back().map(String::as_str)
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        self.notice(message.into());
    }

    fn notice(&mut self, message: String) {
        tracing::warn!(message = %message, "application notice");
        if self.notices.len() == 20 {
            self.notices.pop_front();
        }
        self.notices.push_back(message);
    }

    fn next_tab(&mut self) {
        let index = Tab::ALL
            .iter()
            .position(|tab| *tab == self.active_tab)
            .unwrap_or(0);
        self.active_tab = Tab::ALL[(index + 1) % Tab::ALL.len()];
        self.inspected = None;
    }

    fn previous_tab(&mut self) {
        let index = Tab::ALL
            .iter()
            .position(|tab| *tab == self.active_tab)
            .unwrap_or(0);
        self.active_tab = Tab::ALL[(index + Tab::ALL.len() - 1) % Tab::ALL.len()];
        self.inspected = None;
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        let current = self.selected_index();
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            (current + delta as usize).min(count - 1)
        };
        self.set_selection(next);
    }

    fn set_selection(&mut self, index: usize) {
        self.selected.insert(self.active_tab, index);
    }

    fn item_count(&self) -> usize {
        match self.active_tab {
            Tab::Updates => self.official_updates.len() + self.aur_updates.len(),
            Tab::Packages => self.filtered_packages(false).len(),
            Tab::Aur => self.filtered_packages(true).len(),
            Tab::Config => self
                .health_report
                .as_ref()
                .map_or(0, |report| report.config_artifacts.len()),
            Tab::Health => self
                .health_report
                .as_ref()
                .map_or(0, |report| report.findings.len()),
            Tab::History => self.history.len(),
        }
    }

    fn selected_package(&self) -> Option<&Package> {
        match self.active_tab {
            Tab::Packages => self
                .filtered_packages(false)
                .get(self.selected_index())
                .copied(),
            Tab::Aur => self
                .filtered_packages(true)
                .get(self.selected_index())
                .copied(),
            _ => None,
        }
    }

    fn selected_aur_update(&self) -> Option<&PackageUpdate> {
        let index = self
            .selected_index()
            .checked_sub(self.official_updates.len())?;
        self.aur_updates.get(index)
    }

    fn selected_official_update(&self) -> Option<&PackageUpdate> {
        self.official_updates.get(self.selected_index())
    }
}

fn mark_installed(packages: &mut [Package], installed: &[Package]) {
    let known: HashMap<_, _> = installed
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect();
    for package in packages {
        if let Some(local) = known.get(package.name.as_str()) {
            package.installed = true;
            package.install_reason = local.install_reason;
            package.install_date.clone_from(&local.install_date);
            package.installed_size = local.installed_size;
        }
    }
}

fn transaction_search_offset(transaction: &TransactionView, query: &str) -> u16 {
    let query = query.to_ascii_lowercase();
    transaction
        .output
        .iter()
        .flat_map(|(_, chunk)| chunk.lines())
        .position(|line| line.to_ascii_lowercase().contains(&query))
        .map(|line| line.saturating_add(3).min(u16::MAX as usize) as u16)
        .unwrap_or(0)
}

fn transaction_copy_text(transaction: &TransactionView) -> Option<String> {
    if let Some(query) = &transaction.search_query
        && let Some(line) = transaction
            .output
            .iter()
            .flat_map(|(_, chunk)| chunk.lines())
            .find(|line| {
                line.to_ascii_lowercase()
                    .contains(&query.to_ascii_lowercase())
            })
    {
        return Some(line.to_owned());
    }
    transaction
        .recovery
        .as_ref()
        .filter(|recovery| !recovery.relevant_errors.is_empty())
        .map(|recovery| recovery.relevant_errors.join("\n"))
        .or_else(|| {
            let stderr = transaction.stderr.trim();
            (!stderr.is_empty()).then(|| stderr.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::demo_system_profile;

    #[test]
    fn confirmation_is_required_before_transaction_intent() {
        let mut app = App::new(demo_system_profile(), true);
        let request = TransactionRequest::SystemUpgrade;
        app.apply(AppMessage::TransactionPrepared {
            request: request.clone(),
            result: Ok(vec!["demo-transaction".into(), "upgrade".into()]),
        });
        assert!(app.pending_transaction.is_some());
        let intents = app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(matches!(
            intents.as_slice(),
            [Intent::ExecuteTransaction(
                TransactionRequest::SystemUpgrade
            )]
        ));
        assert!(app.pending_transaction.is_none());
        assert!(matches!(
            app.transaction.as_ref().map(|view| view.phase),
            Some(TransactionPhase::AcquiringPrivilege)
        ));
    }

    #[test]
    fn failed_transaction_builds_recovery_state() {
        let mut app = App::new(demo_system_profile(), true);
        app.transaction = Some(TransactionView {
            request: TransactionRequest::SystemUpgrade,
            command: vec!["demo".into()],
            phase: TransactionPhase::Running,
            stdout: String::new(),
            stderr: String::new(),
            result: None,
            recovery: None,
            follow: true,
            show_recovery: false,
            scroll: 0,
            input: String::new(),
            output: Vec::new(),
            show_summary: false,
            search_input: None,
            search_query: None,
            snapshot: None,
            snapshot_status: None,
        });
        app.apply(AppMessage::Transaction(TransactionEvent::Finished(
            Box::new(TransactionResult {
                command: vec!["demo".into()],
                exit_code: Some(1),
                stdout: "upgrading linux\n".into(),
                stderr: "error: DKMS build failed\n".into(),
                cancelled: false,
                hooks: Vec::new(),
            }),
        )));
        let transaction = app.transaction.as_ref().expect("transaction view");
        assert_eq!(transaction.phase, TransactionPhase::Finished);
        assert!(transaction.recovery.is_some());
    }

    #[test]
    fn pkgbuild_approval_requires_full_upgrade_plan_before_execution() {
        let mut app = App::new(demo_system_profile(), true);
        app.pkgbuild_review = Some(PkgbuildReview {
            package: "example-bin".into(),
            baseline_source: None,
            current_pkgbuild: "pkgname=example-bin\n".into(),
            unified_diff: String::new(),
            findings: Vec::new(),
            related_files: Vec::new(),
            evidence_notes: Vec::new(),
        });
        let intents = app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(matches!(
            intents.as_slice(),
            [Intent::BuildAurFlightPlan(packages)] if packages == &["example-bin"]
        ));
        assert!(app.pending_transaction.is_none());
    }

    #[test]
    fn enter_continues_from_flight_plan_to_explicit_confirmation() {
        let mut app = App::new(demo_system_profile(), true);
        app.flight_plan = Some(crate::domain::FlightPlan {
            generated_at: chrono::Utc::now(),
            packages: Vec::new(),
            download_size: None,
            installed_size_delta: None,
            attention: Vec::new(),
            expected_hooks: Vec::new(),
            aur_rebuild_candidates: Vec::new(),
            separate_aur_updates: Vec::new(),
            policy: Default::default(),
            evidence_notes: Vec::new(),
        });
        app.flight_plan_request = Some(TransactionRequest::SystemUpgrade);
        let intents = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            intents.as_slice(),
            [Intent::PrepareTransaction(
                TransactionRequest::SystemUpgrade
            )]
        ));
    }

    #[test]
    fn update_controls_distinguish_selected_from_full_system() {
        let mut app = App::new(demo_system_profile(), true);
        app.official_updates = crate::domain::UpdateSet::demo().official;

        let selected = app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
        assert!(matches!(
            selected.as_slice(),
            [Intent::BuildSelectedUpdateFlightPlan(package)] if package == "linux"
        ));

        let full = app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(matches!(full.as_slice(), [Intent::BuildFlightPlan]));
    }
}
