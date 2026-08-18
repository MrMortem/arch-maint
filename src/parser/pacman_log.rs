use crate::domain::{HistoryTransaction, PackageAction, PackageChange, TransactionKind};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};

pub fn parse_pacman_log(input: &str) -> Result<Vec<HistoryTransaction>> {
    let mut transactions = Vec::new();
    let mut current: Option<HistoryTransaction> = None;
    let mut last_command: Option<String> = None;

    for line in input.lines() {
        let Some((timestamp, component, message)) = parse_line(line) else {
            continue;
        };
        if component == "PACMAN" && message.starts_with("Running '") {
            last_command = Some(
                message
                    .trim_start_matches("Running '")
                    .trim_end_matches('\'')
                    .to_owned(),
            );
            continue;
        }
        if component != "ALPM" {
            continue;
        }
        if message == "transaction started" {
            if let Some(previous) = current.take() {
                transactions.push(finalize(previous));
            }
            current = Some(HistoryTransaction {
                started_at: timestamp,
                completed: false,
                kind: TransactionKind::Mixed,
                command_line: last_command.take(),
                changes: Vec::new(),
            });
            continue;
        }
        if message == "transaction completed" {
            if let Some(mut transaction) = current.take() {
                transaction.completed = true;
                transactions.push(finalize(transaction));
            }
            continue;
        }
        if let Some(change) = parse_change(message) {
            if current.is_none() {
                current = Some(HistoryTransaction {
                    started_at: timestamp,
                    completed: false,
                    kind: TransactionKind::Mixed,
                    command_line: last_command.take(),
                    changes: Vec::new(),
                });
            }
            if let Some(transaction) = &mut current {
                transaction.changes.push(change);
            }
        }
    }
    if let Some(transaction) = current {
        transactions.push(finalize(transaction));
    }
    transactions.retain(|transaction| !transaction.changes.is_empty());
    transactions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(transactions)
}

fn parse_line(line: &str) -> Option<(DateTime<FixedOffset>, &str, &str)> {
    let after_open = line.strip_prefix('[')?;
    let (timestamp, rest) = after_open.split_once("] [")?;
    let (component, message) = rest.split_once("] ")?;
    let timestamp = DateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S%z")
        .or_else(|_| DateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .with_context(|| format!("invalid pacman log timestamp `{timestamp}`"))
        .ok()?;
    Some((timestamp, component, message))
}

fn parse_change(message: &str) -> Option<PackageChange> {
    let (action, rest) = [
        ("installed ", PackageAction::Installed),
        ("upgraded ", PackageAction::Upgraded),
        ("downgraded ", PackageAction::Downgraded),
        ("reinstalled ", PackageAction::Reinstalled),
        ("removed ", PackageAction::Removed),
    ]
    .into_iter()
    .find_map(|(prefix, action)| message.strip_prefix(prefix).map(|rest| (action, rest)))?;
    let open = rest.rfind(" (")?;
    let name = rest[..open].to_owned();
    let versions = rest[open + 2..].strip_suffix(')')?;
    let (old_version, new_version) = match action {
        PackageAction::Installed => (None, Some(versions.to_owned())),
        PackageAction::Removed => (Some(versions.to_owned()), None),
        _ => {
            let (old, new) = versions.split_once(" -> ")?;
            (Some(old.to_owned()), Some(new.to_owned()))
        }
    };
    Some(PackageChange {
        action,
        name,
        old_version,
        new_version,
    })
}

fn finalize(mut transaction: HistoryTransaction) -> HistoryTransaction {
    let installed = transaction
        .changes
        .iter()
        .any(|c| c.action == PackageAction::Installed);
    let removed = transaction
        .changes
        .iter()
        .any(|c| c.action == PackageAction::Removed);
    let upgraded = transaction.changes.iter().any(|c| {
        matches!(
            c.action,
            PackageAction::Upgraded | PackageAction::Downgraded | PackageAction::Reinstalled
        )
    });
    let is_system_upgrade = transaction.command_line.as_deref().is_some_and(|command| {
        command.split_whitespace().any(|argument| {
            argument == "--sysupgrade"
                || (argument.starts_with('-')
                    && argument.contains('S')
                    && argument.contains('y')
                    && argument.contains('u'))
        })
    });
    transaction.kind = match (installed, removed, upgraded) {
        (false, false, true) if is_system_upgrade => TransactionKind::SystemUpgrade,
        (false, false, true) => TransactionKind::Upgrade,
        (true, false, false) => TransactionKind::Install,
        (false, true, false) => TransactionKind::Remove,
        _ => TransactionKind::Mixed,
    };
    transaction
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"[2025-01-12T10:30:00-0500] [PACMAN] Running 'pacman -Syu'
[2025-01-12T10:30:01-0500] [ALPM] transaction started
[2025-01-12T10:30:02-0500] [ALPM] upgraded linux (6.12.7.arch1-1 -> 6.12.8.arch1-1)
[2025-01-12T10:30:03-0500] [ALPM] upgraded mesa (24.3.2-1 -> 24.3.3-1)
[2025-01-12T10:30:04-0500] [ALPM] transaction completed
"#;

    #[test]
    fn groups_completed_transactions() {
        let history = parse_pacman_log(LOG).expect("log should parse");
        assert_eq!(history.len(), 1);
        assert!(history[0].completed);
        assert_eq!(history[0].kind, TransactionKind::SystemUpgrade);
        assert_eq!(history[0].changes[0].name, "linux");
        assert_eq!(history[0].command_line.as_deref(), Some("pacman -Syu"));
    }

    #[test]
    fn preserves_incomplete_transaction() {
        let log = "[2025-01-12T10:30:01-0500] [ALPM] transaction started\n[2025-01-12T10:30:02-0500] [ALPM] installed foo (1.0-1)\n";
        let history = parse_pacman_log(log).expect("log should parse");
        assert!(!history[0].completed);
        assert_eq!(history[0].kind, TransactionKind::Install);
    }

    #[test]
    fn does_not_invent_system_upgrade_from_change_count() {
        let log = "[2025-01-12T10:30:00-0500] [PACMAN] Running 'pacman -S foo bar'\n\
[2025-01-12T10:30:01-0500] [ALPM] transaction started\n\
[2025-01-12T10:30:02-0500] [ALPM] upgraded foo (1.0-1 -> 1.1-1)\n\
[2025-01-12T10:30:03-0500] [ALPM] upgraded bar (1.0-1 -> 1.1-1)\n\
[2025-01-12T10:30:04-0500] [ALPM] transaction completed\n";
        let history = parse_pacman_log(log).expect("log should parse");
        assert_eq!(history[0].kind, TransactionKind::Upgrade);
    }
}
