use anyhow::{Context, Result, bail};
use arch_maint::{
    app::{App, Intent, Tab},
    backend::{
        ArchNewsBackend, AurHelperBackend, AurRpcBackend, DemoAurBackend, DemoHealthBackend,
        DemoHistoryBackend, DemoPackageBackend, DemoTransactionBackend, HelperKind, PacdiffBackend,
        PackageHygieneBackend, PacmanBackend, PacmanTransactionBackend, Services, SnapshotKind,
        SystemHealthBackend, SystemSnapshotBackend, demo_system_profile, probe_system,
    },
    config::{AurHelperPreference, Config, state_dir},
    event::{AppMessage, TaskKind},
    ui,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    fs::OpenOptions,
    io::{self, Stdout},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Arch Linux system maintenance and package transaction TUI"
)]
struct Cli {
    /// Use deterministic sample data; works on non-Arch systems.
    #[arg(long)]
    demo: bool,

    /// Read configuration from this path.
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let (_log_guard, log_path) = init_logging()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), log = %log_path.display(), "starting arch-maint");
    let (config, config_path) = Config::load(cli.config)?;
    tracing::debug!(path = %config_path.display(), "configuration loaded");

    let mut profile = if cli.demo {
        demo_system_profile()
    } else {
        probe_system().await
    };
    select_helper(&mut profile, config.aur_helper);
    if profile.running_as_root && !cli.demo {
        bail!(
            "arch-maint must run as a regular user; privilege escalation belongs only to explicit transactions"
        )
    }

    let services = build_services(&profile, &config, cli.demo)?;
    let mut app = App::new(profile, cli.demo);
    app.dependency_depth = config.dependency_depth;
    app.snapshot_before_upgrade = config.snapshot_before_upgrade
        && (app.profile.tools.snapper || app.profile.tools.timeshift);
    if config.snapshot_before_upgrade && !app.snapshot_before_upgrade {
        tracing::warn!("snapshot_before_upgrade requested, but no supported backend was detected");
    }
    app.show_arch_news = config.show_arch_news;
    if app.show_arch_news {
        app.loading.insert(TaskKind::News);
    }
    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;
    let result = run_app(&mut terminal, app, services).await;
    terminal.show_cursor().ok();
    result
}

fn init_logging() -> Result<(tracing_appender::non_blocking::WorkerGuard, PathBuf)> {
    let directory = state_dir();
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create log directory {}", directory.display()))?;
    let log_path = directory.join("arch-maint.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open log file {}", log_path.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(log_file);
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("arch_maint=info")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer),
        )
        .try_init()
        .context("failed to initialize logging")?;
    Ok((guard, log_path))
}

fn select_helper(profile: &mut arch_maint::domain::SystemProfile, preference: AurHelperPreference) {
    profile.selected_aur_helper = match preference {
        AurHelperPreference::Auto if profile.tools.paru => Some("paru".into()),
        AurHelperPreference::Auto if profile.tools.yay => Some("yay".into()),
        AurHelperPreference::Paru if profile.tools.paru => Some("paru".into()),
        AurHelperPreference::Yay if profile.tools.yay => Some("yay".into()),
        AurHelperPreference::None
        | AurHelperPreference::Auto
        | AurHelperPreference::Paru
        | AurHelperPreference::Yay => None,
    };
}

