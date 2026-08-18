use crate::{
    app::{App, InputMode, Tab, TransactionPhase, TransactionView},
    domain::{
        FindingSeverity, FlightPlan, HistoryTransaction, HookStage, Package, PackageAction,
        PackageUpdate, PlannedAction, TransactionRequest,
    },
    event::TaskKind,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Padding, Paragraph, Row, Table,
        TableState, Tabs, Wrap,
    },
};

const ACCENT: Color = Color::Cyan;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const MUTED: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < 72 || area.height < 22 {
        frame.render_widget(
            Paragraph::new(format!(
                "arch-maint needs at least 72×22\nCurrent terminal: {}×{}",
                area.width, area.height
            ))
            .alignment(Alignment::Center)
            .block(Block::bordered().title(" Terminal too small ")),
            area,
        );
        return;
    }

    let layout = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(1),
    ])
    .split(area);
    render_system(frame, app, layout[0]);
    render_tabs(frame, app, layout[1]);
    render_active_view(frame, app, layout[2]);
    render_status(frame, app, layout[3]);

    if app.show_help {
        render_help(frame, area);
    }
    if app.input_mode != InputMode::Normal {
        render_input(frame, app, area);
    }
    if let Some(pending) = &app.pending_transaction {
        render_transaction_confirmation(frame, app, pending, area);
    }
    if let Some(action) = app.pending_maintenance {
        render_maintenance_confirmation(frame, action, area);
    }
}

fn render_system(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let profile = &app.profile;
    let update_count = app.official_updates.len();
    let aur_count = app.aur_updates.len();
    let checks = [
        indicator(profile.is_arch, "Arch Linux detected", &profile.distro_name),
        indicator(profile.tools.pacman, "pacman available", "pacman missing"),
        indicator(
            profile.tools.checkupdates,
            "safe update checks",
            "using cached sync database",
        ),
        indicator(
            profile.tools.pacdiff,
            "pacdiff available",
            "pacdiff unavailable",
        ),
        count_indicator(
            update_count,
            "official update",
            app.loading.contains(&TaskKind::OfficialUpdates),
        ),
        count_indicator(
            aur_count,
            "AUR update",
            app.loading.contains(&TaskKind::AurUpdates),
        ),
    ];
    let columns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area.inner(Margin {
            horizontal: 2,
            vertical: 1,
        }));
    for (index, column) in columns.iter().enumerate() {
        let lines = checks[index * 3..index * 3 + 3].to_vec();
        frame.render_widget(Paragraph::new(lines), *column);
    }
    let title = if app.demo {
        " System · UNPRIVILEGED · DEMO "
    } else {
        " System · UNPRIVILEGED "
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(MUTED))
            .title(Span::styled(
                title,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
        area,
    );
}

fn indicator(ok: bool, good: &str, bad: &str) -> Line<'static> {
    if ok {
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(GOOD)),
            Span::raw(good.to_owned()),
        ])
    } else {
        Line::from(vec![
            Span::styled("! ", Style::default().fg(WARN)),
            Span::raw(bad.to_owned()),
        ])
    }
}

fn count_indicator(count: usize, singular: &str, loading: bool) -> Line<'static> {
    if loading {
        Line::from(vec![
            Span::styled("◌ ", Style::default().fg(ACCENT)),
            Span::raw(format!("Checking {singular}s…")),
        ])
    } else if count == 0 {
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(GOOD)),
            Span::raw(format!("No {singular}s found")),
        ])
    } else {
        Line::from(vec![
            Span::styled("! ", Style::default().fg(WARN)),
            Span::raw(format!(
                "{count} {singular}{}",
                if count == 1 { "" } else { "s" }
            )),
        ])
    }
}

fn render_tabs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = Tab::ALL
        .iter()
        .position(|tab| *tab == app.active_tab)
        .unwrap_or(0);
    let titles = Tab::ALL
        .iter()
        .map(|tab| Line::from(format!(" {} ", tab.title())))
        .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .divider(" ")
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(MUTED)),
            ),
        area,
    );
}

fn render_active_view(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if let Some(transaction) = &app.transaction {
        render_transaction(frame, transaction, area);
        return;
    }
    if let Some(plan) = &app.removal_plan {
        render_removal_plan(frame, plan, area);
        return;
    }
    if let Some(report) = &app.dependency_report {
        render_dependency_report(frame, app, report, area);
        return;
    }
    if let Some(review) = &app.config_review {
        render_config_review(frame, app, review, area);
        return;
    }
    if let Some((path, drift)) = &app.manifest_drift {
        render_manifest_drift(frame, path, drift, area);
        return;
    }
    if let Some(report) = &app.hygiene_report {
        render_hygiene(frame, app, report, area);
        return;
    }
    if app.show_news {
        render_arch_news(frame, app, area);
        return;
    }
    if let Some(snapshots) = &app.snapshots {
        render_snapshots(frame, app, snapshots, area);
        return;
    }
    if let Some(hooks) = &app.hooks {
        render_hook_inspector(frame, app, hooks, area);
        return;
    }
    if let Some(review) = &app.pkgbuild_review {
        render_pkgbuild_review(frame, app, review, area);
        return;
    }
    if let Some(plan) = &app.flight_plan {
        render_flight_plan(frame, app, plan, area);
        return;
    }
    match app.active_tab {
        Tab::Updates => render_updates(frame, app, area),
        Tab::Packages => render_packages(frame, app, area, false),
        Tab::Aur => render_packages(frame, app, area, true),
        Tab::Config => render_config(frame, app, area),
        Tab::Health => render_health(frame, app, area),
        Tab::History => render_history(frame, app, area),
    }
}

