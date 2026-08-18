use super::{
    CommandRunner, CommandSpec, FlightPlanBackend, HistoryBackend, HookBackend, PackageBackend,
    RemovalBackend,
};
use crate::config::cache_dir;
use crate::{
    analysis::{FlightPlanInput, build_flight_plan},
    domain::{
        FlightPlan, HistoryTransaction, HookDefinition, Package, PackageSource, PackageUpdate,
        RemovalPlan,
    },
    parser::{
        AlpmHook, parse_alpm_hook, parse_info_records, parse_modified_backup_records,
        parse_pacman_log, parse_pacman_policy, parse_removal_print, parse_search,
        parse_transaction_print, parse_updates, validate_search_query,
    },
};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct PacmanBackend {
    runner: CommandRunner,
    checkupdates_available: bool,
    log_path: PathBuf,
    config_path: PathBuf,
    hook_directories: Vec<PathBuf>,
    checkupdates_db: PathBuf,
}

#[async_trait]
impl HookBackend for PacmanBackend {
    async fn hooks(&self) -> Result<Vec<HookDefinition>> {
        let mut notes = Vec::new();
        let hooks = load_hooks(&self.hook_directories, &mut notes).await;
        if hooks.is_empty() && !notes.is_empty() {
            bail!("ALPM hook inspection failed: {}", notes.join("; "));
        }
        Ok(hooks
            .into_iter()
            .map(|hook| {
                let mut operations = hook
                    .triggers
                    .iter()
                    .flat_map(|trigger| trigger.operations.iter().cloned())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                operations.sort();
                let mut targets = hook
                    .triggers
                    .into_iter()
                    .flat_map(|trigger| trigger.targets)
                    .collect::<Vec<_>>();
                targets.sort();
                targets.dedup();
                HookDefinition {
                    name: hook.name,
                    description: hook.description,
                    stage: hook.stage,
                    command: hook.command,
                    operations,
                    targets,
                }
            })
            .collect())
    }
}

impl PacmanBackend {
    pub fn new(checkupdates_available: bool) -> Self {
        Self {
            runner: CommandRunner,
            checkupdates_available,
            log_path: PathBuf::from("/var/log/pacman.log"),
            config_path: PathBuf::from("/etc/pacman.conf"),
            hook_directories: vec![
                PathBuf::from("/usr/share/libalpm/hooks"),
                PathBuf::from("/etc/pacman.d/hooks"),
            ],
            checkupdates_db: cache_dir().join("checkupdates-db"),
        }
    }

    #[cfg(test)]
    pub fn with_log_path(path: PathBuf) -> Self {
        Self {
            runner: CommandRunner,
            checkupdates_available: false,
            log_path: path,
            config_path: PathBuf::from("/etc/pacman.conf"),
            hook_directories: Vec::new(),
            checkupdates_db: cache_dir().join("checkupdates-db"),
        }
    }

    async fn foreign_names(&self) -> HashSet<String> {
        let spec = CommandSpec::read_only("pacman", ["-Qmq"]);
        self.runner
            .run(spec)
            .await
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output.stdout.lines().map(ToOwned::to_owned).collect())
            .unwrap_or_default()
    }
}