fn build_services(
    profile: &arch_maint::domain::SystemProfile,
    config: &Config,
    demo: bool,
) -> Result<Services> {
    if demo {
        return Ok(Services {
            packages: Arc::new(DemoPackageBackend),
            aur: Arc::new(DemoAurBackend),
            history: Arc::new(DemoHistoryBackend),
            planner: Arc::new(DemoPackageBackend),
            health: Arc::new(DemoHealthBackend),
            transactions: Arc::new(DemoTransactionBackend),
            removal: Arc::new(DemoPackageBackend),
            config_files: Arc::new(DemoHealthBackend),
            snapshots: Arc::new(SystemSnapshotBackend::new(SnapshotKind::Disabled)),
            hygiene: Arc::new(DemoPackageBackend),
            news: Arc::new(DemoAurBackend),
            hooks: Arc::new(DemoPackageBackend),
        });
    }
    let pacman = PacmanBackend::new(profile.tools.checkupdates);
    let helper = match profile.selected_aur_helper.as_deref() {
        Some("paru") => Some(AurHelperBackend::new(HelperKind::Paru)),
        Some("yay") => Some(AurHelperBackend::new(HelperKind::Yay)),
        _ => None,
    };
    let helper_kind = helper.as_ref().map(AurHelperBackend::kind);
    let snapshot_kind = if profile.tools.snapper {
        SnapshotKind::Snapper
    } else if profile.tools.timeshift {
        SnapshotKind::Timeshift
    } else {
        SnapshotKind::Disabled
    };
    Ok(Services {
        packages: Arc::new(pacman.clone()),
        aur: Arc::new(AurRpcBackend::new(config.aur_rpc_url.clone(), helper)?),
        history: Arc::new(pacman),
        planner: Arc::new(PacmanBackend::new(profile.tools.checkupdates)),
        health: Arc::new(SystemHealthBackend::default()),
        transactions: Arc::new(PacmanTransactionBackend::new(helper_kind)),
        removal: Arc::new(PacmanBackend::new(profile.tools.checkupdates)),
        config_files: Arc::new(PacdiffBackend),
        snapshots: Arc::new(SystemSnapshotBackend::new(snapshot_kind)),
        hygiene: Arc::new(PackageHygieneBackend::default()),
        news: Arc::new(ArchNewsBackend::new()?),
        hooks: Arc::new(PacmanBackend::new(profile.tools.checkupdates)),
    })
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        disable_raw_mode().ok();
        return Err(error).context("failed to enter alternate screen");
    }
    Terminal::new(CrosstermBackend::new(stdout)).context("failed to initialize terminal")
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        disable_raw_mode().ok();
        execute!(io::stdout(), LeaveAlternateScreen).ok();
    }
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut app: App,
    services: Services,
) -> Result<()> {
    let (input_tx, mut input_rx) = mpsc::unbounded_channel();
    let input_running = Arc::new(AtomicBool::new(true));
    let input_suspended = Arc::new(AtomicBool::new(false));
    spawn_input_thread(input_tx, input_running.clone(), input_suspended.clone());
    let (message_tx, mut message_rx) = mpsc::unbounded_channel();
    refresh(&app, &services, &message_tx);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut running = true;
    let mut transaction_controls: Option<
        mpsc::UnboundedSender<arch_maint::domain::TransactionControl>,
    > = None;

    while running {
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .context("terminal draw failed")?;
        tokio::select! {
            _ = tick.tick() => {}
            Some(key) = input_rx.recv() => {
                for intent in app.handle_key(key) {
                    match intent {
                        Intent::Quit => running = false,
                        Intent::Refresh => {
                            app.begin_refresh();
                            refresh(&app, &services, &message_tx);
                        }
                        Intent::Search { tab, query, generation } => search(tab, query, generation, &services, &message_tx),
                        Intent::Inspect(package) => inspect(*package, &services, &message_tx),
                        Intent::BuildFlightPlan => build_flight_plan(&app, &services, &message_tx),
                        Intent::BuildSelectedUpdateFlightPlan(package) => {
                            build_selected_update_flight_plan(package, &app, &services, &message_tx)
                        }
                        Intent::BuildInstallFlightPlan(packages) => {
                            build_install_flight_plan(packages, &app, &services, &message_tx)
                        }
                        Intent::BuildAurFlightPlan(packages) => {
                            build_aur_flight_plan(packages, &app, &services, &message_tx)
                        }
                        Intent::PrepareTransaction(request) => {
                            tracing::info!(operation = request.label(), "preparing reviewed transaction");
                            let result = services
                                .transactions
                                .command_preview(&request)
                                .map_err(|error| format!("Could not prepare transaction: {error:#}"));
                            match &result {
                                Ok(command) => tracing::info!(
                                    operation = request.label(),
                                    command = ?command,
                                    "transaction awaiting explicit confirmation"
                                ),
                                Err(error) => tracing::error!(
                                    operation = request.label(),
                                    %error,
                                    "transaction preparation failed"
                                ),
                            }
                            app.apply(AppMessage::TransactionPrepared { request, result });
                        }
                        Intent::ExecuteTransaction(request) => {
                            tracing::info!(operation = request.label(), "confirmed transaction requesting scoped privilege");
                            let privilege = if app.demo || !request.requires_privilege() {
                                Ok(())
                            } else {
                                acquire_privileges(
                                    terminal,
                                    request.label(),
                                    input_suspended.clone(),
                                )
                                .await
                            };
                            match privilege {
                                Ok(()) => {
                                    transaction_controls = Some(start_transaction(
                                        request,
                                        &services,
                                        &message_tx,
                                        app.snapshot_before_upgrade,
                                    ));
                                }
                                Err(error) => app.apply(AppMessage::Transaction(
                                    {
                                        tracing::error!(
                                            operation = request.label(),
                                            error = %format!("{error:#}"),
                                            "privilege acquisition failed"
                                        );
                                        arch_maint::domain::TransactionEvent::FailedToStart(
                                            format!("Privilege acquisition failed: {error:#}"),
                                        )
                                    },
                                )),
                            }
                        }
                        Intent::TransactionControl(control) => {
                            if let Some(sender) = &transaction_controls {
                                sender.send(control).ok();
                            }
                        }
                        Intent::SimulateRemoval(packages) => {
                            simulate_removal(packages, &services, &message_tx);
                        }
                        Intent::ReviewPkgbuild(package) => {
                            review_pkgbuild(*package, &services, &message_tx);
                        }
                        Intent::ReviewAurPackageName(package) => {
                            review_aur_package_name(package, &services, &message_tx);
                        }
                        Intent::ReviewConfig(artifact) => {
                            review_config(artifact, &services, &message_tx);
                        }
                        Intent::ExportManifest => export_manifest(&app, &message_tx),
                        Intent::CompareManifest => compare_manifest(&app, &message_tx),
                        Intent::LaunchPacdiff => {
                            match launch_pacdiff(terminal, input_suspended.clone()).await {
                                Ok(()) => app.notify("pacdiff exited; refreshing configuration and health state."),
                                Err(error) => app.notify(format!("pacdiff: {error:#}")),
                            }
                            app.begin_refresh();
                            refresh(&app, &services, &message_tx);
                        }
                        Intent::CopyText(value) => {
                            copy_to_terminal_clipboard(&value)?;
                            app.notify("Copied selected transaction evidence through OSC 52.");
                        }
                        Intent::LoadHygiene => load_hygiene(&app, &services, &message_tx),
                        Intent::LoadSnapshots => load_snapshots(&services, &message_tx),
                        Intent::LoadHooks => load_hooks(&services, &message_tx),
                    }
                }
            }
            Some(message) = message_rx.recv() => {
                match &message {
                    AppMessage::Transaction(arch_maint::domain::TransactionEvent::Started { command }) => {
                        tracing::info!(command = ?command, "package transaction started");
                    }
                    AppMessage::Transaction(arch_maint::domain::TransactionEvent::Finished(result)) => {
                        tracing::info!(
                            exit_code = ?result.exit_code,
                            cancelled = result.cancelled,
                            "package transaction finished"
                        );
                    }
                    AppMessage::Transaction(arch_maint::domain::TransactionEvent::FailedToStart(error)) => {
                        tracing::error!(%error, "package transaction failed to start");
                    }
                    _ => {}
                }
                let transaction_finished = matches!(
                    &message,
                    AppMessage::Transaction(
                        arch_maint::domain::TransactionEvent::Finished(_)
                            | arch_maint::domain::TransactionEvent::FailedToStart(_)
                    )
                );
                app.apply(message);
                if transaction_finished {
                    transaction_controls = None;
                    app.begin_refresh();
                    refresh(&app, &services, &message_tx);
                }
            },
        }
    }
    input_running.store(false, Ordering::Relaxed);
    Ok(())
}