fn render_flight_plan(frame: &mut Frame<'_>, app: &App, plan: &FlightPlan, area: Rect) {
    let title = app
        .flight_plan_request
        .as_ref()
        .map(TransactionRequest::label)
        .unwrap_or("Transaction review");
    let mut lines = vec![
        Line::styled(
            title.to_uppercase(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!(
            "{} upgrades  ·  {} new  ·  {} replacements  ·  {} removals",
            plan.count(PlannedAction::Upgrade),
            plan.count(PlannedAction::Install),
            plan.count(PlannedAction::Replace),
            plan.count(PlannedAction::Remove),
        )),
        Line::raw(format!(
            "Download: {}  ·  Disk delta: {}  ·  Ignored: {}",
            plan.download_size
                .map(human_bytes)
                .unwrap_or_else(|| "unknown".into()),
            plan.installed_size_delta
                .map(human_signed_bytes)
                .unwrap_or_else(|| "unknown".into()),
            plan.ignored_count(),
        )),
        Line::raw(""),
    ];

    if let Some(TransactionRequest::AurInstall { packages }) = &app.flight_plan_request {
        plan_heading(&mut lines, "AUR HELPER TARGETS");
        lines.push(Line::raw(format!("  {}", packages.join(", "))));
        lines.push(Line::styled(
            "  The configured helper will run a full -Syu transaction and include these reviewed targets.",
            Style::default().fg(WARN),
        ));
        lines.push(Line::raw(""));
    }

    if let Some(TransactionRequest::OfficialUpdate { package }) = &app.flight_plan_request {
        plan_heading(&mut lines, "SELECTED UPDATE");
        lines.push(Line::styled(
            format!("  {package}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::styled(
            "  Arch safety: this target is passed to pacman -Syu; every required repository upgrade shown below remains in the transaction.",
            Style::default().fg(WARN),
        ));
        lines.push(Line::raw(""));
    }

    plan_heading(&mut lines, "ATTENTION");
    if plan.attention.is_empty() {
        lines.push(Line::styled(
            "  No reason-based attention findings detected.",
            Style::default().fg(GOOD),
        ));
    }
    for finding in &plan.attention {
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(WARN)),
            Span::styled(
                finding.kind.label(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", finding.packages.join(", ")),
                Style::default().fg(MUTED),
            ),
        ]));
        lines.push(Line::raw(format!("  {}", finding.explanation)));
    }

    lines.push(Line::raw(""));
    plan_heading(&mut lines, "PACKAGES");
    for package in &plan.packages {
        let action = match package.action {
            PlannedAction::Upgrade => "upgrade",
            PlannedAction::Install => "install",
            PlannedAction::Replace => "replace",
            PlannedAction::Remove => "remove",
        };
        let versions = match (&package.old_version, &package.new_version) {
            (Some(old), Some(new)) => format!("{old} → {new}"),
            (None, Some(new)) => new.clone(),
            (Some(old), None) => old.clone(),
            _ => "unknown".into(),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {action:8}"), Style::default().fg(ACCENT)),
            Span::styled(
                format!("{:<28}", package.name),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(versions),
            if package.ignored {
                Span::styled("  IGNORED", Style::default().fg(WARN))
            } else {
                Span::raw("")
            },
        ]));
    }

    lines.push(Line::raw(""));
    plan_heading(&mut lines, "AUR UPDATES AVAILABLE · SEPARATE WORKFLOW");
    if plan.separate_aur_updates.is_empty() {
        lines.push(Line::styled("  none", Style::default().fg(MUTED)));
    } else {
        for update in &plan.separate_aur_updates {
            lines.push(Line::raw(format!(
                "  {}  {} → {}",
                update.name, update.current_version, update.new_version
            )));
        }
        lines.push(Line::styled(
            "  These packages are not part of the Pacman command. Review each PKGBUILD from the Updates view.",
            Style::default().fg(WARN),
        ));
    }

    lines.push(Line::raw(""));
    plan_heading(&mut lines, "PACMAN POLICY");
    lines.push(Line::raw(format!(
        "  IgnorePkg: {}",
        display_list(&plan.policy.ignore_packages)
    )));
    lines.push(Line::raw(format!(
        "  IgnoreGroup: {}",
        display_list(&plan.policy.ignore_groups)
    )));
    lines.push(Line::raw(format!(
        "  HoldPkg: {}",
        display_list(&plan.policy.hold_packages)
    )));

    lines.push(Line::raw(""));
    plan_heading(&mut lines, "EXPECTED ALPM HOOKS");
    if plan.expected_hooks.is_empty() {
        lines.push(Line::styled(
            "  No package-triggered hooks matched the evidenced transaction targets.",
            Style::default().fg(MUTED),
        ));
    }
    for hook in &plan.expected_hooks {
        let stage = match hook.stage {
            HookStage::PreTransaction => "pre",
            HookStage::PostTransaction => "post",
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {stage:4} "), Style::default().fg(ACCENT)),
            Span::styled(
                hook.description.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ({})", hook.name), Style::default().fg(MUTED)),
        ]));
        lines.push(Line::styled(
            format!("       targets: {}", hook.matched_packages.join(", ")),
            Style::default().fg(MUTED),
        ));
    }

    lines.push(Line::raw(""));
    plan_heading(&mut lines, "AUR REBUILD CHECK");
    lines.push(Line::raw(format!(
        "  Candidates: {}",
        display_list(&plan.aur_rebuild_candidates)
    )));

    lines.push(Line::raw(""));
    plan_heading(&mut lines, "EVIDENCE / LIMITS");
    for note in &plan.evidence_notes {
        lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(MUTED)),
            Span::raw(note.clone()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "No changes have been made. This is an inspection view, not a safety guarantee.",
        Style::default().fg(WARN).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::styled(
        "Press Enter or x to continue; a separate confirmation and scoped sudo check follow.",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));

    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.flight_plan_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                section_block(
                    " TRANSACTION FLIGHT PLAN · Enter/x continue · j/k scroll · Esc closes ",
                )
                .padding(Padding::horizontal(1)),
            ),
        area,
    );
}