#[async_trait]
impl RemovalBackend for PacmanBackend {
    async fn simulate_removal(
        &self,
        packages: &[String],
        remove_unused: bool,
    ) -> Result<RemovalPlan> {
        if packages.is_empty() {
            bail!("at least one package is required for removal simulation");
        }
        for package in packages {
            validate_package_name(package)?;
        }
        let operation = if remove_unused { "-Rsp" } else { "-Rp" };
        let mut args = vec![
            operation.to_owned(),
            "--print-format".into(),
            "%n|%v|%s".into(),
            "--".into(),
        ];
        args.extend(packages.iter().cloned());
        let output = self
            .runner
            .run(CommandSpec::read_only("pacman", args))
            .await
            .context("removal simulation failed to start")?;
        let candidates = parse_removal_print(&output.stdout);
        let requested = packages.iter().map(String::as_str).collect::<HashSet<_>>();
        let (direct_removals, dependencies_becoming_unused) = candidates
            .into_iter()
            .partition::<Vec<_>, _>(|candidate| requested.contains(candidate.name.as_str()));
        let all_sizes = direct_removals
            .iter()
            .chain(&dependencies_becoming_unused)
            .map(|candidate| candidate.installed_size)
            .collect::<Option<Vec<_>>>();
        let mut evidence_notes = Vec::new();
        if !output.stderr.trim().is_empty() {
            evidence_notes.push(output.stderr.trim().to_owned());
        }
        let affected_packages = dependency_blockers(&output.stderr);
        let missing_direct = direct_removals.len() != packages.len();
        if missing_direct && output.status.success() {
            evidence_notes.push(
                "Pacman did not return every requested target; execution is blocked until the discrepancy is understood."
                    .into(),
            );
        }
        Ok(RemovalPlan {
            requested: packages.to_vec(),
            direct_removals,
            dependencies_becoming_unused,
            affected_packages,
            space_reclaimed: all_sizes.map(|sizes| sizes.into_iter().sum()),
            blocked: !output.status.success() || missing_direct,
            evidence_notes,
        })
    }
}

fn dependency_blockers(stderr: &str) -> Vec<String> {
    let mut blockers = stderr
        .lines()
        .filter_map(|line| line.split(" required by ").nth(1))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(|name| name.trim_matches(['\'', '"', '.', ',']).to_owned())
        .collect::<Vec<_>>();
    blockers.sort();
    blockers.dedup();
    blockers
}

