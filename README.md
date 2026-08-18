# arch-maint

`arch-maint` is a keyboard-first Arch Linux system-maintenance and package-management TUI. Its long-term focus is transaction understanding and recovery:

> Show the user what an update will do before it happens, then verify the system after it happens.

The 0.1 release includes the read-only foundation, Transaction Flight Plan, guarded package operations, health/recovery workflow, PKGBUILD review, dependency/removal analysis, optional snapshots, configuration review, and package manifests. All slow process and network work runs outside the render loop.

## Screenshot

<!-- Replace with an asciinema/GIF capture after the first tagged release. -->

```text
╭ System · UNPRIVILEGED ────────────────────────────────────────────────╮
│ ✓ Arch Linux detected                 ! 12 official updates           │
│ ✓ pacman available                    ✓ No AUR updates found           │
│ ✓ safe update checks                  ✓ pacdiff available              │
╰──────────────────────────────────────────────────────────────────────╯
 Updates   Packages   AUR   Config   Health   History
╭ SYSTEM UPDATES ───────────────────────────────────────────────────────╮
│ 12 official upgrades · 0 AUR upgrades                                │
│ [u/Enter] Update selected   [a] Full system update                    │
│ Official targets remain full-sync transactions.                      │
├──────────────────────────────────────────────────────────────────────┤
│ Source       Package             Installed          Available         │
│ ▸ core       linux               6.12.8.arch1-1     6.12.9.arch1-1   │
╰──────────────────────────────────────────────────────────────────────╯
 / Search   : Commands   ? Help   R Refresh   q Quit             READY
```

## Current features

- Full-screen Ratatui interface designed for terminals around 100×30 and larger
- Arch Linux and root-session detection
- Capability detection for `pacman`, `checkupdates`, `pacdiff`, `paru`, `yay`, Snapper, and Timeshift
- Installed package listing with install reason, sizes, dependencies, reverse dependencies, conflicts, provides, and package metadata
- Unified official repository and AUR search, plus a focused AUR view
- Fuzzy ordering of search results
- AUR RPC v5 metadata including maintainer, votes, popularity, dependencies, and out-of-date status
- Official update checks using `checkupdates`, with a read-only `pacman -Qu` fallback
- AUR update checks through an automatically or explicitly selected `paru`/`yay` backend
- Structured transaction history parsed from `/var/log/pacman.log`, including incomplete transactions
- Read-only Transaction Flight Plan with official/AUR actions, download and disk deltas where evidenced, package additions/replacements, Pacman holds/ignores, expected package-triggered ALPM hooks, and explicit evidence limits
- Explainable attention findings for kernels, boot packages, graphics, systemd, glibc, Pacman, Python ABI changes, DKMS, modified backup files, replacements/removals, and AUR runtime rebuild candidates—without opaque risk scores
- Async process/network jobs, stale-search suppression, loading indicators, and contextual errors
- Concurrent read-only health report for package database and package-owned-file consistency, failed system/user services, DKMS status, orphan/foreign inventories, running-kernel module-tree consistency, and processes with deleted shared-library mappings
- `.pacnew`, `.pacsave`, and `.pacorig` discovery under `/etc`, surfaced without automatic edits or merges
- Explicit full-upgrade execution gated by a generated Flight Plan, a second confirmation, and foreground `sudo -v` credential validation
- Highlighted official-package updates that remain safely coupled to a complete `pacman -Syu` transaction
- Interactive streamed Pacman stdout/stderr with prompt input, process-group cancellation, structured completion, and automatic data/health refresh
- Automatic transaction-output following, with manual scrolling and one-key follow resumption
- Recovery summary that extracts completed packages, relevant errors, lock state, and non-destructive suggested checks
- Official installs coupled to a complete `pacman -Syu`, never a partial-upgrade workflow
- Pacman-backed removal simulation with direct removals, newly unused dependencies, dependency blockers, and reclaimed size before `-Rs` can be requested
- AUR install/update execution through the selected helper only after PKGBUILD review and a separate transaction confirmation
- PKGBUILD helper-cache baseline discovery, unified diff, and explainable dependency/source/domain/checksum/build/install/shell-command change findings
- Bounded, cycle-aware dependency and reverse-dependency explorer with explicit-root paths and orphan candidates
- Structured pre/post ALPM hook outcomes extracted from transaction output, while preserving the raw stream
- Read-only installed ALPM hook inspector with parsed stages, triggers, targets, and commands
- Read-only `.pacnew`/`.pacsave`/`.pacorig` current/artifact/diff review plus an explicitly confirmed foreground `pacdiff` workflow
- Optional pre-upgrade Snapper or Timeshift snapshot creation and read-only snapshot listing; snapshot failure blocks the package transaction
- Explicit-package manifest export and read-only drift comparison in XDG state storage, with unverified foreign packages kept separate from known AUR packages
- Installed-package hygiene report covering explicit/dependency/orphan-candidate/foreign packages and a preview of cached package versions
- Optional official Arch News reader, clearly presented as informational rather than an applicability claim
- Searchable raw transaction output, structured/raw switching, relevant-error copying through size-bounded OSC 52, and scroll/follow controls
- Deterministic `--demo` mode for UI development on any Linux system
- XDG configuration and external diagnostic logging

