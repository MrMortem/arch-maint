use crate::domain::{RecoveryReport, TransactionResult};

pub fn analyze_transaction_failure(
    result: &TransactionResult,
    package_database_lock_present: bool,
) -> RecoveryReport {
    let mut relevant_errors = result
        .stderr
        .lines()
        .chain(result.stdout.lines())
        .filter(|line| {
            let lowercase = line.to_ascii_lowercase();
            lowercase.contains("error:")
                || lowercase.contains("failed")
                || lowercase.contains("fatal")
        })
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    relevant_errors.sort();
    relevant_errors.dedup();
    relevant_errors.truncate(20);

    let mut completed_packages = result
        .stdout
        .lines()
        .filter_map(completed_package)
        .collect::<Vec<_>>();
    completed_packages.sort();
    completed_packages.dedup();

    let combined = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
    let mut suggested_checks = Vec::new();
    if combined.contains("dkms") {
        suggested_checks.push("Inspect DKMS output and verify matching kernel headers.".into());
    }
    if combined.contains("could not lock database") || package_database_lock_present {
        suggested_checks.push(
            "Check whether another package manager is active before investigating /var/lib/pacman/db.lck."
                .into(),
        );
    }
    if combined.contains("invalid or corrupted package") || combined.contains("signature") {
        suggested_checks.push("Inspect the named package/signature error and mirror state; do not disable signature verification.".into());
    }
    if combined.contains("failed to commit transaction") {
        suggested_checks.push("Inspect the complete transaction output and run a read-only package database health check.".into());
    }
    if suggested_checks.is_empty() {
        suggested_checks.push(
            "Inspect the complete stderr/stdout stream and the Pacman log before retrying.".into(),
        );
    }
    let summary = if result.cancelled {
        "Transaction was cancelled before normal completion.".into()
    } else {
        match result.exit_code {
            Some(code) => format!("Package manager exited with status {code}."),
            None => "Package manager terminated without an exit status.".into(),
        }
    };
    RecoveryReport {
        summary,
        relevant_errors,
        completed_packages,
        package_database_lock_present,
        suggested_checks,
    }
}

fn completed_package(line: &str) -> Option<String> {
    const ACTIONS: &[&str] = &[
        "upgrading ",
        "installing ",
        "removing ",
        "reinstalling ",
        "downgrading ",
    ];
    let lowercase = line.to_ascii_lowercase();
    for action in ACTIONS {
        if let Some(index) = lowercase.find(action) {
            let rest = line[index + action.len()..].trim();
            let name = rest.split_whitespace().next()?.trim_matches(['(', ')']);
            if !name.is_empty() {
                return Some(name.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_recovery_evidence_without_prescribing_destructive_action() {
        let result = TransactionResult {
            command: vec!["sudo".into(), "-n".into(), "pacman".into(), "-Syu".into()],
            exit_code: Some(1),
            stdout: "(1/2) upgrading linux\n(2/2) upgrading nvidia-dkms\n".into(),
            stderr: "error: command failed to execute correctly\nDKMS build failed\n".into(),
            cancelled: false,
            hooks: Vec::new(),
        };
        let report = analyze_transaction_failure(&result, false);
        assert_eq!(report.completed_packages, ["linux", "nvidia-dkms"]);
        assert!(
            report
                .suggested_checks
                .iter()
                .any(|check| check.contains("DKMS"))
        );
        assert!(
            !report
                .suggested_checks
                .iter()
                .any(|check| check.contains("remove"))
        );
    }
}