#[async_trait]
impl FlightPlanBackend for PacmanBackend {
    async fn build_flight_plan(
        &self,
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan> {
        self.build_flight_plan_for_targets(&[], official_updates, aur_updates, installed)
            .await
    }

    async fn build_install_flight_plan(
        &self,
        targets: &[String],
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan> {
        if targets.is_empty() {
            bail!("at least one install target is required");
        }
        for target in targets {
            validate_package_name(target)?;
        }
        self.build_flight_plan_for_targets(targets, official_updates, aur_updates, installed)
            .await
    }
}

impl PacmanBackend {
    async fn build_flight_plan_for_targets(
        &self,
        targets: &[String],
        official_updates: &[PackageUpdate],
        aur_updates: &[PackageUpdate],
        installed: &[Package],
    ) -> Result<FlightPlan> {
        let mut evidence_notes = Vec::new();
        let policy = match tokio::fs::read_to_string(&self.config_path).await {
            Ok(config) => parse_pacman_policy(&config),
            Err(error) => {
                evidence_notes.push(format!(
                    "Could not read {}: {error}",
                    self.config_path.display()
                ));
                Default::default()
            }
        };

        let mut transaction_args = Vec::new();
        if self.checkupdates_available {
            transaction_args.extend([
                "--dbpath".to_owned(),
                self.checkupdates_db.to_string_lossy().into_owned(),
            ]);
        } else {
            evidence_notes.push(
                "Preview uses the existing system sync database because checkupdates is unavailable; repository metadata may be stale."
                    .into(),
            );
        }
        transaction_args.extend([
            "-Sup".to_owned(),
            "--print-format".to_owned(),
            "%n|%v|%r|%s".to_owned(),
        ]);
        if !targets.is_empty() {
            transaction_args.push("--".into());
            transaction_args.extend(targets.iter().cloned());
        }
        let transaction = self
            .runner
            .run(
                CommandSpec::read_only("pacman", transaction_args)
                    .with_timeout(Duration::from_secs(90)),
            )
            .await;
        let candidates = match transaction {
            Ok(output) if output.status.success() => parse_transaction_print(&output.stdout),
            Ok(output) => {
                evidence_notes.push(format!(
                    "Pacman transaction print failed: {}",
                    output.stderr.trim()
                ));
                Vec::new()
            }
            Err(error) => {
                evidence_notes.push(format!("Pacman transaction print was unavailable: {error}"));
                Vec::new()
            }
        };

        let mut names = candidates
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect::<Vec<_>>();
        names.extend(official_updates.iter().map(|update| update.name.clone()));
        names.sort();
        names.dedup();
        let sync_details = self.query_sync_details(&names, &mut evidence_notes).await;
        let modified_backups = self
            .query_modified_backups(official_updates, &mut evidence_notes)
            .await;
        let hooks = load_hooks(&self.hook_directories, &mut evidence_notes).await;

        Ok(build_flight_plan(FlightPlanInput {
            official_updates,
            aur_updates,
            installed,
            transaction_candidates: &candidates,
            sync_details: &sync_details,
            policy,
            modified_backups: &modified_backups,
            hooks: &hooks,
            evidence_notes,
        }))
    }
}

impl PacmanBackend {
    async fn query_sync_details(&self, names: &[String], notes: &mut Vec<String>) -> Vec<Package> {
        if names.is_empty() {
            return Vec::new();
        }
        let mut args = Vec::new();
        if self.checkupdates_available {
            args.extend([
                "--dbpath".to_owned(),
                self.checkupdates_db.to_string_lossy().into_owned(),
            ]);
        }
        args.push("-Si".to_owned());
        args.extend(names.iter().cloned());
        match self
            .runner
            .run(CommandSpec::read_only("pacman", args))
            .await
        {
            Ok(output) => {
                if !output.status.success() {
                    notes.push(format!(
                        "Some synchronized package metadata was unavailable: {}",
                        output.stderr.trim()
                    ));
                }
                parse_info_records(&output.stdout, PackageSource::Official("repo".into()))
            }
            Err(error) => {
                notes.push(format!(
                    "Could not query synchronized package metadata: {error}"
                ));
                Vec::new()
            }
        }
    }

    async fn query_modified_backups(
        &self,
        updates: &[PackageUpdate],
        notes: &mut Vec<String>,
    ) -> Vec<(String, String)> {
        if updates.is_empty() {
            return Vec::new();
        }
        let mut args = vec!["-Qii".to_owned()];
        args.extend(updates.iter().map(|update| update.name.clone()));
        match self
            .runner
            .run(CommandSpec::read_only("pacman", args))
            .await
        {
            Ok(output) => {
                if !output.status.success() {
                    notes.push(format!(
                        "Modified configuration inspection was incomplete: {}",
                        output.stderr.trim()
                    ));
                }
                parse_modified_backup_records(&output.stdout)
            }
            Err(error) => {
                notes.push(format!(
                    "Could not inspect modified package backup files: {error}"
                ));
                Vec::new()
            }
        }
    }
}

async fn load_hooks(directories: &[PathBuf], notes: &mut Vec<String>) -> Vec<AlpmHook> {
    let mut hooks = BTreeMap::new();
    for directory in directories {
        let mut entries = match tokio::fs::read_dir(directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                notes.push(format!(
                    "Could not read hook directory {}: {error}",
                    directory.display()
                ));
                continue;
            }
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("hook") {
                continue;
            }
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                let name = file_name(&path);
                if let Some(hook) = parse_alpm_hook(&name, &content) {
                    hooks.insert(name, hook);
                }
            }
        }
    }
    hooks.into_values().collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown.hook")
        .to_owned()
}

#[async_trait]
impl PackageBackend for PacmanBackend {
    async fn installed_packages(&self) -> Result<Vec<Package>> {
        let spec = CommandSpec::read_only("pacman", ["-Qi"]).with_timeout(Duration::from_secs(60));
        let (output, foreign) = tokio::join!(self.runner.run_checked(spec), self.foreign_names());
        let output = output.context("failed to list installed packages")?;
        let mut packages =
            parse_info_records(&output.stdout, PackageSource::Official("installed".into()));
        for package in &mut packages {
            if foreign.contains(&package.name) {
                package.source = PackageSource::Foreign;
            }
        }
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(packages)
    }

