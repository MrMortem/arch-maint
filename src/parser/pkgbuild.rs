use crate::domain::{PkgbuildChangeKind, PkgbuildFinding, PkgbuildReview};
use std::collections::{BTreeMap, BTreeSet};

const DEPENDENCY_FIELDS: &[&str] = &[
    "depends",
    "makedepends",
    "checkdepends",
    "optdepends",
    "provides",
    "conflicts",
];
const CHECKSUM_FIELDS: &[&str] = &[
    "md5sums",
    "sha1sums",
    "sha224sums",
    "sha256sums",
    "sha384sums",
    "sha512sums",
    "b2sums",
];

pub fn review_pkgbuild(
    package: impl Into<String>,
    baseline: Option<(&str, &str)>,
    current: String,
) -> PkgbuildReview {
    let package = package.into();
    let (baseline_source, old) = baseline
        .map(|(source, content)| (Some(source.to_owned()), content))
        .unwrap_or((None, ""));
    let findings = baseline_source
        .as_ref()
        .map(|_| classify_changes(old, &current))
        .unwrap_or_default();
    let unified_diff = if baseline_source.is_some() {
        unified_diff(old, &current, "previous/PKGBUILD", "current/PKGBUILD")
    } else {
        String::new()
    };
    PkgbuildReview {
        package,
        baseline_source,
        current_pkgbuild: current,
        unified_diff,
        findings,
        related_files: Vec::new(),
        evidence_notes: Vec::new(),
    }
}

pub fn pkgbuild_install_script(content: &str) -> Option<String> {
    let value = assignments(content, &["install"]).remove("install")?;
    let value = value.trim().trim_matches(['\'', '"']);
    (!value.is_empty()).then(|| value.to_owned())
}

fn classify_changes(old: &str, new: &str) -> Vec<PkgbuildFinding> {
    let mut findings = Vec::new();
    compare_assignments(
        old,
        new,
        DEPENDENCY_FIELDS,
        PkgbuildChangeKind::Dependencies,
        "Dependency-related assignments differ.",
        &mut findings,
    );
    compare_assignments(
        old,
        new,
        &["source"],
        PkgbuildChangeKind::Sources,
        "The source assignment differs.",
        &mut findings,
    );
    let old_domains = source_domains(old);
    let new_domains = source_domains(new);
    let added_domains = new_domains
        .difference(&old_domains)
        .cloned()
        .collect::<Vec<_>>();
    if !added_domains.is_empty() {
        findings.push(PkgbuildFinding {
            kind: PkgbuildChangeKind::NewSourceDomain,
            detail: format!("New source host(s): {}", added_domains.join(", ")),
        });
    }
    compare_assignments(
        old,
        new,
        CHECKSUM_FIELDS,
        PkgbuildChangeKind::Checksums,
        "One or more checksum arrays differ.",
        &mut findings,
    );
    compare_function(
        old,
        new,
        "build",
        PkgbuildChangeKind::BuildCommands,
        &mut findings,
    );
    compare_function(
        old,
        new,
        "package",
        PkgbuildChangeKind::InstallCommands,
        &mut findings,
    );
    compare_function(
        old,
        new,
        "check",
        PkgbuildChangeKind::CheckCommands,
        &mut findings,
    );
    compare_assignments(
        old,
        new,
        &["install"],
        PkgbuildChangeKind::InstallScript,
        "The install-script assignment differs.",
        &mut findings,
    );

    let old_commands = shell_commands(old);
    let new_commands = shell_commands(new);
    let added = new_commands.difference(&old_commands).count();
    let removed = old_commands.difference(&new_commands).count();
    if added > 0 || removed > 0 {
        findings.push(PkgbuildFinding {
            kind: PkgbuildChangeKind::ShellCommands,
            detail: format!("{added} command line(s) added; {removed} removed."),
        });
    }
    findings
}

fn compare_assignments(
    old: &str,
    new: &str,
    fields: &[&str],
    kind: PkgbuildChangeKind,
    detail: &str,
    findings: &mut Vec<PkgbuildFinding>,
) {
    if assignments(old, fields) != assignments(new, fields) {
        findings.push(PkgbuildFinding {
            kind,
            detail: detail.into(),
        });
    }
}

fn assignments(content: &str, fields: &[&str]) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let lines = content.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let Some((key, value)) = trimmed.split_once('=') else {
            index += 1;
            continue;
        };
        let key = key.trim();
        if !fields.contains(&key) {
            index += 1;
            continue;
        }
        let mut value = value.trim().to_owned();
        let mut balance = paren_balance(&value);
        while balance > 0 && index + 1 < lines.len() {
            index += 1;
            let next = lines[index].trim();
            value.push('\n');
            value.push_str(next);
            balance += paren_balance(next);
        }
        result.insert(key.to_owned(), value);
        index += 1;
    }
    result
}

fn paren_balance(value: &str) -> i32 {
    value.chars().fold(0, |balance, character| match character {
        '(' => balance + 1,
        ')' => balance - 1,
        _ => balance,
    })
}

fn compare_function(
    old: &str,
    new: &str,
    name: &str,
    kind: PkgbuildChangeKind,
    findings: &mut Vec<PkgbuildFinding>,
) {
    if function_body(old, name) != function_body(new, name) {
        findings.push(PkgbuildFinding {
            kind,
            detail: format!("The {name}() function differs."),
        });
    }
}