fn copy_to_terminal_clipboard(value: &str) -> Result<()> {
    use std::io::Write;
    const MAX_BYTES: usize = 16 * 1024;
    let bytes = value.as_bytes();
    let bytes = &bytes[..bytes.len().min(MAX_BYTES)];
    let encoded = STANDARD.encode(bytes);
    write!(io::stdout(), "\x1b]52;c;{encoded}\x07")
        .context("failed to write OSC 52 clipboard sequence")?;
    io::stdout()
        .flush()
        .context("failed to flush clipboard sequence")
}

async fn acquire_privileges(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    operation: &str,
    input_suspended: Arc<AtomicBool>,
) -> Result<()> {
    let _input_guard = InputSuspendGuard::new(input_suspended);
    tokio::time::sleep(Duration::from_millis(120)).await;
    terminal.show_cursor().ok();
    disable_raw_mode().context("failed to suspend terminal raw mode")?;
    execute!(io::stdout(), LeaveAlternateScreen)
        .context("failed to suspend the terminal interface")?;
    println!("arch-maint needs temporary administrator credentials for: {operation}");
    println!("The TUI remains unprivileged; only the package-manager child will use sudo.");
    let status = tokio::process::Command::new("sudo")
        .arg("-v")
        .status()
        .await
        .context("could not run sudo -v");
    let restore_screen = execute!(io::stdout(), EnterAlternateScreen)
        .context("failed to restore the terminal interface");
    let restore_raw = enable_raw_mode().context("failed to restore terminal raw mode");
    terminal.clear().ok();
    restore_screen?;
    restore_raw?;
    let status = status?;
    if !status.success() {
        bail!("sudo credential validation exited with {status}");
    }
    Ok(())
}