fn render_removal_plan(frame: &mut Frame<'_>, plan: &crate::domain::RemovalPlan, area: Rect) {
    let mut lines = vec![
        Line::styled(
            format!("REMOVE {}", plan.requested.join(", ")),
            Style::default()
                .fg(if plan.blocked { Color::Red } else { WARN })
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        detail(
            "Space reclaimed",
            plan.space_reclaimed
                .map(human_bytes)
                .unwrap_or_else(|| "unknown".into()),
        ),
        detail(
            "Affected/blocking packages",
            if plan.affected_packages.is_empty() {
                "none evidenced".into()
            } else {
                plan.affected_packages.join(", ")
            },
        ),
        Line::raw(""),
        Line::styled(
            "DIRECT REMOVAL",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ];
    lines.extend(plan.direct_removals.iter().map(|package| {
        Line::raw(format!(
            "  {}  {}  {}",
            package.name,
            package.version,
            package
                .installed_size
                .map(human_bytes)
                .unwrap_or_else(|| "unknown".into())
        ))
    }));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "DEPENDENCIES BECOMING UNUSED (included by -Rs)",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
    if plan.dependencies_becoming_unused.is_empty() {
        lines.push(Line::styled("  none", Style::default().fg(MUTED)));
    }
    lines.extend(plan.dependencies_becoming_unused.iter().map(|package| {
        Line::raw(format!(
            "  {}  {}  {}",
            package.name,
            package.version,
            package
                .installed_size
                .map(human_bytes)
                .unwrap_or_else(|| "unknown".into())
        ))
    }));
    if !plan.evidence_notes.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "PACMAN EVIDENCE",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        lines.extend(
            plan.evidence_notes
                .iter()
                .map(|note| Line::raw(format!("  • {note}"))),
        );
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "No changes have been made.",
        Style::default().fg(GOOD).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::styled(
        if plan.blocked {
            "Execution is blocked because Pacman could not prove a valid removal transaction. Esc closes."
        } else {
            "Press x to request this exact -Rs removal; confirmation and scoped sudo follow. Esc closes."
        },
        Style::default()
            .fg(if plan.blocked { Color::Red } else { WARN })
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(section_block(" REMOVAL SIMULATOR ").padding(Padding::horizontal(1))),
        area,
    );
}

fn render_pkgbuild_review(
    frame: &mut Frame<'_>,
    app: &App,
    review: &crate::domain::PkgbuildReview,
    area: Rect,
) {
    let mut lines = vec![
        Line::styled(
            format!("AUR PKGBUILD REVIEW · {}", review.package),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        detail(
            "Baseline",
            review
                .baseline_source
                .as_deref()
                .unwrap_or("unavailable — current PKGBUILD is shown without change claims"),
        ),
        Line::raw(""),
    ];
    plan_heading(&mut lines, "MEANINGFUL CHANGES");
    if !review.has_baseline() {
        lines.push(Line::styled(
            "  No prior helper-cache PKGBUILD was found. Review the complete current file.",
            Style::default().fg(WARN),
        ));
    } else if review.findings.is_empty() {
        lines.push(Line::styled(
            "  No classified semantic changes detected; inspect the diff for unclassified edits.",
            Style::default().fg(MUTED),
        ));
    } else {
        for finding in &review.findings {
            lines.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(WARN)),
                Span::styled(
                    finding.kind.label(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {}", finding.detail), Style::default().fg(MUTED)),
            ]));
        }
    }
    lines.push(Line::raw(""));
    plan_heading(
        &mut lines,
        if app.pkgbuild_show_diff && review.has_baseline() {
            "UNIFIED DIFF"
        } else {
            "CURRENT PKGBUILD"
        },
    );
    let content = if app.pkgbuild_show_diff && review.has_baseline() {
        &review.unified_diff
    } else {
        &review.current_pkgbuild
    };
    lines.extend(content.lines().map(|line| {
        let color = if line.starts_with('+') && !line.starts_with("+++") {
            GOOD
        } else if line.starts_with('-') && !line.starts_with("---") {
            Color::Red
        } else if line.starts_with("@@") {
            ACCENT
        } else {
            Color::White
        };
        Line::styled(line.to_owned(), Style::default().fg(color))
    }));
    for file in &review.related_files {
        lines.push(Line::raw(""));
        plan_heading(&mut lines, &format!("RELATED AUR FILE · {}", file.path));
        let related = if app.pkgbuild_show_diff {
            file.unified_diff
                .as_deref()
                .unwrap_or(file.current_content.as_str())
        } else {
            file.current_content.as_str()
        };
        lines.extend(related.lines().map(|line| {
            let color = if line.starts_with('+') && !line.starts_with("+++") {
                GOOD
            } else if line.starts_with('-') && !line.starts_with("---") {
                Color::Red
            } else if line.starts_with("@@") {
                ACCENT
            } else {
                Color::White
            };
            Line::styled(line.to_owned(), Style::default().fg(color))
        }));
    }
    for note in &review.evidence_notes {
        lines.push(Line::styled(
            format!("• {note}"),
            Style::default().fg(MUTED),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Automated classification and visual inspection do not guarantee an AUR package is safe.",
        Style::default().fg(WARN).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::styled(
        "v toggles diff/current · x continues to the helper's full-upgrade Flight Plan · Esc cancels",
        Style::default().fg(ACCENT),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.pkgbuild_review_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                section_block(" PKGBUILD CHANGE INSPECTION · j/k scroll ")
                    .padding(Padding::horizontal(1)),
            ),
        area,
    );
}

fn render_dependency_report(
    frame: &mut Frame<'_>,
    app: &App,
    report: &crate::domain::DependencyReport,
    area: Rect,
) {
    let mut lines = vec![
        Line::styled(
            report.package.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        detail("Installed as", report.install_reason),
        detail("Traversal depth", report.max_depth),
        Line::raw(""),
    ];
    plan_heading(&mut lines, "WHY IS THIS HERE?");
    if report.install_reason == crate::domain::InstallReason::Explicit {
        lines.push(Line::raw("  Explicitly installed by the user."));
    } else if report.why_paths.is_empty() {
        lines.push(Line::styled(
            "  No path to an explicitly installed package was found within the configured depth.",
            Style::default().fg(MUTED),
        ));
    } else {
        for path in &report.why_paths {
            lines.push(Line::raw(format!("  {}", path.join(" → "))));
        }
    }
    lines.push(Line::raw(""));
    plan_heading(&mut lines, "DEPENDENCIES");
    append_dependency_tree(&mut lines, &report.dependencies, "", true);
    lines.push(Line::raw(""));
    plan_heading(&mut lines, "REVERSE DEPENDENCIES / WHAT MAY BREAK");
    append_dependency_tree(&mut lines, &report.reverse_dependencies, "", true);
    lines.push(Line::raw(""));
    plan_heading(&mut lines, "ORPHAN CANDIDATES AFTER REMOVAL");
    lines.push(Line::raw(format!(
        "  {}",
        display_list(&report.orphan_candidates_after_removal)
    )));
    lines.push(Line::styled(
        "This graph uses installed metadata. Run the removal simulator for Pacman's authoritative transaction preview.",
        Style::default().fg(WARN),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.dependency_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                section_block(" DEPENDENCY EXPLORER · j/k scroll · d/Esc closes ")
                    .padding(Padding::horizontal(1)),
            ),
        area,
    );
}

fn append_dependency_tree(
    lines: &mut Vec<Line<'static>>,
    node: &crate::domain::DependencyNode,
    prefix: &str,
    root: bool,
) {
    let suffix = if node.cycle {
        "  ↻ cycle"
    } else if node.depth_limited {
        "  … depth limit"
    } else {
        ""
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {prefix}{}{}", if root { "" } else { "└── " }, node.name),
            Style::default().fg(if node.cycle { WARN } else { Color::White }),
        ),
        Span::styled(suffix, Style::default().fg(WARN)),
    ]));
    let next_prefix = format!("{prefix}    ");
    for child in &node.children {
        append_dependency_tree(lines, child, &next_prefix, false);
    }
}

fn render_config_review(
    frame: &mut Frame<'_>,
    app: &App,
    review: &crate::domain::ConfigReview,
    area: Rect,
) {
    let (heading, content) = match app.config_review_mode {
        1 => (
            "CURRENT FILE",
            review
                .current_content
                .as_deref()
                .unwrap_or("Current file does not exist."),
        ),
        2 => ("PACKAGE ARTIFACT", review.artifact_content.as_str()),
        _ => (
            "UNIFIED DIFF",
            review.unified_diff.as_deref().unwrap_or(
                "A two-file diff is unavailable because the current file does not exist.",
            ),
        ),
    };
    let mut lines = vec![
        Line::styled(
            format!(
                "{} · {}",
                review.artifact.kind.label(),
                review.artifact.path
            ),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        detail("Current", &review.current_path),
        Line::raw(""),
    ];
    plan_heading(&mut lines, heading);
    lines.extend(content.lines().map(|line| {
        let color = if line.starts_with('+') && !line.starts_with("+++") {
            GOOD
        } else if line.starts_with('-') && !line.starts_with("---") {
            Color::Red
        } else if line.starts_with("@@") {
            ACCENT
        } else {
            Color::White
        };
        Line::styled(line.to_owned(), Style::default().fg(color))
    }));
    lines.push(Line::raw(""));
    for note in &review.evidence_notes {
        lines.push(Line::styled(
            format!("• {note}"),
            Style::default().fg(MUTED),
        ));
    }
    lines.push(Line::styled(
        "v cycles views · p opens confirmed pacdiff workflow · Esc closes",
        Style::default().fg(ACCENT),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.config_review_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                section_block(" CONFIGURATION RECONCILIATION · READ-ONLY REVIEW ")
                    .padding(Padding::horizontal(1)),
            ),
        area,
    );
}

fn render_manifest_drift(
    frame: &mut Frame<'_>,
    path: &str,
    drift: &crate::domain::ManifestDrift,
    area: Rect,
) {
    let mut lines = vec![
        Line::styled(
            "PACKAGE MANIFEST DRIFT",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        detail("Manifest", path),
        Line::raw(""),
    ];
    for (heading, packages) in [
        ("MISSING OFFICIAL", &drift.missing_official),
        ("MISSING AUR", &drift.missing_aur),
        ("MISSING UNCLASSIFIED FOREIGN", &drift.missing_foreign),
        ("EXTRA EXPLICIT OFFICIAL", &drift.extra_official),
        ("EXTRA EXPLICIT FOREIGN", &drift.extra_foreign),
    ] {
        plan_heading(&mut lines, heading);
        lines.push(Line::raw(format!("  {}", display_list(packages))));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "Comparison is read-only. No reconciliation, installation, or removal has been performed.",
        Style::default().fg(WARN).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::styled("m/Esc closes", Style::default().fg(ACCENT)));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            section_block(" DESIRED STATE · READ-ONLY COMPARISON ").padding(Padding::horizontal(1)),
        ),
        area,
    );
}

fn render_hygiene(
    frame: &mut Frame<'_>,
    app: &App,
    report: &crate::domain::HygieneReport,
    area: Rect,
) {
    let old_entries = report
        .cache_entries
        .iter()
        .filter(|entry| entry.package.is_some() && !entry.current_installed_version)
        .collect::<Vec<_>>();
    let unclassified = report
        .cache_entries
        .iter()
        .filter(|entry| entry.package.is_none())
        .count();
    let mut lines = vec![
        Line::styled(
            "INSTALLED PACKAGE HYGIENE",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        detail("Explicitly installed", report.explicit_packages.len()),
        detail("Dependency-installed", report.dependency_packages.len()),
        detail("Orphan candidates", report.orphaned_packages.len()),
        detail("Foreign/AUR", report.foreign_packages.len()),
        Line::raw(""),
    ];
    plan_heading(&mut lines, "ORPHAN CANDIDATES");
    lines.push(Line::raw(format!(
        "  {}",
        display_list(&report.orphaned_packages)
    )));
    lines.push(Line::styled(
        "  No removal is offered here; inspect dependencies and run the removal simulator first.",
        Style::default().fg(WARN),
    ));
    lines.push(Line::raw(""));
    plan_heading(&mut lines, "FOREIGN PACKAGES");
    lines.push(Line::raw(format!(
        "  {}",
        display_list(&report.foreign_packages)
    )));
    lines.push(Line::raw(""));
    plan_heading(&mut lines, "PACKAGE CACHE · PREVIEW ONLY");
    lines.push(detail("Total cache size", human_bytes(report.cache_size)));
    lines.push(detail(
        "Matched old-version size",
        human_bytes(report.old_cached_versions_size),
    ));
    lines.push(detail("Unclassified cache files", unclassified));
    for entry in old_entries {
        lines.push(Line::from(vec![
            Span::styled("  old  ", Style::default().fg(WARN)),
            Span::styled(
                entry.package.as_deref().unwrap_or("unknown").to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  {}  {}",
                entry.version.as_deref().unwrap_or("unknown version"),
                human_bytes(entry.size)
            )),
        ]));
    }
    lines.push(Line::raw(""));
    for note in &report.evidence_notes {
        lines.push(Line::styled(
            format!("• {note}"),
            Style::default().fg(MUTED),
        ));
    }
    lines.push(Line::styled(
        "No cache files or packages have been removed. c/Esc closes.",
        Style::default().fg(GOOD).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.hygiene_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(section_block(" PACKAGE HYGIENE · j/k scroll ").padding(Padding::horizontal(1))),
        area,
    );
}

fn render_arch_news(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![
        Line::styled(
            "ARCH LINUX NEWS",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "Read official manual-intervention notices before upgrading. This view does not infer applicability.",
            Style::default().fg(WARN),
        ),
        Line::raw(""),
    ];
    if app.news.is_empty() {
        lines.push(Line::styled(
            if app.loading.contains(&TaskKind::News) {
                "Loading the official Arch news feed…"
            } else {
                "No news items are available."
            },
            Style::default().fg(MUTED),
        ));
    }
    for item in &app.news {
        lines.push(Line::styled(
            item.title.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        if let Some(published) = &item.published {
            lines.push(Line::styled(
                format!("  {published}"),
                Style::default().fg(MUTED),
            ));
        }
        if let Some(summary) = &item.summary {
            lines.push(Line::raw(format!("  {summary}")));
        }
        lines.push(Line::styled(
            format!("  {}", item.link),
            Style::default().fg(MUTED),
        ));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "n/Esc closes · j/k scroll",
        Style::default().fg(ACCENT),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.news_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(section_block(" OFFICIAL NEWS FEED ").padding(Padding::horizontal(1))),
        area,
    );
}

fn render_snapshots(
    frame: &mut Frame<'_>,
    app: &App,
    snapshots: &[crate::domain::Snapshot],
    area: Rect,
) {
    let mut lines = vec![
        Line::styled(
            "AVAILABLE SNAPSHOTS",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "Listing is read-only. No rollback, mount, or deletion action is available here.",
            Style::default().fg(WARN),
        ),
        Line::raw(""),
    ];
    if snapshots.is_empty() {
        lines.push(Line::styled(
            "No snapshots were returned by the configured backend.",
            Style::default().fg(MUTED),
        ));
    }
    for snapshot in snapshots {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}  ", snapshot.backend),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                snapshot.id.as_deref().unwrap_or("identifier unavailable"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
        if let Some(created) = snapshot.created_at {
            lines.push(Line::styled(
                format!("  {}", created.to_rfc3339()),
                Style::default().fg(MUTED),
            ));
        }
        lines.push(Line::raw(format!("  {}", snapshot.description)));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "s/Esc closes · j/k scroll",
        Style::default().fg(ACCENT),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.snapshots_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(section_block(" SNAPSHOT BACKEND · READ-ONLY ").padding(Padding::horizontal(1))),
        area,
    );
}

fn render_hook_inspector(
    frame: &mut Frame<'_>,
    app: &App,
    hooks: &[crate::domain::HookDefinition],
    area: Rect,
) {
    let mut lines = vec![
        Line::styled(
            "INSTALLED ALPM HOOKS",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "These are installed definitions. A Flight Plan separately matches hooks to evidenced transaction targets.",
            Style::default().fg(MUTED),
        ),
        Line::raw(""),
    ];
    if hooks.is_empty() {
        lines.push(Line::styled(
            "No hook definitions were found.",
            Style::default().fg(MUTED),
        ));
    }
    for hook in hooks {
        let stage = match hook.stage {
            HookStage::PreTransaction => "pre",
            HookStage::PostTransaction => "post",
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{stage:4} "),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                hook.description.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {}", hook.name), Style::default().fg(MUTED)),
        ]));
        lines.push(Line::raw(format!(
            "     operations: {}",
            display_list(&hook.operations)
        )));
        lines.push(Line::raw(format!(
            "     targets: {}",
            display_list(&hook.targets)
        )));
        if let Some(command) = &hook.command {
            lines.push(Line::styled(
                format!("     exec: {command}"),
                Style::default().fg(MUTED),
            ));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "Esc closes · j/k scroll",
        Style::default().fg(ACCENT),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.hooks_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(section_block(" HOOK INSPECTOR · READ-ONLY ").padding(Padding::horizontal(1))),
        area,
    );
}