fn function_body(content: &str, name: &str) -> Option<String> {
    let starts = [format!("{name}()"), format!("{name} ()")];
    let lines = content.lines().collect::<Vec<_>>();
    let start = lines.iter().position(|line| {
        let trimmed = line.trim_start();
        starts.iter().any(|prefix| trimmed.starts_with(prefix)) && trimmed.contains('{')
    })?;
    let mut body = String::new();
    let mut braces = 0_i32;
    for line in &lines[start..] {
        braces += line.matches('{').count() as i32;
        braces -= line.matches('}').count() as i32;
        body.push_str(line.trim_end());
        body.push('\n');
        if braces == 0 {
            break;
        }
    }
    Some(body)
}

fn source_domains(content: &str) -> BTreeSet<String> {
    let source = assignments(content, &["source"])
        .remove("source")
        .unwrap_or_default();
    source
        .split(|character: char| character.is_whitespace() || "'\"()".contains(character))
        .filter_map(|token| token.split_once("://").map(|(_, rest)| rest))
        .filter_map(|rest| rest.split('/').next())
        .map(|host| host.trim_end_matches(|character: char| !character.is_ascii_alphanumeric()))
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn shell_commands(content: &str) -> BTreeSet<String> {
    let mut commands = BTreeSet::new();
    for name in ["prepare", "build", "check", "package"] {
        if let Some(body) = function_body(content, name) {
            commands.extend(
                body.lines()
                    .map(str::trim)
                    .filter(|line| {
                        !line.is_empty()
                            && !line.starts_with('#')
                            && !line.ends_with("(){")
                            && !line.ends_with("() {")
                            && *line != "}"
                    })
                    .map(ToOwned::to_owned),
            );
        }
    }
    commands
}

#[derive(Clone, Copy)]
enum DiffLine<'a> {
    Same(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

pub fn unified_diff(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();
    let operations = diff_lines(&old_lines, &new_lines);
    let mut output = format!("--- {old_label}\n+++ {new_label}\n");
    output.push_str(&format!(
        "@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    ));
    for operation in operations {
        let (prefix, line) = match operation {
            DiffLine::Same(line) => (' ', line),
            DiffLine::Removed(line) => ('-', line),
            DiffLine::Added(line) => ('+', line),
        };
        output.push(prefix);
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn diff_lines<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<DiffLine<'a>> {
    if old.len().saturating_mul(new.len()) > 2_000_000 {
        return old
            .iter()
            .map(|line| DiffLine::Removed(line))
            .chain(new.iter().map(|line| DiffLine::Added(line)))
            .collect();
    }
    let mut lcs = vec![vec![0_u32; new.len() + 1]; old.len() + 1];
    for old_index in (0..old.len()).rev() {
        for new_index in (0..new.len()).rev() {
            lcs[old_index][new_index] = if old[old_index] == new[new_index] {
                lcs[old_index + 1][new_index + 1] + 1
            } else {
                lcs[old_index + 1][new_index].max(lcs[old_index][new_index + 1])
            };
        }
    }
    let (mut old_index, mut new_index) = (0, 0);
    let mut result = Vec::new();
    while old_index < old.len() && new_index < new.len() {
        if old[old_index] == new[new_index] {
            result.push(DiffLine::Same(old[old_index]));
            old_index += 1;
            new_index += 1;
        } else if lcs[old_index + 1][new_index] >= lcs[old_index][new_index + 1] {
            result.push(DiffLine::Removed(old[old_index]));
            old_index += 1;
        } else {
            result.push(DiffLine::Added(new[new_index]));
            new_index += 1;
        }
    }
    result.extend(old[old_index..].iter().map(|line| DiffLine::Removed(line)));
    result.extend(new[new_index..].iter().map(|line| DiffLine::Added(line)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_meaningful_pkgbuild_changes_and_generates_diff() {
        let old = r#"depends=('libfoo')
source=('https://old.example/foo.tar.gz')
sha256sums=('abc')
build() {
  make
}
"#;
        let new = r#"depends=('libfoo' 'cmake')
source=('https://downloads.example.org/foo.tar.gz')
sha256sums=('def')
build() {
  cmake -B build
  cmake --build build
}
"#;
        let review = review_pkgbuild("foo", Some(("helper cache", old)), new.into());
        assert!(review.unified_diff.contains("-  make"));
        assert!(review.unified_diff.contains("+  cmake -B build"));
        assert!(review.findings.iter().any(|finding| {
            finding.kind == PkgbuildChangeKind::NewSourceDomain
                && finding.detail.contains("downloads.example.org")
        }));
        assert!(
            review
                .findings
                .iter()
                .any(|finding| { finding.kind == PkgbuildChangeKind::Dependencies })
        );
        assert!(
            review
                .findings
                .iter()
                .any(|finding| { finding.kind == PkgbuildChangeKind::BuildCommands })
        );
    }

    #[test]
    fn no_baseline_does_not_claim_changes() {
        let review = review_pkgbuild("foo", None, "pkgname=foo\n".into());
        assert!(!review.has_baseline());
        assert!(review.findings.is_empty());
        assert!(review.unified_diff.is_empty());
    }

    #[test]
    fn extracts_install_script_assignment() {
        assert_eq!(
            pkgbuild_install_script("pkgname=foo\ninstall='foo.install'\n").as_deref(),
            Some("foo.install")
        );
        assert!(pkgbuild_install_script("pkgname=foo\n").is_none());
    }
}