async fn launch_pacdiff(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    input_suspended: Arc<AtomicBool>,
) -> Result<()> {
    let _input_guard = InputSuspendGuard::new(input_suspended);
    tokio::time::sleep(Duration::from_millis(120)).await;
    terminal.show_cursor().ok();
    disable_raw_mode().context("failed to suspend terminal raw mode")?;
    execute!(io::stdout(), LeaveAlternateScreen)
        .context("failed to suspend the terminal interface")?;
    println!("arch-maint is launching pacdiff with scoped administrator privileges.");
    println!("pacdiff is interactive: review every proposed keep/replace/merge action carefully.");
    let status = tokio::process::Command::new("sudo")
        .args(["--", "pacdiff"])
        .status()
        .await
        .context("could not start sudo pacdiff");
    let restore_screen = execute!(io::stdout(), EnterAlternateScreen)
        .context("failed to restore the terminal interface");
    let restore_raw = enable_raw_mode().context("failed to restore terminal raw mode");
    terminal.clear().ok();
    restore_screen?;
    restore_raw?;
    let status = status?;
    if !status.success() {
        bail!("pacdiff exited with {status}");
    }
    Ok(())
}

fn start_transaction(
    request: arch_maint::domain::TransactionRequest,
    services: &Services,
    app_messages: &mpsc::UnboundedSender<AppMessage>,
    snapshot_before_upgrade: bool,
) -> mpsc::UnboundedSender<arch_maint::domain::TransactionControl> {
    let backend = services.transactions.clone();
    let snapshots = services.snapshots.clone();
    let (controls_tx, controls_rx) = mpsc::unbounded_channel();
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let forward = app_messages.clone();
    tokio::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            if forward.send(AppMessage::Transaction(event)).is_err() {
                break;
            }
        }
    });
    let failure_events = events_tx.clone();
    tokio::spawn(async move {
        if snapshot_before_upgrade && request.supports_pre_snapshot() {
            let Some(snapshot_name) = snapshots.name() else {
                failure_events
                    .send(arch_maint::domain::TransactionEvent::FailedToStart(
                        "Pre-transaction snapshot requested, but no snapshot backend is available."
                            .into(),
                    ))
                    .ok();
                return;
            };
            events_tx
                .send(arch_maint::domain::TransactionEvent::SnapshotStarted {
                    backend: snapshot_name.into(),
                })
                .ok();
            match snapshots
                .create_pre_transaction(&format!("arch-maint: {}", request.label()))
                .await
            {
                Ok(snapshot) => {
                    events_tx
                        .send(arch_maint::domain::TransactionEvent::SnapshotCreated(
                            snapshot,
                        ))
                        .ok();
                }
                Err(error) => {
                    failure_events
                        .send(arch_maint::domain::TransactionEvent::FailedToStart(format!(
                            "Pre-transaction snapshot failed; package transaction was not started: {error:#}"
                        )))
                        .ok();
                    return;
                }
            }
        }
        if let Err(error) = backend.execute(request, events_tx, controls_rx).await {
            failure_events
                .send(arch_maint::domain::TransactionEvent::FailedToStart(
                    format!("Transaction process failed: {error:#}"),
                ))
                .ok();
        }
    });
    controls_tx
}