    async fn search_official(&self, query: &str) -> Result<Vec<Package>> {
        let query = validate_search_query(query)?;
        let output = self
            .runner
            .run(CommandSpec::read_only("pacman", ["-Ss", query]))
            .await
            .context("official repository search failed")?;
        // pacman -Ss returns 1 when there are no matches.
        if output.status.code() == Some(1)
            && output.stdout.trim().is_empty()
            && output.stderr.trim().is_empty()
        {
            return Ok(Vec::new());
        }
        if !output.status.success() {
            bail!(
                "official repository search failed: {}",
                output.stderr.trim()
            );
        }
        Ok(parse_search(&output.stdout))
    }

    async fn package_details(&self, package: &Package) -> Result<Package> {
        let query = validate_package_name(&package.name)?;
        let operation = if package.installed { "-Qi" } else { "-Si" };
        let output = self
            .runner
            .run_checked(CommandSpec::read_only("pacman", [operation, query]))
            .await
            .with_context(|| format!("failed to inspect package {}", package.name))?;
        let mut details = parse_info_records(&output.stdout, package.source.clone())
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("pacman returned no details for {}", package.name))?;

        // `pacman -Qi` does not report a repository. For native installed
        // packages, enrich the local record with the synchronized repository
        // and download size when available, while retaining the installed
        // version and treating stale/missing sync metadata as non-fatal.
        if package.installed
            && matches!(package.source, PackageSource::Official(_))
            && let Ok(sync) = self
                .runner
                .run(CommandSpec::read_only("pacman", ["-Si", query]))
                .await
            && sync.status.success()
            && let Some(sync_details) = parse_info_records(&sync.stdout, package.source.clone())
                .into_iter()
                .next()
        {
            details.source = sync_details.source;
            details.download_size = sync_details.download_size;
        }
        Ok(details)
    }

    async fn check_updates(&self) -> Result<Vec<PackageUpdate>> {
        let spec = if self.checkupdates_available {
            CommandSpec::read_only("checkupdates", std::iter::empty::<&str>())
                .with_env("CHECKUPDATES_DB", self.checkupdates_db.as_os_str())
                .with_timeout(Duration::from_secs(90))
        } else {
            CommandSpec::read_only("pacman", ["-Qu"])
        };
        let output = self
            .runner
            .run(spec)
            .await
            .context("official update check failed")?;
        // checkupdates reserves status 2 for a successful check with an empty set.
        if self.checkupdates_available && output.status.code() == Some(2) {
            return Ok(Vec::new());
        }
        if !output.status.success() {
            bail!("update check failed: {}", output.stderr.trim());
        }
        let mut updates = parse_updates(&output.stdout, PackageSource::Official("repo".into()));
        if let Ok(config) = tokio::fs::read_to_string(&self.config_path).await {
            let policy = parse_pacman_policy(&config);
            for update in &mut updates {
                update.ignored = policy
                    .ignore_packages
                    .iter()
                    .any(|pattern| simple_policy_match(pattern, &update.name));
            }
        }
        Ok(updates)
    }
}

#[async_trait]
impl HistoryBackend for PacmanBackend {
    async fn transactions(&self) -> Result<Vec<HistoryTransaction>> {
        let input = tokio::fs::read_to_string(&self.log_path)
            .await
            .with_context(|| format!("failed to read {}", self.log_path.display()))?;
        parse_pacman_log(&input)
    }
}

fn validate_package_name(name: &str) -> Result<&str> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._+:-".contains(c))
    {
        bail!("invalid package name `{name}`");
    }
    Ok(name)
}

fn simple_policy_match(pattern: &str, value: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == value,
        Some((prefix, suffix)) => value.starts_with(prefix) && value.ends_with(suffix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_arch_package_names() {
        assert!(validate_package_name("lib32-gcc-libs").is_ok());
        assert!(validate_package_name("python-foo_git").is_ok());
        assert!(validate_package_name("--overwrite").is_err());
        assert!(validate_package_name("foo bar").is_err());
    }
}
