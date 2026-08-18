use crate::domain::{DependencyNode, DependencyReport, InstallReason, Package};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub fn build_dependency_report(
    packages: &[Package],
    package_name: &str,
    max_depth: usize,
) -> Option<DependencyReport> {
    let max_depth = max_depth.clamp(1, 20);
    let package_map = packages
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<HashMap<_, _>>();
    let target = package_map.get(package_name)?;
    let dependencies = dependency_edges(packages, &package_map);
    let reverse = reverse_edges(&dependencies);
    let mut path = HashSet::new();
    let dependency_tree = build_tree(package_name, &dependencies, 0, max_depth, &mut path);
    let mut path = HashSet::new();
    let reverse_tree = build_tree(package_name, &reverse, 0, max_depth, &mut path);
    let mut why_paths = Vec::new();
    let mut current = vec![package_name.to_owned()];
    collect_why_paths(
        package_name,
        &reverse,
        &package_map,
        max_depth,
        &mut current,
        &mut HashSet::new(),
        &mut why_paths,
    );
    for path in &mut why_paths {
        path.reverse();
    }
    why_paths.sort();
    why_paths.dedup();

    Some(DependencyReport {
        package: package_name.to_owned(),
        install_reason: target.install_reason,
        why_paths,
        dependencies: dependency_tree,
        reverse_dependencies: reverse_tree,
        orphan_candidates_after_removal: orphan_candidates(package_name, packages, &dependencies),
        max_depth,
    })
}

fn dependency_edges<'a>(
    packages: &'a [Package],
    package_map: &HashMap<&'a str, &'a Package>,
) -> BTreeMap<String, Vec<String>> {
    packages
        .iter()
        .map(|package| {
            let mut dependencies = package
                .dependencies
                .iter()
                .map(|dependency| dependency_name(dependency))
                .filter(|dependency| package_map.contains_key(dependency.as_str()))
                .collect::<Vec<_>>();
            dependencies.sort();
            dependencies.dedup();
            (package.name.clone(), dependencies)
        })
        .collect()
}

fn reverse_edges(edges: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    let mut reverse = edges
        .keys()
        .map(|name| (name.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for (package, dependencies) in edges {
        for dependency in dependencies {
            reverse
                .entry(dependency.clone())
                .or_default()
                .push(package.clone());
        }
    }
    for packages in reverse.values_mut() {
        packages.sort();
        packages.dedup();
    }
    reverse
}

fn dependency_name(value: &str) -> String {
    value
        .split(|character| ['<', '>', '=', ':'].contains(&character))
        .next()
        .unwrap_or(value)
        .to_owned()
}

fn build_tree(
    name: &str,
    edges: &BTreeMap<String, Vec<String>>,
    depth: usize,
    max_depth: usize,
    path: &mut HashSet<String>,
) -> DependencyNode {
    if path.contains(name) {
        return DependencyNode {
            name: name.to_owned(),
            cycle: true,
            depth_limited: false,
            children: Vec::new(),
        };
    }
    let children = edges.get(name).cloned().unwrap_or_default();
    if depth >= max_depth {
        return DependencyNode {
            name: name.to_owned(),
            cycle: false,
            depth_limited: !children.is_empty(),
            children: Vec::new(),
        };
    }
    path.insert(name.to_owned());
    let children = children
        .iter()
        .map(|child| build_tree(child, edges, depth + 1, max_depth, path))
        .collect();
    path.remove(name);
    DependencyNode {
        name: name.to_owned(),
        cycle: false,
        depth_limited: false,
        children,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_why_paths(
    name: &str,
    reverse: &BTreeMap<String, Vec<String>>,
    packages: &HashMap<&str, &Package>,
    max_depth: usize,
    current: &mut Vec<String>,
    path: &mut HashSet<String>,
    results: &mut Vec<Vec<String>>,
) {
    if current.len() > max_depth + 1 || !path.insert(name.to_owned()) {
        return;
    }
    let explicit = packages
        .get(name)
        .is_some_and(|package| package.install_reason == InstallReason::Explicit);
    if explicit && current.len() > 1 {
        results.push(current.clone());
    } else {
        for parent in reverse.get(name).into_iter().flatten() {
            current.push(parent.clone());
            collect_why_paths(parent, reverse, packages, max_depth, current, path, results);
            current.pop();
        }
    }
    path.remove(name);
}

fn orphan_candidates(
    removed: &str,
    packages: &[Package],
    dependencies: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut reachable = HashSet::new();
    let mut pending = packages
        .iter()
        .filter(|package| {
            package.name != removed && package.install_reason == InstallReason::Explicit
        })
        .map(|package| package.name.clone())
        .collect::<Vec<_>>();
    while let Some(name) = pending.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        pending.extend(
            dependencies
                .get(&name)
                .into_iter()
                .flatten()
                .filter(|dependency| dependency.as_str() != removed)
                .cloned(),
        );
    }
    packages
        .iter()
        .filter(|package| {
            package.name != removed
                && package.install_reason == InstallReason::Dependency
                && !reachable.contains(&package.name)
        })
        .map(|package| package.name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PackageSource;

    fn package(name: &str, reason: InstallReason, dependencies: &[&str]) -> Package {
        let mut package = Package::summary(name, "1", PackageSource::Local);
        package.installed = true;
        package.install_reason = reason;
        package.dependencies = dependencies.iter().map(|value| (*value).into()).collect();
        package
    }

    #[test]
    fn explains_dependency_paths_and_detects_cycles() {
        let packages = vec![
            package("app", InstallReason::Explicit, &["plugin"]),
            package("plugin", InstallReason::Dependency, &["libfoo"]),
            package("libfoo", InstallReason::Dependency, &["plugin"]),
        ];
        let report = build_dependency_report(&packages, "libfoo", 5).expect("report");
        assert_eq!(report.why_paths, [vec!["app", "plugin", "libfoo"]]);
        assert!(report.dependencies.children[0].children[0].cycle);
    }

    #[test]
    fn reports_dependency_orphan_candidates_after_removal() {
        let packages = vec![
            package("app", InstallReason::Explicit, &["libfoo"]),
            package("libfoo", InstallReason::Dependency, &["leaf"]),
            package("leaf", InstallReason::Dependency, &[]),
        ];
        let report = build_dependency_report(&packages, "app", 5).expect("report");
        assert_eq!(report.orphan_candidates_after_removal, ["leaf", "libfoo"]);
    }
}