No cleanup, rollback, configuration merge, orphan removal, or manifest reconciliation is automatic. Every package-changing or pacdiff workflow requires a dedicated preview/review and explicit confirmation.

## Architecture

The crate keeps terminal rendering, state transitions, domain objects, parsing, and external I/O separate:

```text
src/
├── main.rs              terminal lifecycle and async task orchestration
├── app.rs               UI state and keyboard-driven state transitions
├── event.rs             typed messages from background work
├── ui/                  pure Ratatui rendering
├── analysis/            flight plans, dependency graphs, and recovery evidence
├── domain/              package, transaction, health, snapshot, and manifest models
├── backend/             Pacman, AUR/helper, health, snapshot, pacdiff, demo data
├── parser/              Pacman/log/hook/PKGBUILD/transaction-output parsers
└── config.rs            XDG config/state discovery
```

Package, AUR, history, flight-plan, health, transaction, removal, configuration-file, and snapshot operations are injectable traits. The UI never parses raw command output. AUR helpers and snapshot providers are optional backend capabilities, leaving room for other implementations and future direct `libalpm` support.

All child processes are constructed with explicit program arguments. User input is never interpolated into a shell command, and query/package validation rejects option-like values.

## Build and run

Requires Rust 1.88 or later (the minimum supported version of the current Ratatui release). From the project directory, compile and launch the optimized application with:

```bash
cargo run --release
```

Explore with sample data on any Linux system:

```bash
cargo run --release -- --demo
```

`cargo build --release` also produces the standalone binary at `target/release/arch-maint`.

Development checks:

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo audit
```

## Runtime requirements

- Arch Linux (target platform)
- `pacman`
- A regular, non-root user session
- `sudo` for explicitly confirmed package transactions
- Network access for AUR search

Optional tools:

- `checkupdates` and `pacdiff` from `pacman-contrib`
- `paru` or `yay` for installed AUR update checks and AUR transactions
- Snapper or Timeshift for optional pre-transaction snapshots

Without `checkupdates`, the application uses `pacman -Qu`, which only examines the currently synchronized package database and may therefore be stale. Missing optional tools are reported without preventing startup.

## Keyboard controls

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab` | Change major view |
| `j` / `k`, arrows | Move selection |
| `/` | Search; the Packages tab searches official repositories and AUR |
| `Enter` | Inspect a package; review a config artifact; review a selected AUR update |
| `Esc` | Close inspector or cancel input |
| `f` | Cycle all/installed/official/AUR filters in Packages |
| `R` / `F5` | Refresh read-only system data |
| `u` / `Enter` in Updates | Update the highlighted target; official targets use a safe full-sync transaction |
| `a` in Updates | Generate the complete system-upgrade Flight Plan |
| `u` / `Enter` on an AUR update | Fetch metadata and open PKGBUILD change review |
| `Enter` / `x` in the Flight Plan | Continue to the explicit transaction confirmation |
| `i` on a package | Plan an official install with full upgrade, or review an AUR PKGBUILD |
| `r` on an installed package | Run the Pacman removal simulator |
| `d` on an installed package | Open the bounded dependency explorer |
| `c` in Packages | Open the read-only package/cache hygiene report |
| `n` in Updates | Open Arch News when enabled in configuration |
| `p` in config review | Request confirmed foreground `pacdiff` reconciliation |
| `Ctrl+C` during a transaction | Signal the isolated package-manager process group |
| `j` / `k`, then `End` in transaction output | Pause automatic following to scroll; resume following at the bottom |
| `o`, `/`, `y` in transaction output | Toggle structured/raw output, search, and copy the match or relevant errors |
| `:manifest-export` | Export explicitly installed desired state to XDG state storage |
| `:manifest-compare` | Compare the current system with the saved manifest |
| `:hooks` | Inspect installed ALPM hook definitions |
| `:snapshots` | List snapshots from the configured backend without privilege escalation |
| `?` | Show help |
| `q` | Quit |