fn spawn_input_thread(
    tx: mpsc::UnboundedSender<crossterm::event::KeyEvent>,
    running: Arc<AtomicBool>,
    suspended: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while running.load(Ordering::Relaxed) {
            if suspended.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        if tx.send(key).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(%error, "input read failed");
                        break;
                    }
                },
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(%error, "input poll failed");
                    break;
                }
            }
        }
    });
}

struct InputSuspendGuard(Arc<AtomicBool>);

impl InputSuspendGuard {
    fn new(value: Arc<AtomicBool>) -> Self {
        value.store(true, Ordering::Release);
        Self(value)
    }
}

impl Drop for InputSuspendGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn refresh(app: &App, services: &Services, tx: &mpsc::UnboundedSender<AppMessage>) {
    if app.demo || app.profile.tools.pacman {
        let backend = services.packages.clone();
        let sender = tx.clone();
        tokio::spawn(async move {
            let result = backend
                .installed_packages()
                .await
                .map_err(|error| format!("Installed packages: {error:#}"));
            sender.send(AppMessage::Installed(result)).ok();
        });
        let backend = services.packages.clone();
        let sender = tx.clone();
        tokio::spawn(async move {
            let result = backend
                .check_updates()
                .await
                .map_err(|error| format!("Official updates: {error:#}"));
            sender.send(AppMessage::OfficialUpdates(result)).ok();
        });
        let backend = services.history.clone();
        let sender = tx.clone();
        tokio::spawn(async move {
            let result = backend
                .transactions()
                .await
                .map_err(|error| format!("Transaction history: {error:#}"));
            sender.send(AppMessage::History(result)).ok();
        });
        let backend = services.health.clone();
        let sender = tx.clone();
        tokio::spawn(async move {
            let result = backend
                .check()
                .await
                .map_err(|error| format!("System health check: {error:#}"));
            sender.send(AppMessage::Health(Box::new(result))).ok();
        });
    }
    if app.demo || app.profile.selected_aur_helper.is_some() {
        let backend = services.aur.clone();
        let sender = tx.clone();
        tokio::spawn(async move {
            let result = backend
                .check_updates()
                .await
                .map_err(|error| format!("AUR updates: {error:#}"));
            sender.send(AppMessage::AurUpdates(result)).ok();
        });
    }
    if app.show_arch_news {
        let backend = services.news.clone();
        let sender = tx.clone();
        tokio::spawn(async move {
            let result = backend
                .latest()
                .await
                .map_err(|error| format!("Arch news: {error:#}"));
            sender.send(AppMessage::News(result)).ok();
        });
    }
}

