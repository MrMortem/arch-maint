use super::{AurBackend, AurHelperBackend};
use crate::{
    config::xdg_cache_home,
    domain::{
        AurMetadata, Package, PackageSource, PackageUpdate, PkgbuildChangeKind, PkgbuildFinding,
        PkgbuildReview, ReviewedAurFile,
    },
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AurRpcBackend {
    client: Client,
    rpc_url: String,
    helper: Option<AurHelperBackend>,
}

impl AurRpcBackend {
    pub fn new(rpc_url: String, helper: Option<AurHelperBackend>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .context("failed to configure AUR HTTP client")?;
        Ok(Self {
            client,
            rpc_url,
            helper,
        })
    }

    async fn request(&self, query: &[(&str, &str)]) -> Result<AurResponse> {
        let response = self
            .client
            .get(&self.rpc_url)
            .query(query)
            .send()
            .await
            .context("AUR request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("AUR returned HTTP {status}");
        }
        let payload: AurResponse = response.json().await.context("invalid AUR response")?;
        if let Some(error) = payload.error.as_deref() {
            bail!("AUR API error: {error}");
        }
        Ok(payload)
    }

    async fn helper_baseline(&self, package: &Package) -> Option<(PathBuf, String)> {
        let helper = self.helper.as_ref()?;
        let package_base = package
            .aur
            .as_ref()
            .and_then(|metadata| metadata.package_base.as_deref())
            .unwrap_or(&package.name);
        if validate_aur_name(package_base).is_err() {
            return None;
        }
        let path = match helper.kind() {
            super::HelperKind::Paru => xdg_cache_home()
                .join("paru/clone")
                .join(package_base)
                .join("PKGBUILD"),
            super::HelperKind::Yay => xdg_cache_home()
                .join("yay")
                .join(package_base)
                .join("PKGBUILD"),
        };
        tokio::fs::read_to_string(&path)
            .await
            .ok()
            .map(|content| (path, content))
    }

    async fn fetch_plain_file(&self, package_base: &str, file: &str) -> Result<String> {
        let package_base = validate_aur_name(package_base)?;
        let file = validate_aur_file(file)?;
        let url = format!("https://aur.archlinux.org/cgit/aur.git/plain/{file}");
        let response = self
            .client
            .get(url)
            .query(&[("h", package_base)])
            .send()
            .await
            .with_context(|| format!("failed to fetch AUR file {file}"))?;
        if !response.status().is_success() {
            bail!("AUR returned HTTP {} for {file}", response.status());
        }
        response
            .text()
            .await
            .with_context(|| format!("invalid AUR file response for {file}"))
    }
}

#[async_trait]
impl AurBackend for AurRpcBackend {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = crate::parser::validate_search_query(query)?;
        let response = self
            .request(&[
                ("v", "5"),
                ("type", "search"),
                ("by", "name-desc"),
                ("arg", query),
            ])
            .await?;
        Ok(response
            .results
            .into_iter()
            .map(AurPackage::into_domain)
            .collect())
    }

    async fn info(&self, package: &str) -> Result<Option<Package>> {
        let response = self
            .request(&[("v", "5"), ("type", "info"), ("arg[]", package)])
            .await?;
        Ok(response
            .results
            .into_iter()
            .next()
            .map(AurPackage::into_domain))
    }

    async fn check_updates(&self) -> Result<Vec<PackageUpdate>> {
        match &self.helper {
            Some(helper) => helper.check_updates().await,
            None => Ok(Vec::new()),
        }
    }

    async fn fetch_pkgbuild(&self, package: &str) -> Result<String> {
        self.fetch_plain_file(package, "PKGBUILD").await
    }

    async fn review_pkgbuild(&self, package: &Package) -> Result<PkgbuildReview> {
        let package_base = package
            .aur
            .as_ref()
            .and_then(|metadata| metadata.package_base.as_deref())
            .unwrap_or(&package.name);
        let (current, baseline) = tokio::join!(
            self.fetch_plain_file(package_base, "PKGBUILD"),
            self.helper_baseline(package)
        );
        let current = current?;
        let mut review = match &baseline {
            Some((path, previous)) => crate::parser::review_pkgbuild(
                package.name.clone(),
                Some((&path.display().to_string(), previous)),
                current,
            ),
            None => crate::parser::review_pkgbuild(package.name.clone(), None, current),
        };
        let current_script = crate::parser::pkgbuild_install_script(&review.current_pkgbuild);
        let previous_script = baseline
            .as_ref()
            .and_then(|(_, previous)| crate::parser::pkgbuild_install_script(previous));
        if current_script.is_some() || previous_script.is_some() {
            let current_content = match current_script.as_deref() {
                Some(file) => self.fetch_plain_file(package_base, file).await?,
                None => String::new(),
            };
            let previous_content = match (&baseline, previous_script.as_deref()) {
                (Some((pkgbuild_path, _)), Some(file)) => {
                    tokio::fs::read_to_string(pkgbuild_path.with_file_name(file))
                        .await
                        .ok()
                }
                _ => None,
            };
            let path = current_script
                .as_deref()
                .or(previous_script.as_deref())
                .unwrap_or("unknown.install")
                .to_owned();
            let assignment_changed = current_script != previous_script;
            let content_changed = previous_content
                .as_ref()
                .is_some_and(|previous| previous != &current_content);
            let diff = (baseline.is_some()
                && (previous_script.is_none() || previous_content.is_some()))
            .then(|| {
                crate::parser::unified_diff(
                    previous_content.as_deref().unwrap_or(""),
                    &current_content,
                    &format!("previous/{path}"),
                    &format!("current/{path}"),
                )
            });
            if baseline.is_some() && (assignment_changed || content_changed) {
                review
                    .findings
                    .retain(|finding| finding.kind != PkgbuildChangeKind::InstallScript);
                review.findings.push(PkgbuildFinding {
                    kind: PkgbuildChangeKind::InstallScript,
                    detail: format!(
                        "Related AUR file {path} differs from the helper-cache baseline."
                    ),
                });
            }
            if baseline.is_none() {
                review.evidence_notes.push(format!(
                    "No baseline was available for related install script {path}; its current content is shown without change claims."
                ));
            } else if previous_script.is_some() && previous_content.is_none() {
                review.evidence_notes.push(format!(
                    "The helper-cache baseline referenced {path}, but that file could not be read; content changes are not claimed."
                ));
            }
            review.related_files.push(ReviewedAurFile {
                path,
                current_content,
                unified_diff: diff,
            });
        }
        Ok(review)
    }
}