## Configuration and logs

Configuration is loaded from `$XDG_CONFIG_HOME/arch-maint/config.toml`, normally `~/.config/arch-maint/config.toml`. See [`config.example.toml`](config.example.toml). Unknown or absent optional commands are handled gracefully.

Logs are written to `$XDG_STATE_HOME/arch-maint/arch-maint.log`, normally `~/.local/state/arch-maint/arch-maint.log`, so diagnostic output does not corrupt the terminal interface. The application does not log credentials.

## Security and privilege model

The TUI runs unprivileged and refuses to start as root outside demo mode. Read operations never invoke `sudo`. Package operations require a dedicated review/simulation, explicit execution request, and a second confirmation. The interface temporarily leaves the alternate screen for `sudo -v`; Pacman children use `sudo -n` with explicit arguments. AUR helpers run as the user and invoke their normal scoped privilege path. Pacman/helpers stay interactive and `--noconfirm` is never added.

Official install command construction is coupled to `pacman -Syu`, preserving the full-upgrade workflow. Removal uses Pacman's print-only simulation before the exact `-Rs` request is offered. The application never edits `pacman.conf`, automatically removes orphans, chooses a pacdiff response, reconciles a manifest, or rolls back a snapshot.

AUR packages are user-produced content. Metadata display and PKGBUILD diff/classification are change-inspection aids and do **not** guarantee package safety. When no prior helper-cache PKGBUILD exists, the app shows the current file and explicitly makes no change claims. Users remain responsible for reviewing PKGBUILDs, sources, and helper output.

## Current limitations

- Conflict-driven removals that Pacman's print-only format does not expose remain explicitly unknown; direct libalpm integration is planned to close this evidence gap.
- AUR upgrades are reviewed and executed one selected package at a time; there is no bulk “approve every PKGBUILD” shortcut.
- Config inspection is native and read-only; keep/replace/merge/editor semantics are delegated to confirmed foreground `pacdiff` rather than reimplemented.
- Snapshot creation and read-only listing are supported, but rollback planning and automatic rollback are not. Snapper snapshots are standalone snapshots created immediately before a transaction rather than an unpaired Snapper `pre` record. Timeshift creation is recorded without inventing an identifier because its scripted output is not treated as a stable machine API.
- Manifest comparison is read-only. Reconciliation is intentionally unavailable.
- Package-cache and old-version information is preview-only; no cache deletion action is exposed. Cache discovery currently covers Pacman's default cache directory rather than custom `CacheDir` values.
- Service-restart findings are conservative: they report observed processes mapping deleted shared libraries and do not claim that every affected service has been identified.
- The lightweight Arch News feed parser handles the official RSS structure but is not a general-purpose XML implementation.
- Official update checks do not refresh the real sync database. `checkupdates` uses its isolated temporary database; the fallback reflects the existing sync database.
- AUR update checks require `paru` or `yay`; AUR search itself does not.
- Transaction classification is derived from ALPM log events and does not claim details absent from the log.
- Very dense package names and inspector fields may be clipped in an 80-column terminal; the main workflows are designed and tested down to 80×24, with 100×30 recommended.

## Roadmap

1. **Foundation — complete:** event loop, navigation, models, traits, config, logging, demo backend.
2. **Read-only packages — complete:** installed packages, repository/AUR search, inspector, update checks, transaction history.
3. **Transaction Flight Plan — substantially implemented:** full-upgrade/install preview, evidenced totals, replacements, holds/ignores, hook matching, and reason-based findings; direct libalpm would improve conflict/removal evidence.
4. **Package operations — implemented with safety gates:** official/AUR install, removal, full upgrade, explicit confirmation, scoped privilege, cancellation, raw streaming, hook outcomes, and recovery summaries.
5. **Maintenance lifecycle — substantially implemented:** configuration discovery/review, confirmed pacdiff delegation, systemd/DKMS/package/kernel checks, orphan discovery, post-run health refresh, and actionable findings.
6. **Advanced differentiators — substantially implemented:** PKGBUILD and related install-script review, dependency/removal graphs, snapshot creation/listing, manifests, package hygiene, service-restart evidence, hooks, Arch News, output search/copy, and recovery diagnostics are present. Native three-way merge tooling, rollback planning, safe manifest reconciliation, and direct libalpm evidence remain future work.