fn search(
    tab: Tab,
    query: String,
    generation: u64,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let packages = services.packages.clone();
    let aur = services.aur.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = if tab == Tab::Aur {
            aur.search(&query).await
        } else {
            let (official, community) =
                tokio::join!(packages.search_official(&query), aur.search(&query));
            match (official, community) {
                (Ok(mut official), Ok(mut aur)) => {
                    official.append(&mut aur);
                    Ok(official)
                }
                (Ok(official), Err(error)) if !official.is_empty() => {
                    tracing::warn!(%error, "AUR search unavailable; showing official results");
                    Ok(official)
                }
                (Err(error), Ok(aur)) if !aur.is_empty() => {
                    tracing::warn!(%error, "official search unavailable; showing AUR results");
                    Ok(aur)
                }
                (Err(first), Err(second)) => Err(anyhow::anyhow!(
                    "official search: {first:#}; AUR search: {second:#}"
                )),
                (Ok(official), Err(_)) => Ok(official),
                (Err(_), Ok(aur)) => Ok(aur),
            }
        }
        .map_err(|error| format!("Search failed: {error:#}"));
        sender.send(AppMessage::Search { generation, result }).ok();
    });
}

fn inspect(
    package: arch_maint::domain::Package,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let packages = services.packages.clone();
    let aur = services.aur.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = if matches!(package.source, arch_maint::domain::PackageSource::Aur) {
            aur.info(&package.name).await.map(|details| {
                details
                    .map(|mut details| {
                        // RPC metadata describes the AUR record, not local
                        // installation state. Preserve only locally evidenced
                        // fields from the selected package.
                        details.installed = package.installed;
                        details.install_reason = package.install_reason;
                        details.install_date.clone_from(&package.install_date);
                        details.installed_size = package.installed_size;
                        details
                    })
                    .unwrap_or(package)
            })
        } else {
            packages.package_details(&package).await
        }
        .map_err(|error| format!("Package details: {error:#}"));
        sender.send(AppMessage::Details(Box::new(result))).ok();
    });
}

fn build_flight_plan(app: &App, services: &Services, tx: &mpsc::UnboundedSender<AppMessage>) {
    let planner = services.planner.clone();
    let official = app.official_updates.clone();
    let aur = app.aur_updates.clone();
    let installed = app.installed.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = planner
            .build_flight_plan(&official, &aur, &installed)
            .await
            .map_err(|error| format!("Transaction Flight Plan: {error:#}"));
        sender
            .send(AppMessage::FlightPlan {
                request: arch_maint::domain::TransactionRequest::SystemUpgrade,
                result: Box::new(result),
            })
            .ok();
    });
}

fn build_install_flight_plan(
    targets: Vec<String>,
    app: &App,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let planner = services.planner.clone();
    let official = app.official_updates.clone();
    let aur = app.aur_updates.clone();
    let installed = app.installed.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = planner
            .build_install_flight_plan(&targets, &official, &aur, &installed)
            .await
            .map_err(|error| format!("Install Flight Plan: {error:#}"));
        sender
            .send(AppMessage::FlightPlan {
                request: arch_maint::domain::TransactionRequest::OfficialInstall {
                    packages: targets,
                },
                result: Box::new(result),
            })
            .ok();
    });
}

fn build_selected_update_flight_plan(
    package: String,
    app: &App,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let planner = services.planner.clone();
    let official = app.official_updates.clone();
    let aur = app.aur_updates.clone();
    let installed = app.installed.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = planner
            .build_install_flight_plan(std::slice::from_ref(&package), &official, &aur, &installed)
            .await
            .map_err(|error| format!("Selected Update Flight Plan: {error:#}"));
        sender
            .send(AppMessage::FlightPlan {
                request: arch_maint::domain::TransactionRequest::OfficialUpdate { package },
                result: Box::new(result),
            })
            .ok();
    });
}

fn build_aur_flight_plan(
    targets: Vec<String>,
    app: &App,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let planner = services.planner.clone();
    let official = app.official_updates.clone();
    let aur = app.aur_updates.clone();
    let installed = app.installed.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = planner
            .build_flight_plan(&official, &aur, &installed)
            .await
            .map_err(|error| format!("AUR helper full-upgrade Flight Plan: {error:#}"));
        sender
            .send(AppMessage::FlightPlan {
                request: arch_maint::domain::TransactionRequest::AurInstall { packages: targets },
                result: Box::new(result),
            })
            .ok();
    });
}

