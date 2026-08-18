use super::{InstallReason, Package, PackageSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackageManifest {
    pub official: ManifestPackages,
    pub aur: ManifestPackages,
    /// Foreign packages that have not been verified against the AUR RPC.
    pub foreign: ManifestPackages,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestPackages {
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDrift {
    pub missing_official: Vec<String>,
    pub missing_aur: Vec<String>,
    pub missing_foreign: Vec<String>,
    pub extra_official: Vec<String>,
    pub extra_foreign: Vec<String>,
}

impl PackageManifest {
    pub fn from_installed(packages: &[Package]) -> Self {
        let mut manifest = Self::default();
        for package in packages {
            if package.install_reason != InstallReason::Explicit {
                continue;
            }
            match package.source {
                PackageSource::Official(_) => manifest.official.packages.push(package.name.clone()),
                PackageSource::Aur => manifest.aur.packages.push(package.name.clone()),
                PackageSource::Foreign | PackageSource::Local => {
                    manifest.foreign.packages.push(package.name.clone())
                }
            }
        }
        normalize(&mut manifest.official.packages);
        normalize(&mut manifest.aur.packages);
        normalize(&mut manifest.foreign.packages);
        manifest
    }

    pub fn drift(&self, packages: &[Package]) -> ManifestDrift {
        let installed_official = packages
            .iter()
            .filter(|package| matches!(package.source, PackageSource::Official(_)))
            .map(|package| package.name.as_str())
            .collect::<BTreeSet<_>>();
        let installed_foreign = packages
            .iter()
            .filter(|package| {
                matches!(
                    package.source,
                    PackageSource::Aur | PackageSource::Foreign | PackageSource::Local
                )
            })
            .map(|package| package.name.as_str())
            .collect::<BTreeSet<_>>();
        let explicit_official = packages
            .iter()
            .filter(|package| {
                package.install_reason == InstallReason::Explicit
                    && matches!(package.source, PackageSource::Official(_))
            })
            .map(|package| package.name.clone())
            .collect::<BTreeSet<_>>();
        let explicit_foreign = packages
            .iter()
            .filter(|package| {
                package.install_reason == InstallReason::Explicit
                    && matches!(
                        package.source,
                        PackageSource::Aur | PackageSource::Foreign | PackageSource::Local
                    )
            })
            .map(|package| package.name.clone())
            .collect::<BTreeSet<_>>();
        let desired_official = self
            .official
            .packages
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let desired_foreign = self
            .aur
            .packages
            .iter()
            .chain(&self.foreign.packages)
            .cloned()
            .collect::<BTreeSet<_>>();
        ManifestDrift {
            missing_official: missing(&self.official.packages, &installed_official),
            missing_aur: missing(&self.aur.packages, &installed_foreign),
            missing_foreign: missing(&self.foreign.packages, &installed_foreign),
            extra_official: explicit_official
                .difference(&desired_official)
                .cloned()
                .collect(),
            extra_foreign: explicit_foreign
                .difference(&desired_foreign)
                .cloned()
                .collect(),
        }
    }
}

fn normalize(packages: &mut Vec<String>) {
    packages.sort();
    packages.dedup();
}

fn missing(desired: &[String], installed: &BTreeSet<&str>) -> Vec<String> {
    desired
        .iter()
        .filter(|package| !installed.contains(package.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, source: PackageSource) -> Package {
        let mut package = Package::summary(name, "1", source);
        package.installed = true;
        package.install_reason = InstallReason::Explicit;
        package
    }

    #[test]
    fn manifest_round_trip_preserves_honest_foreign_classification() {
        let manifest = PackageManifest::from_installed(&[
            package("git", PackageSource::Official("extra".into())),
            package("unknown-bin", PackageSource::Foreign),
        ]);
        let encoded = toml::to_string_pretty(&manifest).expect("encode");
        let decoded: PackageManifest = toml::from_str(&encoded).expect("decode");
        assert_eq!(decoded.official.packages, ["git"]);
        assert!(decoded.aur.packages.is_empty());
        assert_eq!(decoded.foreign.packages, ["unknown-bin"]);
    }

    #[test]
    fn drift_reports_missing_and_extra_without_reconciling() {
        let manifest: PackageManifest =
            toml::from_str("[official]\npackages=['git', 'firefox']\n[aur]\npackages=['paru']\n")
                .expect("manifest");
        let drift = manifest.drift(&[
            package("git", PackageSource::Official("extra".into())),
            package("ripgrep", PackageSource::Official("extra".into())),
        ]);
        assert_eq!(drift.missing_official, ["firefox"]);
        assert_eq!(drift.missing_aur, ["paru"]);
        assert_eq!(drift.extra_official, ["ripgrep"]);
    }
}