fn plan_heading(lines: &mut Vec<Line<'static>>, heading: &str) {
    lines.push(Line::styled(
        heading.to_owned(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn render_transaction(frame: &mut Frame<'_>, transaction: &TransactionView, area: Rect) {
    if transaction.show_recovery
        && let Some(recovery) = &transaction.recovery
    {
        render_recovery(frame, recovery, area);
        return;
    }
    if transaction.show_summary && transaction.result.is_some() {
        render_transaction_summary(frame, transaction, area);
        return;
    }
    let running = matches!(
        transaction.phase,
        TransactionPhase::AcquiringPrivilege | TransactionPhase::Running
    );
    let sections = if running {
        Layout::vertical([Constraint::Min(5), Constraint::Length(3)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(5), Constraint::Length(0)]).split(area)
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Operation: ", Style::default().fg(MUTED)),
            Span::styled(
                transaction.request.label(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Command: ", Style::default().fg(MUTED)),
            Span::raw(transaction.command.join(" ")),
        ]),
    ];
    if let Some(status) = &transaction.snapshot_status {
        lines.push(Line::from(vec![
            Span::styled("Snapshot: ", Style::default().fg(MUTED)),
            Span::styled(status.clone(), Style::default().fg(ACCENT)),
        ]));
    }
    lines.push(Line::raw(""));
    for (stream, chunk) in &transaction.output {
        let base_style = match stream {
            crate::domain::OutputStream::Stdout => Style::default(),
            crate::domain::OutputStream::Stderr => Style::default().fg(Color::LightRed),
        };
        lines.extend(chunk.lines().map(|line| {
            let matched = transaction.search_query.as_ref().is_some_and(|query| {
                line.to_ascii_lowercase()
                    .contains(&query.to_ascii_lowercase())
            });
            Line::styled(
                line.to_owned(),
                if matched {
                    base_style.bg(Color::DarkGray).add_modifier(Modifier::BOLD)
                } else {
                    base_style
                },
            )
        }));
    }
    if matches!(transaction.phase, TransactionPhase::AcquiringPrivilege) {
        lines.push(Line::styled(
            "Waiting for sudo credential validation in the foreground terminal…",
            Style::default().fg(WARN),
        ));
    }
    if let Some(result) = &transaction.result {
        lines.push(Line::raw(""));
        if !result.hooks.is_empty() {
            lines.push(Line::styled(
                "ALPM HOOKS",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            for hook in &result.hooks {
                let failed = hook.status == crate::domain::HookExecutionStatus::Failed;
                let stage = match hook.stage {
                    crate::domain::HookExecutionStage::PreTransaction => "pre",
                    crate::domain::HookExecutionStage::PostTransaction => "post",
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        if failed { "✗ " } else { "✓ " },
                        Style::default().fg(if failed { Color::Red } else { GOOD }),
                    ),
                    Span::styled(format!("{stage:4} "), Style::default().fg(MUTED)),
                    Span::raw(hook.description.clone()),
                ]));
                if failed {
                    lines.extend(hook.output.iter().map(|output| {
                        Line::styled(
                            format!("       {output}"),
                            Style::default().fg(Color::LightRed),
                        )
                    }));
                }
            }
            lines.push(Line::raw(""));
        }
        let success = result.exit_code == Some(0) && !result.cancelled;
        lines.push(Line::styled(
            if success {
                "Transaction completed successfully. Refresh and health checks are running."
            } else {
                "Transaction did not complete successfully. Press v for the recovery summary."
            },
            Style::default()
                .fg(if success { GOOD } else { Color::Red })
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::styled(
            "o summary/raw · / search · y copy match/errors · Esc closes",
            Style::default().fg(MUTED),
        ));
    }
    if let Some(input) = &transaction.search_input {
        lines.push(Line::styled(
            format!("Search: /{input}_"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    } else if let Some(query) = &transaction.search_query {
        lines.push(Line::styled(
            format!("Search filter: /{query}"),
            Style::default().fg(ACCENT),
        ));
    }
    // Keep raw output one captured line per rendered row. Wrapping made the
    // previous follow calculation under-count visual rows and stop above the
    // newest output.
    let visible = sections[0].height.saturating_sub(2) as usize;
    let offset = if transaction.follow {
        lines.len().saturating_sub(visible).min(u16::MAX as usize) as u16
    } else {
        transaction.scroll
    };
    let paragraph = Paragraph::new(lines).block(section_block(format!(
        " TRANSACTION OUTPUT · {} · {} ",
        transaction_phase_label(transaction.phase),
        if transaction.follow {
            "following output"
        } else {
            "manual scroll · End resumes"
        }
    )));
    frame.render_widget(paragraph.scroll((offset, 0)), sections[0]);
    if running {
        frame.render_widget(
            Paragraph::new(format!("> {}", transaction.input)).block(section_block(
                " Package-manager input · Enter sends · Ctrl+C cancels ",
            )),
            sections[1],
        );
    }
}

fn render_transaction_summary(frame: &mut Frame<'_>, transaction: &TransactionView, area: Rect) {
    let Some(result) = transaction.result.as_ref() else {
        return;
    };
    let success = result.exit_code == Some(0) && !result.cancelled;
    let mut lines = vec![
        Line::styled(
            if success {
                "TRANSACTION COMPLETED"
            } else {
                "TRANSACTION FAILED / INTERRUPTED"
            },
            Style::default()
                .fg(if success { GOOD } else { Color::Red })
                .add_modifier(Modifier::BOLD),
        ),
        detail("Operation", transaction.request.label()),
        detail(
            "Exit code",
            result
                .exit_code
                .map_or_else(|| "signal".into(), |code| code.to_string()),
        ),
        detail("Cancelled", if result.cancelled { "yes" } else { "no" }),
        detail("Command", result.command.join(" ")),
        Line::raw(""),
    ];
    if let Some(status) = &transaction.snapshot_status {
        lines.push(detail("Snapshot", status));
        lines.push(Line::raw(""));
    }
    plan_heading(&mut lines, "ALPM HOOK OUTCOMES");
    if result.hooks.is_empty() {
        lines.push(Line::styled(
            "  No structured hook records were found in package-manager output.",
            Style::default().fg(MUTED),
        ));
    } else {
        for hook in &result.hooks {
            let failed = hook.status == crate::domain::HookExecutionStatus::Failed;
            lines.push(Line::styled(
                format!("  {} {}", if failed { "✗" } else { "✓" }, hook.description),
                Style::default().fg(if failed { Color::Red } else { GOOD }),
            ));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Post-transaction refresh and health checks run independently of package-manager exit status.",
        Style::default().fg(ACCENT),
    ));
    lines.push(Line::styled(
        "o returns to raw output · v opens recovery details when available · Esc closes",
        Style::default().fg(MUTED),
    ));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            section_block(" STRUCTURED TRANSACTION SUMMARY ").padding(Padding::horizontal(1)),
        ),
        area,
    );
}

fn render_recovery(frame: &mut Frame<'_>, recovery: &crate::domain::RecoveryReport, area: Rect) {
    let mut lines = vec![
        Line::styled(
            "UPDATE FAILED / INTERRUPTED",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::raw(recovery.summary.clone()),
        Line::raw(""),
        detail(
            "Package database lock present",
            if recovery.package_database_lock_present {
                "yes — first verify no package manager is active"
            } else {
                "no"
            },
        ),
        Line::raw(""),
        Line::styled(
            "COMPLETED PACKAGES",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!("  {}", display_list(&recovery.completed_packages))),
        Line::raw(""),
        Line::styled(
            "RELEVANT ERRORS",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ];
    lines.extend(
        recovery.relevant_errors.iter().map(|error| {
            Line::styled(format!("  • {error}"), Style::default().fg(Color::LightRed))
        }),
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "SUGGESTED NON-DESTRUCTIVE CHECKS",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
    lines.extend(
        recovery
            .suggested_checks
            .iter()
            .map(|check| Line::raw(format!("  • {check}"))),
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Press v to return to raw output; Esc closes the transaction view.",
        Style::default().fg(MUTED),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(section_block(" RECOVERY ASSISTANT ").padding(Padding::horizontal(1))),
        area,
    );
}

fn transaction_phase_label(phase: TransactionPhase) -> &'static str {
    match phase {
        TransactionPhase::AcquiringPrivilege => "acquiring privilege",
        TransactionPhase::Running => "running",
        TransactionPhase::Finished => "finished",
        TransactionPhase::FailedToStart => "failed to start",
    }
}

fn render_updates(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let sections = Layout::vertical([Constraint::Length(5), Constraint::Min(5)]).split(area);
    let summary = if app.loading.contains(&TaskKind::OfficialUpdates)
        || app.loading.contains(&TaskKind::AurUpdates)
    {
        "Checking repositories and configured AUR helper…".to_owned()
    } else {
        format!(
            "{} official upgrades  ·  {} AUR upgrades\n[u/Enter] Update selected   [a] Full system update\nOfficial targets remain full-sync transactions; AUR targets open PKGBUILD review first.",
            app.official_updates.len(),
            app.aur_updates.len()
        )
    };
    frame.render_widget(
        Paragraph::new(summary)
            .block(section_block(" SYSTEM UPDATES "))
            .wrap(Wrap { trim: true }),
        sections[0],
    );
    let mut rows = Vec::new();
    rows.extend(app.official_updates.iter().map(update_row));
    rows.extend(app.aur_updates.iter().map(update_row));
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(32),
            Constraint::Percentage(27),
            Constraint::Percentage(27),
        ],
    )
    .header(
        Row::new(["Source", "Package", "Installed", "Available"])
            .style(header_style())
            .bottom_margin(1),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▸ ")
    .block(section_block(" AVAILABLE UPDATES "));
    let mut state = TableState::default().with_selected(Some(
        app.selected_index()
            .min(app.official_updates.len() + app.aur_updates.len())
            .saturating_sub(0),
    ));
    frame.render_stateful_widget(table, sections[1], &mut state);
}

fn update_row(update: &PackageUpdate) -> Row<'static> {
    let row = Row::new([
        update.source.label().to_owned(),
        if update.ignored {
            format!("{}  [IGNORED]", update.name)
        } else {
            update.name.clone()
        },
        update.current_version.clone(),
        update.new_version.clone(),
    ]);
    if update.ignored {
        row.style(Style::default().fg(WARN))
    } else {
        row
    }
}

fn render_packages(frame: &mut Frame<'_>, app: &App, area: Rect, aur_only: bool) {
    let split = if app.inspected.is_some() {
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(area)
    };
    let packages = app.filtered_packages(aur_only);
    let title = if aur_only {
        " AUR SEARCH ".to_owned()
    } else {
        format!(
            " PACKAGES · FILTER: {} · f cycles ",
            app.package_filter.label()
        )
    };
    let rows = packages.iter().map(|package| {
        Row::new([
            package.source.label().to_owned(),
            package.name.clone(),
            package.version.clone(),
            if package.installed {
                "✓".into()
            } else {
                "—".into()
            },
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(40),
            Constraint::Percentage(34),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(["Source", "Package", "Version", "Installed"])
            .style(header_style())
            .bottom_margin(1),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▸ ")
    .block(section_block(title));
    let mut state = TableState::default().with_selected(
        (!packages.is_empty()).then(|| app.selected_index().min(packages.len() - 1)),
    );
    frame.render_stateful_widget(table, split[0], &mut state);
    if let Some(package) = &app.inspected {
        render_package_details(frame, app, package, split[1]);
    } else if packages.is_empty() {
        render_empty(
            frame,
            split[0],
            if app.loading.contains(&TaskKind::Search) {
                "Searching…"
            } else if aur_only {
                "Press / to search the AUR"
            } else {
                "Loading installed packages…"
            },
        );
    }
}

fn render_package_details(frame: &mut Frame<'_>, app: &App, package: &Package, area: Rect) {
    let mut lines = vec![
        detail("Name", &package.name),
        detail("Version", &package.version),
        detail("Source", package.source.label()),
        detail("Installed", if package.installed { "yes" } else { "no" }),
        detail("Reason", package.install_reason),
    ];
    if let Some(architecture) = &package.architecture {
        lines.push(detail("Architecture", architecture));
    }
    if let Some(description) = &package.description {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            description.clone(),
            Style::default().fg(Color::White),
        ));
    }
    lines.push(Line::raw(""));
    if let Some(size) = package.installed_size {
        lines.push(detail("Installed size", human_bytes(size)));
    }
    if let Some(size) = package.download_size {
        lines.push(detail("Download size", human_bytes(size)));
    }
    if let Some(date) = &package.install_date {
        lines.push(detail("Install date", date));
    }
    if let Some(url) = &package.url {
        lines.push(detail("Upstream", url));
    }
    if let Some(packager) = &package.packager {
        lines.push(detail("Packager", packager));
    }
    if let Some(aur) = &package.aur {
        if let Some(package_base) = &aur.package_base {
            lines.push(detail("Package base", package_base));
        }
        lines.push(detail(
            "Maintainer",
            aur.maintainer.as_deref().unwrap_or("orphaned"),
        ));
        lines.push(detail("Votes", aur.votes));
        lines.push(detail("Popularity", format!("{:.2}", aur.popularity)));
        if let Some(submitted) = aur.first_submitted {
            lines.push(detail("First submitted", submitted.to_rfc3339()));
        }
        if let Some(modified) = aur.last_modified {
            lines.push(detail("Last updated", modified.to_rfc3339()));
        }
        if aur.out_of_date.is_some() {
            lines.push(Line::styled(
                "! Flagged out of date",
                Style::default().fg(WARN),
            ));
        }
    }
    add_list(&mut lines, "Dependencies", &package.dependencies);
    add_list(
        &mut lines,
        "Optional dependencies",
        &package.optional_dependencies,
    );
    add_list(&mut lines, "Required by", &package.reverse_dependencies);
    add_list(&mut lines, "Conflicts", &package.conflicts);
    add_list(&mut lines, "Provides", &package.provides);
    add_list(&mut lines, "Replaces", &package.replaces);
    add_list(&mut lines, "Licenses", &package.licenses);
    add_list(&mut lines, "Groups", &package.groups);
    let package_history = app
        .history
        .iter()
        .flat_map(|transaction| {
            transaction
                .changes
                .iter()
                .filter(|change| change.name == package.name)
                .map(move |change| (transaction.started_at, change))
        })
        .take(5)
        .collect::<Vec<_>>();
    if !package_history.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "VERSION HISTORY",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        for (date, change) in package_history {
            let transition = match (&change.old_version, &change.new_version) {
                (Some(old), Some(new)) => format!("{old} → {new}"),
                (None, Some(new)) => format!("installed {new}"),
                (Some(old), None) => format!("removed {old}"),
                _ => String::new(),
            };
            lines.push(Line::raw(format!(
                "  {}  {transition}",
                date.format("%b %d")
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.inspector_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                section_block(" PACKAGE INSPECTOR · j/k scroll · Esc closes ")
                    .padding(Padding::horizontal(1)),
            ),
        area,
    );
}

fn detail(label: &str, value: impl ToString) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(MUTED)),
        Span::raw(value.to_string()),
    ])
}

fn add_list(lines: &mut Vec<Line<'static>>, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        title.to_uppercase(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
    lines.extend(values.iter().map(|value| Line::raw(format!("  {value}"))));
}

fn render_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let split =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area);
    let items = app
        .history
        .iter()
        .map(|transaction| {
            let status = if transaction.completed { "✓" } else { "!" };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{status} "),
                        Style::default().fg(if transaction.completed { GOOD } else { WARN }),
                    ),
                    Span::styled(
                        transaction.started_at.format("%b %d  %H:%M").to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::styled(
                    format!(
                        "  {} · {} package changes",
                        transaction.kind.label(),
                        transaction.changes.len()
                    ),
                    Style::default().fg(MUTED),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(section_block(" TRANSACTIONS "))
        .highlight_style(selected_style())
        .highlight_symbol("▸ ");
    let mut state = ratatui::widgets::ListState::default().with_selected(
        (!app.history.is_empty()).then(|| app.selected_index().min(app.history.len() - 1)),
    );
    frame.render_stateful_widget(list, split[0], &mut state);
    if let Some(transaction) = app.history.get(app.selected_index()) {
        render_history_details(frame, transaction, split[1]);
    } else {
        render_empty(
            frame,
            split[0],
            if app.loading.contains(&TaskKind::History) {
                "Parsing /var/log/pacman.log…"
            } else {
                "No package transactions found"
            },
        );
    }
}

fn render_history_details(frame: &mut Frame<'_>, transaction: &HistoryTransaction, area: Rect) {
    let mut lines = vec![
        detail("Started", transaction.started_at.to_rfc2822()),
        detail(
            "Completed",
            if transaction.completed {
                "yes"
            } else {
                "no — inspect logs"
            },
        ),
    ];
    if let Some(command) = &transaction.command_line {
        lines.push(detail("Command", command));
    }
    lines.push(Line::raw(""));
    for change in &transaction.changes {
        let marker = match change.action {
            PackageAction::Installed => "+",
            PackageAction::Removed => "−",
            _ => "↑",
        };
        let versions = match (&change.old_version, &change.new_version) {
            (Some(old), Some(new)) => format!("{old} → {new}"),
            (None, Some(new)) => new.clone(),
            (Some(old), None) => old.clone(),
            _ => String::new(),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(ACCENT)),
            Span::styled(
                change.name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {versions}")),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block(" TRANSACTION DETAIL ").padding(Padding::horizontal(1)))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_config(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let artifacts = app
        .health_report
        .as_ref()
        .map(|report| report.config_artifacts.as_slice())
        .unwrap_or_default();
    let rows = artifacts
        .iter()
        .map(|artifact| Row::new([artifact.kind.label().to_owned(), artifact.path.clone()]));
    let table = Table::new(rows, [Constraint::Length(12), Constraint::Min(20)])
        .header(
            Row::new(["Status", "File"])
                .style(header_style())
                .bottom_margin(1),
        )
        .row_highlight_style(selected_style())
        .highlight_symbol("▸ ")
        .block(section_block(" CONFIGURATION FILES · Enter reviews diff "));
    let mut state = TableState::default().with_selected(
        (!artifacts.is_empty()).then(|| app.selected_index().min(artifacts.len() - 1)),
    );
    frame.render_stateful_widget(table, area, &mut state);
    if artifacts.is_empty() {
        render_empty(
            frame,
            area,
            if app.loading.contains(&TaskKind::Health) {
                "Scanning /etc for Pacman configuration artifacts…"
            } else {
                "No .pacnew, .pacsave, or .pacorig files found"
            },
        );
    }
}

fn render_health(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(report) = &app.health_report else {
        render_empty(
            frame,
            area,
            if app.loading.contains(&TaskKind::Health) {
                "Running package, service, DKMS, configuration, and kernel checks…"
            } else {
                "Health report unavailable; press R to retry"
            },
        );
        return;
    };
    let split =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)]).split(area);
    let items = report
        .findings
        .iter()
        .map(|finding| {
            let (marker, color) = severity_marker(finding.severity);
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{marker} "), Style::default().fg(color)),
                    Span::styled(
                        finding.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::styled(
                    format!("  {}", finding.category.label()),
                    Style::default().fg(MUTED),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(section_block(format!(
            " HEALTH · {} actionable findings ",
            report.issue_count()
        )))
        .highlight_style(selected_style())
        .highlight_symbol("▸ ");
    let mut state = ratatui::widgets::ListState::default().with_selected(
        (!report.findings.is_empty()).then(|| app.selected_index().min(report.findings.len() - 1)),
    );
    frame.render_stateful_widget(list, split[0], &mut state);
    if let Some(finding) = report.findings.get(app.selected_index()) {
        let mut lines = vec![
            detail("Category", finding.category.label()),
            detail("Finding", &finding.title),
            Line::raw(""),
            Line::raw(finding.detail.clone()),
        ];
        if let Some(check) = &finding.suggested_check {
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "SUGGESTED CHECK",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::raw(check.clone()));
        }
        if !report.evidence_notes.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "UNAVAILABLE / LIMITED CHECKS",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ));
            lines.extend(
                report
                    .evidence_notes
                    .iter()
                    .map(|note| Line::raw(format!("• {note}"))),
            );
        }
        frame.render_widget(
            Paragraph::new(lines)
                .block(section_block(" ACTIONABLE DETAIL ").padding(Padding::horizontal(1)))
                .wrap(Wrap { trim: false }),
            split[1],
        );
    }
}

fn severity_marker(severity: FindingSeverity) -> (&'static str, Color) {
    match severity {
        FindingSeverity::Healthy => ("✓", GOOD),
        FindingSeverity::Advisory => ("i", ACCENT),
        FindingSeverity::Warning => ("!", WARN),
        FindingSeverity::Error => ("✗", Color::Red),
    }
}

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let left = if app.flight_plan.is_some() {
        " Enter/x Continue to confirmation   j/k Scroll   Esc Back".into()
    } else if let Some(notice) = app.latest_notice() {
        format!(" ! {notice}")
    } else {
        " / Search   : Commands   ? Help   R Refresh   q Quit".into()
    };
    let right = if app.loading.is_empty() {
        "READY"
    } else {
        "WORKING"
    };
    let width = area.width as usize;
    let padding = width.saturating_sub(left.chars().count() + right.len() + 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(left),
            Span::raw(" ".repeat(padding)),
            Span::styled(
                right,
                Style::default()
                    .fg(if app.loading.is_empty() { GOOD } else { ACCENT })
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::default().bg(Color::Rgb(22, 27, 34))),
        area,
    );
}

fn render_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = area.width.saturating_sub(8).min(80);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + area.height.saturating_sub(4),
        width,
        height: 3,
    };
    frame.render_widget(Clear, popup);
    let prefix = if app.input_mode == InputMode::Search {
        "/"
    } else {
        ":"
    };
    frame.render_widget(
        Paragraph::new(format!("{prefix}{}", app.input)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .title(if app.input_mode == InputMode::Search {
                    " Search · Enter to run · Esc to cancel "
                } else {
                    " Command · refresh · manifest-export · manifest-compare "
                }),
        ),
        popup,
    );
    frame.set_cursor_position((popup.x + 2 + app.input.chars().count() as u16, popup.y + 1));
}

fn render_transaction_confirmation(
    frame: &mut Frame<'_>,
    app: &App,
    pending: &crate::app::PendingTransaction,
    area: Rect,
) {
    let popup = centered_rect(94, 84, area);
    frame.render_widget(Clear, popup);
    let text = vec![
        Line::styled(
            pending.request.label(),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        detail("Exact command arguments", pending.command.join(" ")),
        Line::raw(""),
        Line::raw("This operation changes packages and performs foreground sudo validation."),
        Line::raw("The TUI stays unprivileged; only package-manager subprocesses obtain root."),
        Line::raw("The package manager remains interactive; --noconfirm is never added."),
        Line::raw(
            if app.snapshot_before_upgrade && pending.request.supports_pre_snapshot() {
                "A configured pre-transaction snapshot will be created first; snapshot failure blocks the package operation."
            } else {
                "No automatic pre-transaction snapshot is configured for this operation."
            },
        ),
        Line::raw(""),
        Line::styled(
            "Proceed?  y yes   n/Esc cancel",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(WARN))
                .title(" Explicit transaction confirmation ")
                .padding(Padding::uniform(1)),
        ),
        popup,
    );
}

fn render_maintenance_confirmation(
    frame: &mut Frame<'_>,
    action: crate::app::MaintenanceAction,
    area: Rect,
) {
    let popup = centered_rect(80, 58, area);
    frame.render_widget(Clear, popup);
    let text = vec![
        Line::styled(
            action.label(),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::raw("The TUI will suspend and run sudo -- pacdiff in the foreground."),
        Line::raw(
            "pacdiff may keep, replace, remove, or launch a merge tool only in response to your input.",
        ),
        Line::raw(
            "arch-maint will not choose an answer or merge configuration files automatically.",
        ),
        Line::raw(""),
        Line::styled(
            "Proceed?  y yes   n/Esc cancel",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(WARN))
                .title(" Explicit configuration-action confirmation ")
                .padding(Padding::uniform(1)),
        ),
        popup,
    );
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(66, 72, area);
    frame.render_widget(Clear, popup);
    let text = Text::from(vec![
        Line::styled(
            "KEYBOARD",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        detail("Tab / Shift+Tab", "change major view"),
        detail("j / k, ↑ / ↓", "move selection"),
        detail("/", "search (Packages searches official + AUR)"),
        detail("Enter", "inspect package"),
        detail("i", "plan official install with a full upgrade"),
        detail("r", "simulate removal of an installed package"),
        detail("d", "open bounded dependency explorer"),
        detail(
            "c in Packages",
            "inspect orphans, foreign packages, and cache",
        ),
        detail(
            "u / Enter in Updates",
            "update highlighted target (official targets keep full sync)",
        ),
        detail(
            "a in Updates",
            "generate the full-system upgrade Flight Plan",
        ),
        detail("u on AUR update", "review PKGBUILD before helper execution"),
        detail("n in Updates", "open official Arch Linux news feed"),
        detail(
            "Enter / x in plan",
            "continue to explicit transaction confirmation",
        ),
        detail("Esc", "close inspector or cancel input"),
        detail("R / F5", "refresh read-only system data"),
        detail(":", "command palette"),
        detail(
            ":manifest-export",
            "write explicit package manifest to XDG state",
        ),
        detail(
            ":manifest-compare",
            "show read-only drift from saved manifest",
        ),
        detail(":snapshots", "list snapshots without privilege escalation"),
        detail(":hooks", "inspect installed ALPM hook definitions"),
        detail("q", "quit"),
        Line::raw(""),
        Line::styled(
            "TRANSACTION SAFETY",
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        Line::raw(
            "The TUI stays unprivileged. Full upgrades require plan review, confirmation, and scoped sudo validation.",
        ),
        Line::raw("Press any key to close."),
    ]);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(ACCENT))
                .title(" Help ")
                .padding(Padding::uniform(1)),
        ),
        popup,
    );
}

fn render_empty(frame: &mut Frame<'_>, area: Rect, message: &str) {
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 2,
    });
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED)),
        inner,
    );
}