fn simulate_removal(
    packages: Vec<String>,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let backend = services.removal.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = backend
            .simulate_removal(&packages, true)
            .await
            .map_err(|error| format!("Removal simulation: {error:#}"));
        sender.send(AppMessage::RemovalPlan(result)).ok();
    });
}

fn review_pkgbuild(
    package: arch_maint::domain::Package,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let backend = services.aur.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = backend
            .review_pkgbuild(&package)
            .await
            .map_err(|error| format!("PKGBUILD review: {error:#}"));
        sender
            .send(AppMessage::PkgbuildReview(Box::new(result)))
            .ok();
    });
}

fn review_aur_package_name(
    package: String,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let backend = services.aur.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = async {
            let package = backend
                .info(&package)
                .await?
                .context("AUR RPC returned no package metadata")?;
            backend.review_pkgbuild(&package).await
        }
        .await
        .map_err(|error| format!("PKGBUILD review: {error:#}"));
        sender
            .send(AppMessage::PkgbuildReview(Box::new(result)))
            .ok();
    });
}

fn review_config(
    artifact: arch_maint::domain::ConfigArtifact,
    services: &Services,
    tx: &mpsc::UnboundedSender<AppMessage>,
) {
    let backend = services.config_files.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = backend
            .review(&artifact)
            .await
            .map_err(|error| format!("Configuration review: {error:#}"));
        sender.send(AppMessage::ConfigReview(Box::new(result))).ok();
    });
}

fn manifest_path() -> PathBuf {
    state_dir().join("manifest.toml")
}

fn export_manifest(app: &App, tx: &mpsc::UnboundedSender<AppMessage>) {
    let manifest = arch_maint::domain::PackageManifest::from_installed(&app.installed);
    let path = manifest_path();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = async {
            let parent = path.parent().context("manifest path has no parent")?;
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create {}", parent.display()))?;
            let content = toml::to_string_pretty(&manifest).context("failed to encode manifest")?;
            tokio::fs::write(&path, content)
                .await
                .with_context(|| format!("failed to write {}", path.display()))?;
            Ok::<_, anyhow::Error>(path.display().to_string())
        }
        .await
        .map_err(|error| format!("Manifest export: {error:#}"));
        sender.send(AppMessage::ManifestExported(result)).ok();
    });
}

fn compare_manifest(app: &App, tx: &mpsc::UnboundedSender<AppMessage>) {
    let installed = app.installed.clone();
    let path = manifest_path();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = async {
            let content = tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("failed to read {}", path.display()))?;
            let manifest: arch_maint::domain::PackageManifest = toml::from_str(&content)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            Ok::<_, anyhow::Error>((path.display().to_string(), manifest.drift(&installed)))
        }
        .await
        .map_err(|error| format!("Manifest comparison: {error:#}"));
        sender
            .send(AppMessage::ManifestCompared(Box::new(result)))
            .ok();
    });
}

fn load_hygiene(app: &App, services: &Services, tx: &mpsc::UnboundedSender<AppMessage>) {
    let backend = services.hygiene.clone();
    let installed = app.installed.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = backend
            .inspect(&installed)
            .await
            .map_err(|error| format!("Package hygiene inspection: {error:#}"));
        sender.send(AppMessage::Hygiene(Box::new(result))).ok();
    });
}

fn load_snapshots(services: &Services, tx: &mpsc::UnboundedSender<AppMessage>) {
    let backend = services.snapshots.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = backend
            .list()
            .await
            .map_err(|error| format!("Snapshot listing: {error:#}"));
        sender.send(AppMessage::Snapshots(result)).ok();
    });
}

fn load_hooks(services: &Services, tx: &mpsc::UnboundedSender<AppMessage>) {
    let backend = services.hooks.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        let result = backend
            .hooks()
            .await
            .map_err(|error| format!("ALPM hook inspection: {error:#}"));
        sender.send(AppMessage::Hooks(result)).ok();
    });
}