fn validate_aur_name(name: &str) -> Result<&str> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "@._+-".contains(character))
    {
        bail!("invalid AUR package name `{name}`");
    }
    Ok(name)
}

fn validate_aur_file(file: &str) -> Result<&str> {
    if file.is_empty()
        || file.len() > 255
        || file.starts_with('-')
        || !file
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "@._+-".contains(character))
    {
        bail!("invalid AUR related filename `{file}`");
    }
    Ok(file)
}

#[derive(Debug, Deserialize)]
struct AurResponse {
    #[serde(default)]
    results: Vec<AurPackage>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AurPackage {
    name: String,
    version: String,
    description: Option<String>,
    package_base: Option<String>,
    url: Option<String>,
    maintainer: Option<String>,
    #[serde(default)]
    num_votes: u64,
    #[serde(default)]
    popularity: f64,
    first_submitted: Option<i64>,
    last_modified: Option<i64>,
    out_of_date: Option<i64>,
    #[serde(default)]
    depends: Vec<String>,
    #[serde(default)]
    opt_depends: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    replaces: Vec<String>,
    #[serde(default)]
    license: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
}

impl AurPackage {
    fn into_domain(self) -> Package {
        let mut package = Package::summary(self.name, self.version, PackageSource::Aur);
        package.description = self.description;
        package.url = self.url;
        package.dependencies = self.depends;
        package.optional_dependencies = self.opt_depends;
        package.conflicts = self.conflicts;
        package.provides = self.provides;
        package.replaces = self.replaces;
        package.licenses = self.license;
        package.groups = self.groups;
        package.aur = Some(AurMetadata {
            package_base: self.package_base,
            maintainer: self.maintainer,
            votes: self.num_votes,
            popularity: self.popularity,
            first_submitted: self
                .first_submitted
                .and_then(|value| Utc.timestamp_opt(value, 0).single()),
            last_modified: self
                .last_modified
                .and_then(|value| Utc.timestamp_opt(value, 0).single()),
            out_of_date: self
                .out_of_date
                .and_then(|value| Utc.timestamp_opt(value, 0).single()),
        });
        package
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aur_rpc_metadata() {
        let json = r#"{"version":5,"type":"search","resultcount":1,"results":[{"Name":"paru","PackageBase":"paru","Version":"2.0.4-1","Description":"Feature packed AUR helper","URL":"https://github.com/Morganamilo/paru","NumVotes":1234,"Popularity":9.5,"Maintainer":"example","FirstSubmitted":1600000000,"LastModified":1700000000,"Depends":["git","pacman"],"OptDepends":[],"Conflicts":[],"Provides":["paru"],"Replaces":[],"License":["GPL-3.0-or-later"],"Groups":[]}]}"#;
        let response: AurResponse = serde_json::from_str(json).expect("RPC fixture should parse");
        let package = response
            .results
            .into_iter()
            .next()
            .expect("one package")
            .into_domain();
        assert_eq!(package.name, "paru");
        assert_eq!(package.dependencies, ["git", "pacman"]);
        assert_eq!(package.aur.expect("AUR metadata").votes, 1234);
    }

    #[test]
    fn aur_names_cannot_escape_helper_cache_directories() {
        assert!(validate_aur_name("visual-studio-code-bin").is_ok());
        assert!(validate_aur_name("../escape").is_err());
        assert!(validate_aur_name("--config").is_err());
        assert!(validate_aur_file("foo.install").is_ok());
        assert!(validate_aur_file("../foo.install").is_err());
    }
}