fn section_block(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(
            title
                .into()
                .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        )
}

fn header_style() -> Style {
    Style::default().fg(MUTED).add_modifier(Modifier::BOLD)
}
fn selected_style() -> Style {
    Style::default()
        .bg(Color::Rgb(32, 60, 70))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn human_signed_bytes(bytes: i64) -> String {
    if bytes >= 0 {
        format!("+{}", human_bytes(bytes as u64))
    } else {
        format!("−{}", human_bytes(bytes.unsigned_abs()))
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::{DemoPackageBackend, FlightPlanBackend, PackageBackend, demo_system_profile},
        domain::UpdateSet,
    };
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn formats_sizes() {
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(5), "5 B");
    }

    #[tokio::test]
    async fn renders_transaction_flight_plan_at_target_size() {
        let backend = DemoPackageBackend;
        let installed = backend.installed_packages().await.expect("demo packages");
        let updates = UpdateSet::demo();
        let plan = backend
            .build_flight_plan(&updates.official, &updates.aur, &installed)
            .await
            .expect("demo flight plan");
        let mut app = App::new(demo_system_profile(), true);
        app.flight_plan = Some(plan);
        app.flight_plan_request = Some(TransactionRequest::SystemUpgrade);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("TRANSACTION FLIGHT PLAN"));
        assert!(rendered.contains("Enter/x Continue to confirmation"));
        assert!(rendered.contains("SYSTEM UPGRADE"));
        assert!(rendered.contains("Kernel update"));
    }

    #[tokio::test]
    async fn aur_flight_plan_names_the_reviewed_helper_target() {
        let backend = DemoPackageBackend;
        let installed = backend.installed_packages().await.expect("demo packages");
        let updates = UpdateSet::demo();
        let plan = backend
            .build_flight_plan(&updates.official, &updates.aur, &installed)
            .await
            .expect("demo flight plan");
        let mut app = App::new(demo_system_profile(), true);
        app.flight_plan = Some(plan);
        app.flight_plan_request = Some(TransactionRequest::AurInstall {
            packages: vec!["new-aur-package".into()],
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("AUR HELPER TARGETS"));
        assert!(rendered.contains("new-aur-package"));
        assert!(rendered.contains("full -Syu transaction"));
    }

    #[test]
    fn confirmation_prompt_is_actionable_at_eighty_by_twenty_four() {
        let mut app = App::new(demo_system_profile(), true);
        app.pending_transaction = Some(crate::app::PendingTransaction {
            request: TransactionRequest::AurInstall {
                packages: vec!["visual-studio-code-bin".into()],
            },
            command: vec![
                "paru".into(),
                "-Syu".into(),
                "--needed".into(),
                "--".into(),
                "visual-studio-code-bin".into(),
            ],
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Proceed?"));
        assert!(rendered.contains("y yes"));
    }

    #[test]
    fn running_transaction_follows_the_latest_output_line() {
        let mut app = App::new(demo_system_profile(), true);
        let output = (0..50)
            .map(|index| format!("transaction-line-{index:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.transaction = Some(TransactionView {
            request: TransactionRequest::SystemUpgrade,
            command: vec!["sudo".into(), "pacman".into(), "-Syu".into()],
            phase: TransactionPhase::Running,
            stdout: output.clone(),
            stderr: String::new(),
            result: None,
            recovery: None,
            follow: true,
            show_recovery: false,
            scroll: 0,
            input: String::new(),
            output: vec![(crate::domain::OutputStream::Stdout, output)],
            show_summary: false,
            search_input: None,
            search_query: None,
            snapshot: None,
            snapshot_status: None,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("following output"));
        assert!(rendered.contains("transaction-line-49"));
        assert!(!rendered.contains("transaction-line-00"));
    }
}
