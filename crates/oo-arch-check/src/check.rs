//! Evaluate one `cargo metadata` document.
//!
//! A required workspace edge is present only when the manifest declares at
//! least one normal, non-optional, unconditional dependency whose path
//! normalizes to that member's manifest directory. A matching package name
//! with neither path nor source does not satisfy it. Dev, build, disabled
//! optional, `cfg(...)`, unknown kind, and every resolve edge do not satisfy
//! it either.
//!
//! The forbidden walk keeps every manifest kind, optional flag, and target,
//! including a pathless name-only member edge. Resolve traversal uses the
//! union of `node.deps` and `node.dependencies`, keyed by package id rather
//! than the dependency alias. An id with no kind fails closed. Unknown kinds,
//! unresolved path dependencies, unknown workspace members, dangling resolve
//! ids, and cycles fail closed instead of being skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use cargo_metadata::{Dependency, DependencyKind, Metadata, MetadataCommand, Node, Package};

use crate::policy::{
    R_ANALYSIS_DB_DRIVER, R_CLIENT_DB_DRIVER, R_CORE_DB, R_CORE_MODEL, R_CORE_SANDBOX,
    R_CORE_WORKSPACE_DEP, R_CYCLE, R_DEP_KIND_UNKNOWN, R_DEP_UNRESOLVED, R_EDGE_MISSING,
    R_EDGE_UNDECLARED, R_GATEWAY_ADMIN_DDL, R_GATEWAY_CONNECTOR, R_GATEWAY_PIPELINE,
    R_GATEWAY_RUNTIME, R_GATEWAY_SOURCE_CREDENTIAL, R_GATEWAY_SOURCE_DRIVER, R_MEMBER_DUPLICATE,
    R_MEMBER_MISSING, R_MEMBER_UNKNOWN, R_MEMBER_UNRESOLVED, R_RESOLVE_DANGLING, R_RESOLVE_MISSING,
    R_TOOL_FORBIDDEN, REQUIRED_EDGES, REQUIRED_MEMBERS, is_admin_ddl_crate,
    is_client_database_driver, is_gateway_forbidden_driver, is_model_sdk, is_required_edge,
    is_sandbox_orchestrator, is_source_credential_crate,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub rule: &'static str,
    pub package: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub violations: Vec<Violation>,
}

impl Report {
    pub fn is_pass(&self) -> bool {
        self.violations.is_empty()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.violations.is_empty() {
            return formatter.write_str("architecture check: pass");
        }
        writeln!(
            formatter,
            "architecture check: fail ({} violations)",
            self.violations.len()
        )?;
        for violation in &self.violations {
            writeln!(
                formatter,
                "{} {} {}",
                violation.rule, violation.package, violation.detail
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Demonstration {
    pub baseline: Report,
    pub mutated: Report,
    pub restored: Report,
    pub expected_rule: &'static str,
}

impl Demonstration {
    /// The original graph passed, the mutated copy failed for the expected rule, and the original still passes.
    pub fn succeeded(&self) -> bool {
        self.baseline.is_pass()
            && self.restored.is_pass()
            && !self.mutated.is_pass()
            && self
                .mutated
                .violations
                .iter()
                .any(|violation| violation.rule == self.expected_rule)
    }
}

impl fmt::Display for Demonstration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "baseline: {}", self.baseline)?;
        writeln!(
            formatter,
            "mutated oo-gateway -> oo-pipeline: {}",
            self.mutated
        )?;
        writeln!(formatter, "restored baseline: {}", self.restored)?;
        if self.succeeded() {
            formatter.write_str("negative demonstration: pass")
        } else {
            formatter.write_str("negative demonstration: fail")
        }
    }
}

pub fn default_workspace_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")
}

pub fn load_workspace_metadata(manifest: &Path) -> Result<Metadata, cargo_metadata::Error> {
    // A separate target dir keeps this invocation off the caller's Cargo lock.
    // Tests run inside `cargo test`, which already holds the workspace target lock.
    MetadataCommand::new()
        .manifest_path(manifest)
        .env("CARGO_TARGET_DIR", metadata_target_dir())
        .other_options(["--locked".to_string()])
        .exec()
}

fn metadata_target_dir() -> PathBuf {
    std::env::temp_dir().join("open-ontology-sp00-w01-metadata")
}

pub fn check(metadata: &Metadata) -> Report {
    let mut violations = BTreeSet::new();
    let Some(resolve) = metadata.resolve.as_ref() else {
        violations.insert(violation(
            R_RESOLVE_MISSING,
            "-",
            "cargo metadata did not include a resolve graph",
        ));
        return finish(violations);
    };

    let mut by_id: BTreeMap<&str, &Package> = BTreeMap::new();
    for package in &metadata.packages {
        if by_id.insert(package.id.repr.as_str(), package).is_some() {
            violations.insert(violation(
                R_MEMBER_DUPLICATE,
                package.name.as_ref(),
                format!("duplicate package id {}", package.id.repr),
            ));
        }
    }

    let mut members: BTreeMap<String, &Package> = BTreeMap::new();
    for member_id in &metadata.workspace_members {
        let Some(package) = by_id.get(member_id.repr.as_str()).copied() else {
            violations.insert(violation(
                R_MEMBER_UNRESOLVED,
                &member_id.repr,
                "workspace member id is not in the package list",
            ));
            continue;
        };
        let name = package.name.to_string();
        if members.insert(name.clone(), package).is_some() {
            violations.insert(violation(
                R_MEMBER_DUPLICATE,
                &name,
                "workspace member name is repeated",
            ));
        }
    }

    for required in REQUIRED_MEMBERS {
        if !members.contains_key(*required) {
            violations.insert(violation(
                R_MEMBER_MISSING,
                required,
                "required workspace member is absent",
            ));
        }
    }
    for name in members.keys() {
        if !REQUIRED_MEMBERS.contains(&name.as_str()) {
            violations.insert(violation(
                R_MEMBER_UNKNOWN,
                name,
                "workspace member is not in the architecture set",
            ));
        }
    }

    let nodes: BTreeMap<&str, &Node> = resolve
        .nodes
        .iter()
        .map(|node| (node.id.repr.as_str(), node))
        .collect();
    for package in members.values() {
        if !nodes.contains_key(package.id.repr.as_str()) {
            violations.insert(violation(
                R_MEMBER_UNRESOLVED,
                package.name.as_ref(),
                "workspace member has no resolve node",
            ));
        }
    }

    let member_ids: BTreeSet<&str> = members
        .values()
        .map(|package| package.id.repr.as_str())
        .collect();
    record_dangling_resolve(resolve, &by_id, &mut violations);

    // Production edges satisfy REQUIRED_EDGES. Observed edges are the forbidden
    // walk: every manifest kind, a pathless member name, and the resolve union.
    let mut production_edges: BTreeSet<(String, String)> = BTreeSet::new();
    let mut observed_edges: BTreeSet<(String, String)> = BTreeSet::new();
    let mut direct_external: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (name, package) in &members {
        let mut external_names = BTreeSet::new();
        for dependency in &package.dependencies {
            if dependency.kind == DependencyKind::Unknown {
                violations.insert(violation(
                    R_DEP_KIND_UNKNOWN,
                    name,
                    format!("dependency {} has an unknown kind", dependency.name),
                ));
            }
            if let Some(target) = path_workspace_target(package, dependency, &members) {
                observed_edges.insert((name.clone(), target.clone()));
                if manifest_edge_is_production(dependency) {
                    production_edges.insert((name.clone(), target));
                }
            } else if dependency.path.is_some() {
                violations.insert(violation(
                    R_DEP_UNRESOLVED,
                    name,
                    format!(
                        "path dependency {} does not match a workspace member",
                        dependency.name
                    ),
                ));
            } else if let Some(target) = name_only_workspace_member(dependency, &members) {
                // Name identity stays on the forbidden walk. It is not proof
                // that the edge targets that member's manifest directory.
                observed_edges.insert((name.clone(), target));
            } else {
                external_names.insert(dependency.name.clone());
                if let Some(renamed) = &dependency.rename {
                    external_names.insert(renamed.clone());
                }
            }
        }
        if let Some(node) = nodes.get(package.id.repr.as_str()) {
            observe_resolve_node(
                name,
                node,
                &by_id,
                &member_ids,
                &mut observed_edges,
                &mut violations,
            );
        }
        direct_external.insert(name.clone(), external_names);
    }

    for (from, to) in &observed_edges {
        if !is_required_edge(from, to) {
            violations.insert(violation(
                R_EDGE_UNDECLARED,
                from,
                format!("workspace edge {from} -> {to} is not in the allowed set"),
            ));
        }
    }
    for (from, to) in REQUIRED_EDGES {
        if !production_edges.contains(&(from.to_string(), to.to_string())) {
            violations.insert(violation(
                R_EDGE_MISSING,
                from,
                format!("required workspace edge {from} -> {to} is absent"),
            ));
        }
    }

    if let Some(cycle) = find_cycle(&observed_edges) {
        violations.insert(violation(
            R_CYCLE,
            &cycle[0],
            format!("cycle {}", cycle.join(" -> ")),
        ));
    }

    for name in members.keys() {
        let reached = reachable_others(name, &observed_edges);
        let mut externals =
            resolve_external_names(package_id(&members, name), &nodes, &by_id, &member_ids);
        for reached_name in std::iter::once(name).chain(reached.iter()) {
            if let Some(names) = direct_external.get(reached_name) {
                externals.extend(names.iter().cloned());
            }
        }
        apply_package_rules(name, &reached, &externals, &mut violations);
    }

    finish(violations)
}

pub fn demonstrate_forbidden_gateway_edge(metadata: &Metadata) -> Demonstration {
    let baseline = check(metadata);
    let mutated_metadata = inject_workspace_edge(metadata, "oo-gateway", "oo-pipeline");
    let mutated = check(&mutated_metadata);
    let restored = check(metadata);
    Demonstration {
        baseline,
        mutated,
        restored,
        expected_rule: R_GATEWAY_PIPELINE,
    }
}

fn finish(violations: BTreeSet<Violation>) -> Report {
    Report {
        violations: violations.into_iter().collect(),
    }
}

fn violation(rule: &'static str, package: &str, detail: impl Into<String>) -> Violation {
    Violation {
        rule,
        package: package.to_string(),
        detail: detail.into(),
    }
}

fn package_id<'a>(members: &'a BTreeMap<String, &Package>, name: &str) -> Option<&'a str> {
    members.get(name).map(|package| package.id.repr.as_str())
}

fn record_dangling_resolve(
    resolve: &cargo_metadata::Resolve,
    by_id: &BTreeMap<&str, &Package>,
    violations: &mut BTreeSet<Violation>,
) {
    for node in &resolve.nodes {
        if !by_id.contains_key(node.id.repr.as_str()) {
            violations.insert(violation(
                R_RESOLVE_DANGLING,
                &node.id.repr,
                "resolve node id is not in the package list",
            ));
        }
        let mut missing = BTreeSet::new();
        for dependency_id in resolve_dependency_ids(node) {
            if !by_id.contains_key(dependency_id) {
                missing.insert(dependency_id);
            }
        }
        for id in missing {
            violations.insert(violation(
                R_RESOLVE_DANGLING,
                &node.id.repr,
                format!("resolve dependency {id} is not in the package list"),
            ));
        }
    }
}

/// Kind, optional, and target gates. Callers also require a path that resolved
/// to the member directory; a name-only edge never reaches this predicate.
fn manifest_edge_is_production(dependency: &Dependency) -> bool {
    dependency.path.is_some()
        && dependency.kind == DependencyKind::Normal
        && !dependency.optional
        && dependency.target.is_none()
}

fn path_workspace_target(
    from: &Package,
    dependency: &Dependency,
    members: &BTreeMap<String, &Package>,
) -> Option<String> {
    let path = dependency.path.as_ref()?;
    let base = from.manifest_path.parent()?;
    let joined = if path.is_absolute() {
        path.clone()
    } else {
        base.join(path)
    };
    let directory = normalize(&joined);
    for (name, package) in members {
        let Some(parent) = package.manifest_path.parent() else {
            continue;
        };
        if normalize(parent) == directory {
            return Some(name.clone());
        }
    }
    None
}

fn name_only_workspace_member(
    dependency: &Dependency,
    members: &BTreeMap<String, &Package>,
) -> Option<String> {
    if dependency.path.is_none()
        && dependency.source.is_none()
        && members.contains_key(&dependency.name)
    {
        Some(dependency.name.clone())
    } else {
        None
    }
}

fn resolve_dependency_ids(node: &Node) -> BTreeSet<&str> {
    let mut ids = BTreeSet::new();
    for dependency in &node.deps {
        ids.insert(dependency.pkg.repr.as_str());
    }
    for dependency_id in &node.dependencies {
        ids.insert(dependency_id.repr.as_str());
    }
    ids
}

fn observe_resolve_node(
    package_name: &str,
    node: &Node,
    by_id: &BTreeMap<&str, &Package>,
    member_ids: &BTreeSet<&str>,
    observed_edges: &mut BTreeSet<(String, String)>,
    violations: &mut BTreeSet<Violation>,
) {
    let mut with_kind = BTreeSet::new();
    let mut unknown_labels = BTreeSet::new();
    for dependency in &node.deps {
        let id = dependency.pkg.repr.as_str();
        if dependency.dep_kinds.is_empty() {
            continue;
        }
        with_kind.insert(id);
        let Some(target) = by_id.get(id) else {
            continue;
        };
        let label = target.name.to_string();
        for kind in &dependency.dep_kinds {
            if kind.kind == DependencyKind::Unknown {
                unknown_labels.insert(label.clone());
            }
        }
    }
    for id in resolve_dependency_ids(node) {
        if !with_kind.contains(id)
            && let Some(target) = by_id.get(id)
        {
            // Empty `dep_kinds`, or an id present only on `dependencies`.
            unknown_labels.insert(target.name.to_string());
        }
        if member_ids.contains(id) {
            let Some(target) = by_id.get(id) else {
                continue;
            };
            observed_edges.insert((package_name.to_string(), target.name.to_string()));
        }
    }
    for label in unknown_labels {
        violations.insert(violation(
            R_DEP_KIND_UNKNOWN,
            package_name,
            format!("resolve dependency {label} has an unknown kind"),
        ));
    }
}

fn normalize(path: &cargo_metadata::camino::Utf8Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path.as_std_path()))
}

fn reachable_others(origin: &str, edges: &BTreeSet<(String, String)>) -> BTreeSet<String> {
    let mut adjacency: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (from, to) in edges {
        adjacency.entry(from.clone()).or_default().push(to.clone());
    }
    let mut reached = BTreeSet::new();
    let mut stack = vec![origin.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if !seen.insert(node.clone()) {
            continue;
        }
        if node != origin {
            reached.insert(node.clone());
        }
        if let Some(next) = adjacency.get(&node) {
            stack.extend(next.iter().cloned());
        }
    }
    reached
}

fn resolve_external_names(
    origin_id: Option<&str>,
    nodes: &BTreeMap<&str, &Node>,
    by_id: &BTreeMap<&str, &Package>,
    member_ids: &BTreeSet<&str>,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Some(origin_id) = origin_id else {
        return names;
    };
    let mut stack = vec![origin_id];
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(id) else {
            continue;
        };
        for dependency_id in resolve_dependency_ids(node) {
            if seen.contains(dependency_id) {
                continue;
            }
            if member_ids.contains(dependency_id) {
                stack.push(dependency_id);
                continue;
            }
            if let Some(package) = by_id.get(dependency_id) {
                names.insert(package.name.to_string());
                stack.push(dependency_id);
            }
        }
    }
    names
}

fn find_cycle(edges: &BTreeSet<(String, String)>) -> Option<Vec<String>> {
    let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut nodes = BTreeSet::new();
    for (from, to) in edges {
        adjacency
            .entry(from.as_str())
            .or_default()
            .push(to.as_str());
        nodes.insert(from.as_str());
        nodes.insert(to.as_str());
    }
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    for node in nodes {
        if color.get(node).copied().unwrap_or(0) == 0 {
            let mut stack = Vec::new();
            if let Some(cycle) = visit(node, &adjacency, &mut color, &mut stack) {
                return Some(cycle);
            }
        }
    }
    None
}

fn visit<'a>(
    node: &'a str,
    adjacency: &BTreeMap<&'a str, Vec<&'a str>>,
    color: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
) -> Option<Vec<String>> {
    color.insert(node, 1);
    stack.push(node);
    if let Some(next_nodes) = adjacency.get(node) {
        for next in next_nodes {
            let next = *next;
            match color.get(next).copied().unwrap_or(0) {
                1 => {
                    let start = stack.iter().position(|item| *item == next)?;
                    let mut cycle: Vec<String> = stack[start..]
                        .iter()
                        .map(|item| (*item).to_string())
                        .collect();
                    cycle.push(next.to_string());
                    return Some(cycle);
                }
                0 => {
                    if let Some(cycle) = visit(next, adjacency, color, stack) {
                        return Some(cycle);
                    }
                }
                _ => {}
            }
        }
    }
    stack.pop();
    color.insert(node, 2);
    None
}

fn apply_package_rules(
    package: &str,
    reached: &BTreeSet<String>,
    externals: &BTreeSet<String>,
    violations: &mut BTreeSet<Violation>,
) {
    if package == "oo-core" {
        for other in reached {
            violations.insert(violation(
                R_CORE_WORKSPACE_DEP,
                package,
                format!("oo-core reaches workspace package {other}"),
            ));
        }
        for name in externals {
            if is_client_database_driver(name) {
                violations.insert(violation(
                    R_CORE_DB,
                    package,
                    format!("oo-core reaches database crate {name}"),
                ));
            }
            if is_sandbox_orchestrator(name) {
                violations.insert(violation(
                    R_CORE_SANDBOX,
                    package,
                    format!("oo-core reaches sandbox crate {name}"),
                ));
            }
            if is_model_sdk(name) {
                violations.insert(violation(
                    R_CORE_MODEL,
                    package,
                    format!("oo-core reaches model SDK {name}"),
                ));
            }
        }
    }

    if package == "oo-gateway" {
        for other in reached {
            if other == "oo-connectors" {
                violations.insert(violation(
                    R_GATEWAY_CONNECTOR,
                    package,
                    format!("oo-gateway reaches {other}"),
                ));
            }
            if other == "oo-pipeline" {
                violations.insert(violation(
                    R_GATEWAY_PIPELINE,
                    package,
                    format!("oo-gateway reaches {other}"),
                ));
            }
            if is_admin_ddl_crate(other) {
                violations.insert(violation(
                    R_GATEWAY_ADMIN_DDL,
                    package,
                    format!("oo-gateway reaches administrative DDL crate {other}"),
                ));
            }
            if is_source_credential_crate(other) {
                violations.insert(violation(
                    R_GATEWAY_SOURCE_CREDENTIAL,
                    package,
                    format!("oo-gateway reaches source-credential crate {other}"),
                ));
            }
        }
        for name in externals {
            if is_gateway_forbidden_driver(name) {
                violations.insert(violation(
                    R_GATEWAY_SOURCE_DRIVER,
                    package,
                    format!("oo-gateway reaches source or control driver {name}"),
                ));
            }
            if is_sandbox_orchestrator(name) || is_model_sdk(name) {
                violations.insert(violation(
                    R_GATEWAY_RUNTIME,
                    package,
                    format!("oo-gateway reaches sandbox or model crate {name}"),
                ));
            }
        }
    }

    if matches!(package, "oo-sdk" | "oo-cli" | "oo-mcp") {
        for name in externals {
            if is_client_database_driver(name) {
                violations.insert(violation(
                    R_CLIENT_DB_DRIVER,
                    package,
                    format!("{package} reaches database crate {name}"),
                ));
            }
        }
    }

    if package == "oo-analysis" {
        for name in externals {
            if is_client_database_driver(name) {
                violations.insert(violation(
                    R_ANALYSIS_DB_DRIVER,
                    package,
                    format!("oo-analysis reaches database crate {name}"),
                ));
            }
        }
    }

    if package == "oo-arch-check" {
        for other in reached {
            violations.insert(violation(
                R_TOOL_FORBIDDEN,
                package,
                format!("architecture gate reaches workspace package {other}"),
            ));
        }
        for name in externals {
            if is_client_database_driver(name)
                || is_sandbox_orchestrator(name)
                || is_model_sdk(name)
            {
                violations.insert(violation(
                    R_TOOL_FORBIDDEN,
                    package,
                    format!("architecture gate reaches forbidden crate {name}"),
                ));
            }
        }
    }
}

fn inject_workspace_edge(metadata: &Metadata, from: &str, to: &str) -> Metadata {
    let mut metadata = metadata.clone();
    let Some(to_id) = metadata
        .packages
        .iter()
        .find(|package| package.name == to)
        .map(|package| package.id.clone())
    else {
        return metadata;
    };
    let manifest_template = metadata
        .packages
        .iter()
        .find_map(|package| package.dependencies.first().cloned());
    let resolve_template = metadata.resolve.as_ref().and_then(|resolve| {
        resolve
            .nodes
            .iter()
            .find_map(|node| node.deps.first().cloned())
    });
    let from_id = metadata
        .packages
        .iter()
        .find(|package| package.name == from)
        .map(|package| package.id.clone());
    if let Some(package) = metadata
        .packages
        .iter_mut()
        .find(|package| package.name == from)
        && let Some(template) = manifest_template
    {
        let mut dependency = template;
        dependency.name = to.to_string();
        dependency.rename = None;
        dependency.optional = false;
        dependency.kind = DependencyKind::Normal;
        dependency.path = Some(cargo_metadata::camino::Utf8PathBuf::from(format!(
            "../{to}"
        )));
        dependency.source = None;
        dependency.features.clear();
        package.dependencies.push(dependency);
    }
    if let (Some(from_id), Some(resolve)) = (from_id, metadata.resolve.as_mut())
        && let Some(node) = resolve.nodes.iter_mut().find(|node| node.id == from_id)
    {
        if let Some(template) = resolve_template {
            let mut dependency = template;
            dependency.pkg = to_id.clone();
            dependency.name = to.replace('-', "_");
            node.deps.push(dependency);
        }
        node.dependencies.push(to_id);
    }
    metadata
}

#[cfg(test)]
fn inject_external_crate(metadata: &Metadata, package_name: &str, crate_name: &str) -> Metadata {
    let mut metadata = metadata.clone();
    let Some(template) = metadata.packages.iter().find_map(|package| {
        package
            .dependencies
            .iter()
            .find(|dependency| dependency.path.is_none() && dependency.source.is_some())
            .cloned()
    }) else {
        return metadata;
    };
    if let Some(package) = metadata
        .packages
        .iter_mut()
        .find(|package| package.name == package_name)
    {
        let mut dependency = template;
        dependency.name = crate_name.to_string();
        dependency.rename = None;
        dependency.optional = false;
        dependency.kind = DependencyKind::Normal;
        dependency.path = None;
        dependency.features.clear();
        package.dependencies.push(dependency);
    }
    metadata
}

/// `cargo metadata` must be the workspace's cargo, not a nested resolver that ignores `--locked`.
#[cfg(test)]
fn ensure_locked_cargo() -> bool {
    std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--locked")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(default_workspace_manifest())
        .env("CARGO_TARGET_DIR", metadata_target_dir())
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::OnceLock;

    use super::*;
    use crate::policy::{
        R_CLIENT_DB_DRIVER, R_CORE_DB, R_CORE_MODEL, R_CORE_SANDBOX, R_CORE_WORKSPACE_DEP, R_CYCLE,
        R_DEP_KIND_UNKNOWN, R_EDGE_MISSING, R_EDGE_UNDECLARED, R_GATEWAY_ADMIN_DDL,
        R_GATEWAY_CONNECTOR, R_GATEWAY_PIPELINE, R_GATEWAY_RUNTIME, R_GATEWAY_SOURCE_CREDENTIAL,
        R_GATEWAY_SOURCE_DRIVER, R_MEMBER_MISSING, R_MEMBER_UNKNOWN, R_RESOLVE_DANGLING,
        REQUIRED_MEMBERS,
    };

    fn metadata() -> &'static Metadata {
        static CELL: OnceLock<Metadata> = OnceLock::new();
        CELL.get_or_init(|| {
            assert!(ensure_locked_cargo(), "locked cargo metadata failed");
            load_workspace_metadata(&default_workspace_manifest()).expect("load metadata")
        })
    }

    fn rules(report: &Report) -> BTreeSet<&'static str> {
        report
            .violations
            .iter()
            .map(|violation| violation.rule)
            .collect()
    }

    fn package_id_of(metadata: &Metadata, name: &str) -> cargo_metadata::PackageId {
        metadata
            .packages
            .iter()
            .find(|package| package.name == name)
            .unwrap_or_else(|| panic!("package {name}"))
            .id
            .clone()
    }

    fn manifest_dependency_mut<'a>(
        metadata: &'a mut Metadata,
        from: &str,
        to: &str,
    ) -> &'a mut cargo_metadata::Dependency {
        metadata
            .packages
            .iter_mut()
            .find(|package| package.name == from)
            .unwrap_or_else(|| panic!("package {from}"))
            .dependencies
            .iter_mut()
            .find(|dependency| dependency.name == to)
            .unwrap_or_else(|| panic!("manifest dependency {from} -> {to}"))
    }

    fn set_manifest_edge(
        metadata: &mut Metadata,
        from: &str,
        to: &str,
        kind: DependencyKind,
        optional: bool,
        target: Option<cargo_metadata::cargo_platform::Platform>,
    ) {
        let dependency = manifest_dependency_mut(metadata, from, to);
        dependency.kind = kind;
        dependency.optional = optional;
        dependency.target = target;
    }

    fn cfg_windows() -> cargo_metadata::cargo_platform::Platform {
        cargo_metadata::cargo_platform::Platform::from_str("cfg(windows)").expect("cfg(windows)")
    }

    fn assert_normal_unconditional_resolve(metadata: &Metadata, from: &str, to: &str) {
        let from_id = package_id_of(metadata, from);
        let to_id = package_id_of(metadata, to);
        let node = metadata
            .resolve
            .as_ref()
            .expect("resolve")
            .nodes
            .iter()
            .find(|node| node.id == from_id)
            .unwrap_or_else(|| panic!("resolve node for {from}"));
        let dependency = node
            .deps
            .iter()
            .find(|dependency| dependency.pkg == to_id)
            .unwrap_or_else(|| panic!("resolve edge {from} -> {to}"));
        assert!(
            dependency
                .dep_kinds
                .iter()
                .any(|kind| kind.kind == DependencyKind::Normal && kind.target.is_none()),
            "expected a normal unconditional resolve edge {from} -> {to}, got {dependency:?}"
        );
    }

    fn set_resolve_edge_kind(metadata: &mut Metadata, from: &str, to: &str, kind: DependencyKind) {
        let from_id = package_id_of(metadata, from);
        let to_id = package_id_of(metadata, to);
        let node = metadata
            .resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == from_id)
            .unwrap_or_else(|| panic!("resolve node for {from}"));
        let dependency = node
            .deps
            .iter_mut()
            .find(|dependency| dependency.pkg == to_id)
            .unwrap_or_else(|| panic!("resolve edge {from} -> {to}"));
        assert!(
            !dependency.dep_kinds.is_empty(),
            "resolve edge {from} -> {to} has no dep_kinds"
        );
        for info in &mut dependency.dep_kinds {
            info.kind = kind;
            info.target = None;
        }
    }

    fn detach_resolve_edge(metadata: &mut Metadata, from: &str, to: &str) {
        let from_id = package_id_of(metadata, from);
        let to_id = package_id_of(metadata, to);
        let node = metadata
            .resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == from_id)
            .unwrap_or_else(|| panic!("resolve node for {from}"));
        let before = node.deps.len();
        node.deps.retain(|dependency| dependency.pkg != to_id);
        node.dependencies.retain(|id| id != &to_id);
        assert_ne!(
            node.deps.len(),
            before,
            "resolve edge {from} -> {to} was not present"
        );
    }

    fn assert_edge_missing(report: &Report, from: &str, to: &str) {
        let expected = format!("{from} -> {to}");
        assert!(
            report.violations.iter().any(|violation| {
                violation.rule == R_EDGE_MISSING
                    && violation.package == from
                    && violation.detail.contains(&expected)
            }),
            "{report}"
        );
    }

    fn assert_package_rule(report: &Report, rule: &str, package: &str, name: &str) {
        assert!(
            report.violations.iter().any(|violation| {
                violation.rule == rule
                    && violation.package == package
                    && violation.detail.contains(name)
            }),
            "{name} on {package}:\n{report}"
        );
    }

    fn violation_count(report: &Report, rule: &str, package: &str, detail: &str) -> usize {
        report
            .violations
            .iter()
            .filter(|violation| {
                violation.rule == rule
                    && violation.package == package
                    && violation.detail.contains(detail)
            })
            .count()
    }

    fn member_dir(metadata: &Metadata, name: &str) -> String {
        metadata
            .packages
            .iter()
            .find(|package| package.name == name)
            .unwrap_or_else(|| panic!("package {name}"))
            .manifest_path
            .parent()
            .unwrap_or_else(|| panic!("manifest dir {name}"))
            .to_string()
    }

    fn resolve_node<'a>(metadata: &'a Metadata, name: &str) -> &'a cargo_metadata::Node {
        let id = package_id_of(metadata, name);
        metadata
            .resolve
            .as_ref()
            .expect("resolve")
            .nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap_or_else(|| panic!("resolve node {name}"))
    }

    fn resolve_node_mut<'a>(
        metadata: &'a mut Metadata,
        name: &str,
    ) -> &'a mut cargo_metadata::Node {
        let id = package_id_of(metadata, name);
        metadata
            .resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == id)
            .unwrap_or_else(|| panic!("resolve node {name}"))
    }

    /// Registry package plus an empty resolve node. The id is not linked from anywhere yet.
    fn add_external_package(
        metadata: &mut Metadata,
        crate_name: &str,
    ) -> cargo_metadata::PackageId {
        let template = metadata
            .packages
            .iter()
            .find(|package| package.source.is_some())
            .cloned()
            .expect("registry package");
        let node_template = metadata
            .resolve
            .as_ref()
            .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == template.id))
            .cloned()
            .expect("registry resolve node");
        let id = cargo_metadata::PackageId {
            repr: format!("arch-check-repro#{crate_name}@0.0.0"),
        };
        let mut package = template;
        package.name = cargo_metadata::PackageName::new(crate_name.to_string());
        package.id = id.clone();
        package.dependencies.clear();
        metadata.packages.push(package);
        let mut node = node_template;
        node.id = id.clone();
        node.dependencies.clear();
        node.deps.clear();
        node.features.clear();
        metadata.resolve.as_mut().expect("resolve").nodes.push(node);
        id
    }

    fn push_dependency_id(metadata: &mut Metadata, from: &str, to: &cargo_metadata::PackageId) {
        resolve_node_mut(metadata, from)
            .dependencies
            .push(to.clone());
    }

    fn push_resolve_dep(
        metadata: &mut Metadata,
        from: &str,
        to: &cargo_metadata::PackageId,
        alias: &str,
        empty_kinds: bool,
    ) {
        let template = metadata
            .resolve
            .as_ref()
            .and_then(|resolve| {
                resolve
                    .nodes
                    .iter()
                    .find_map(|node| node.deps.first().cloned())
            })
            .expect("resolve dependency template");
        let mut edge = template;
        edge.pkg = to.clone();
        edge.name = alias.to_string();
        if empty_kinds {
            edge.dep_kinds.clear();
        } else {
            for kind in &mut edge.dep_kinds {
                kind.kind = DependencyKind::Normal;
                kind.target = None;
            }
        }
        resolve_node_mut(metadata, from).deps.push(edge);
    }

    fn push_name_only_member(metadata: &mut Metadata, from: &str, to: &str) {
        let template = metadata
            .packages
            .iter()
            .find(|package| package.name == from)
            .unwrap_or_else(|| panic!("package {from}"))
            .dependencies
            .first()
            .cloned()
            .unwrap_or_else(|| panic!("manifest dependency on {from}"));
        let mut dependency = template;
        dependency.name = to.to_string();
        dependency.rename = None;
        dependency.optional = false;
        dependency.kind = DependencyKind::Normal;
        dependency.target = None;
        dependency.path = None;
        dependency.source = None;
        dependency.features.clear();
        metadata
            .packages
            .iter_mut()
            .find(|package| package.name == from)
            .unwrap_or_else(|| panic!("package {from}"))
            .dependencies
            .push(dependency);
    }

    /// A resolve node two hops out, with no manifest dependency on either hop.
    fn inject_resolve_only_transitive(
        metadata: &Metadata,
        origin: &str,
        via_name: &str,
        leaf_name: &str,
    ) -> Metadata {
        let mut metadata = metadata.clone();
        let template = metadata
            .packages
            .iter()
            .find(|package| package.source.is_some())
            .cloned()
            .expect("registry package");
        let dep_template = metadata
            .resolve
            .as_ref()
            .and_then(|resolve| {
                resolve
                    .nodes
                    .iter()
                    .find_map(|node| node.deps.first().cloned())
            })
            .expect("resolve dependency");
        let node_template = metadata
            .resolve
            .as_ref()
            .and_then(|resolve| resolve.nodes.first().cloned())
            .expect("resolve node");
        let origin_id = package_id_of(&metadata, origin);
        let via_id = cargo_metadata::PackageId {
            repr: format!("arch-check-via#{via_name}@0.0.0"),
        };
        let leaf_id = cargo_metadata::PackageId {
            repr: format!("arch-check-leaf#{leaf_name}@0.0.0"),
        };

        let mut via_package = template.clone();
        via_package.name = cargo_metadata::PackageName::new(via_name.to_string());
        via_package.id = via_id.clone();
        via_package.dependencies.clear();
        let mut leaf_package = template;
        leaf_package.name = cargo_metadata::PackageName::new(leaf_name.to_string());
        leaf_package.id = leaf_id.clone();
        leaf_package.dependencies.clear();
        metadata.packages.push(via_package);
        metadata.packages.push(leaf_package);

        let mut leaf_edge = dep_template.clone();
        leaf_edge.pkg = leaf_id.clone();
        leaf_edge.name = leaf_name.replace('-', "_");
        for kind in &mut leaf_edge.dep_kinds {
            kind.kind = DependencyKind::Normal;
            kind.target = None;
        }
        let mut via_node = node_template.clone();
        via_node.id = via_id.clone();
        via_node.dependencies = vec![leaf_id.clone()];
        via_node.deps = vec![leaf_edge];
        via_node.features.clear();

        let mut origin_edge = dep_template;
        origin_edge.pkg = via_id.clone();
        origin_edge.name = via_name.replace('-', "_");
        for kind in &mut origin_edge.dep_kinds {
            kind.kind = DependencyKind::Normal;
            kind.target = None;
        }
        let mut leaf_node = node_template;
        leaf_node.id = leaf_id;
        leaf_node.dependencies.clear();
        leaf_node.deps.clear();
        leaf_node.features.clear();

        let resolve = metadata.resolve.as_mut().expect("resolve");
        let origin_node = resolve
            .nodes
            .iter_mut()
            .find(|node| node.id == origin_id)
            .expect("origin node");
        origin_node.dependencies.push(via_id.clone());
        origin_node.deps.push(origin_edge);
        resolve.nodes.push(via_node);
        resolve.nodes.push(leaf_node);
        metadata
    }

    #[test]
    fn real_workspace_passes() {
        let report = check(metadata());
        assert!(report.is_pass(), "baseline violations:\n{report}");
    }

    #[test]
    fn forbidden_pipeline_edge_fails_then_baseline_passes() {
        let demonstration = demonstrate_forbidden_gateway_edge(metadata());
        assert!(
            demonstration.succeeded(),
            "baseline pass={} mutated={} restored pass={}\n{demonstration}",
            demonstration.baseline.is_pass(),
            rules(&demonstration.mutated)
                .into_iter()
                .collect::<Vec<_>>()
                .join(","),
            demonstration.restored.is_pass()
        );
        assert!(rules(&demonstration.mutated).contains(R_GATEWAY_PIPELINE));
        assert!(demonstration.baseline == demonstration.restored);
    }

    #[test]
    fn gateway_connector_edge_reports_connector_and_source_credentials() {
        let mutated = inject_workspace_edge(metadata(), "oo-gateway", "oo-connectors");
        let report = check(&mutated);
        let found = rules(&report);
        assert!(found.contains(R_GATEWAY_CONNECTOR), "{report}");
        assert!(found.contains(R_GATEWAY_SOURCE_CREDENTIAL), "{report}");
        assert!(check(metadata()).is_pass());
    }

    #[test]
    fn gateway_control_edge_reports_admin_ddl() {
        let mutated = inject_workspace_edge(metadata(), "oo-gateway", "oo-control");
        let report = check(&mutated);
        assert!(rules(&report).contains(R_GATEWAY_ADMIN_DDL), "{report}");
    }

    #[test]
    fn transitive_connector_through_core_fails() {
        let mutated = inject_workspace_edge(metadata(), "oo-core", "oo-connectors");
        let report = check(&mutated);
        let found = rules(&report);
        assert!(found.contains(R_CORE_WORKSPACE_DEP), "{report}");
        assert!(found.contains(R_GATEWAY_CONNECTOR), "{report}");
        assert!(found.contains(R_GATEWAY_SOURCE_CREDENTIAL), "{report}");
    }

    #[test]
    fn core_database_and_sandbox_crates_fail() {
        let with_sqlx = inject_external_crate(metadata(), "oo-core", "sqlx");
        assert!(
            rules(&check(&with_sqlx)).contains(R_CORE_DB),
            "{}",
            check(&with_sqlx)
        );
        let with_bollard = inject_external_crate(metadata(), "oo-core", "bollard");
        assert!(
            rules(&check(&with_bollard)).contains(R_CORE_SANDBOX),
            "{}",
            check(&with_bollard)
        );
    }

    #[test]
    fn sdk_driver_reaches_cli() {
        let mutated = inject_external_crate(metadata(), "oo-sdk", "tokio-postgres");
        let report = check(&mutated);
        let found = rules(&report);
        assert!(found.contains(R_CLIENT_DB_DRIVER), "{report}");
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.rule == R_CLIENT_DB_DRIVER
                    && violation.package == "oo-cli"),
            "{report}"
        );
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.rule == R_CLIENT_DB_DRIVER
                    && violation.package == "oo-sdk"),
            "{report}"
        );
    }

    #[test]
    fn cycle_is_reported() {
        let mutated = inject_workspace_edge(metadata(), "oo-core", "oo-gateway");
        let report = check(&mutated);
        assert!(rules(&report).contains(R_CYCLE), "{report}");
        assert!(rules(&report).contains(R_CORE_WORKSPACE_DEP), "{report}");
    }

    #[test]
    fn unknown_member_and_missing_member_fail() {
        let mut added = metadata().clone();
        let mut package = added
            .packages
            .iter()
            .find(|package| package.name == "oo-core")
            .cloned()
            .expect("oo-core");
        package.name = cargo_metadata::PackageName::new("oo-rogue".to_string());
        package.id.repr = format!("{}-rogue", package.id.repr);
        added.workspace_members.push(package.id.clone());
        added.packages.push(package);
        let added_report = check(&added);
        assert!(
            rules(&added_report).contains(R_MEMBER_UNKNOWN),
            "{added_report}"
        );

        let mut removed = metadata().clone();
        let jobs_id = removed
            .packages
            .iter()
            .find(|package| package.name == "oo-jobs")
            .map(|package| package.id.clone())
            .expect("oo-jobs");
        removed.workspace_members.retain(|id| id != &jobs_id);
        let removed_report = check(&removed);
        assert!(
            rules(&removed_report).contains(R_MEMBER_MISSING),
            "{removed_report}"
        );
        assert!(check(metadata()).is_pass());
    }

    #[test]
    fn unknown_manifest_kind_is_detected_without_resolve_edge() {
        let mut metadata = metadata().clone();
        assert_normal_unconditional_resolve(&metadata, "oo-gateway", "oo-core");
        set_manifest_edge(
            &mut metadata,
            "oo-gateway",
            "oo-core",
            DependencyKind::Unknown,
            false,
            None,
        );
        detach_resolve_edge(&mut metadata, "oo-gateway", "oo-core");
        let report = check(&metadata);
        assert!(
            report.violations.iter().any(|violation| {
                violation.rule == R_DEP_KIND_UNKNOWN
                    && violation.package == "oo-gateway"
                    && violation.detail == "dependency oo-core has an unknown kind"
            }),
            "{report}"
        );
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.detail
                    != "resolve dependency oo-core has an unknown kind"),
            "{report}"
        );
        assert_edge_missing(&report, "oo-gateway", "oo-core");
    }

    #[test]
    fn unknown_kind_forbidden_edge_stays_in_the_forbidden_walk() {
        let mut metadata = inject_workspace_edge(metadata(), "oo-gateway", "oo-pipeline");
        set_manifest_edge(
            &mut metadata,
            "oo-gateway",
            "oo-pipeline",
            DependencyKind::Unknown,
            false,
            None,
        );
        detach_resolve_edge(&mut metadata, "oo-gateway", "oo-pipeline");
        let report = check(&metadata);
        assert!(
            report.violations.iter().any(|violation| {
                violation.rule == R_DEP_KIND_UNKNOWN
                    && violation.package == "oo-gateway"
                    && violation.detail == "dependency oo-pipeline has an unknown kind"
            }),
            "{report}"
        );
        assert!(rules(&report).contains(R_GATEWAY_PIPELINE), "{report}");
    }

    #[test]
    fn dangling_resolve_id_fails() {
        let mut metadata = metadata().clone();
        let resolve = metadata.resolve.as_mut().expect("resolve");
        let node = resolve
            .nodes
            .iter_mut()
            .find(|node| !node.deps.is_empty())
            .expect("resolve node with a dependency");
        let dependency = node.deps.first_mut().expect("dep");
        dependency.pkg.repr = "missing-package-id".to_string();
        let report = check(&metadata);
        assert!(rules(&report).contains(R_RESOLVE_DANGLING), "{report}");
    }

    #[test]
    fn dev_only_required_edge_is_missing_despite_stale_normal_resolve() {
        let mut metadata = metadata().clone();
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Development,
            false,
            None,
        );
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
    }

    #[test]
    fn build_only_required_edge_is_missing_despite_stale_normal_resolve() {
        let mut metadata = metadata().clone();
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Build,
            false,
            None,
        );
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
    }

    #[test]
    fn disabled_optional_required_edge_is_missing_despite_stale_normal_resolve() {
        let mut metadata = metadata().clone();
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Normal,
            true,
            None,
        );
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
    }

    #[test]
    fn cfg_only_required_edge_is_missing_despite_stale_normal_resolve() {
        let mut metadata = metadata().clone();
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Normal,
            false,
            Some(cfg_windows()),
        );
        assert_normal_unconditional_resolve(&metadata, "oo-pipeline", "oo-connectors");
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
    }

    #[test]
    fn normal_unconditional_required_edge_restored_passes() {
        let mut metadata = metadata().clone();
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Development,
            false,
            None,
        );
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Normal,
            false,
            None,
        );
        let report = check(&metadata);
        assert!(report.is_pass(), "{report}");
    }

    #[test]
    fn dev_only_gateway_pipeline_is_forbidden() {
        let mut metadata = inject_workspace_edge(metadata(), "oo-gateway", "oo-pipeline");
        set_manifest_edge(
            &mut metadata,
            "oo-gateway",
            "oo-pipeline",
            DependencyKind::Development,
            false,
            None,
        );
        set_resolve_edge_kind(
            &mut metadata,
            "oo-gateway",
            "oo-pipeline",
            DependencyKind::Development,
        );
        let report = check(&metadata);
        let found = rules(&report);
        assert!(found.contains(R_GATEWAY_PIPELINE), "{report}");
        assert!(found.contains(R_GATEWAY_CONNECTOR), "{report}");
        assert!(found.contains(R_GATEWAY_SOURCE_CREDENTIAL), "{report}");
    }

    #[test]
    fn dev_only_cycle_is_reported() {
        let mut metadata = inject_workspace_edge(metadata(), "oo-core", "oo-gateway");
        set_manifest_edge(
            &mut metadata,
            "oo-core",
            "oo-gateway",
            DependencyKind::Development,
            false,
            None,
        );
        set_resolve_edge_kind(
            &mut metadata,
            "oo-core",
            "oo-gateway",
            DependencyKind::Development,
        );
        let report = check(&metadata);
        assert!(rules(&report).contains(R_CYCLE), "{report}");
    }

    #[test]
    fn resolve_only_transitive_driver_is_forbidden() {
        let metadata =
            inject_resolve_only_transitive(metadata(), "oo-gateway", "schema-stub", "sqlx");
        let gateway = metadata
            .packages
            .iter()
            .find(|package| package.name == "oo-gateway")
            .expect("gateway");
        assert!(
            gateway
                .dependencies
                .iter()
                .all(|dependency| dependency.name != "sqlx" && dependency.name != "schema-stub")
        );
        let report = check(&metadata);
        assert_package_rule(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", "sqlx");
    }

    #[test]
    fn renamed_sqlx_package_is_forbidden() {
        let mut metadata = inject_external_crate(metadata(), "oo-core", "sqlx");
        let dependency = manifest_dependency_mut(&mut metadata, "oo-core", "sqlx");
        dependency.rename = Some("friendly".to_string());
        assert_eq!(dependency.name, "sqlx");
        let report = check(&metadata);
        assert_package_rule(&report, R_CORE_DB, "oo-core", "sqlx");
        assert!(
            report
                .violations
                .iter()
                .all(|violation| !violation.detail.contains("friendly")),
            "{report}"
        );
    }

    #[test]
    fn gateway_postgres_stack_is_allowed() {
        let mut metadata = metadata().clone();
        for name in [
            "tokio-postgres",
            "postgres-types",
            "postgres-protocol",
            "deadpool-postgres",
        ] {
            metadata = inject_external_crate(&metadata, "oo-gateway", name);
        }
        let report = check(&metadata);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != R_GATEWAY_SOURCE_DRIVER),
            "{report}"
        );
        assert!(report.is_pass(), "{report}");
    }

    #[test]
    fn gateway_sync_postgres_and_sqlx_are_forbidden() {
        for name in ["postgres", "sqlx"] {
            let metadata = inject_external_crate(metadata(), "oo-gateway", name);
            let report = check(&metadata);
            assert_package_rule(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", name);
        }
    }

    #[test]
    fn named_database_family_bypasses_are_forbidden() {
        for name in [
            "diesel",
            "diesel-async",
            "diesel_migrations",
            "sea-orm",
            "sea-orm-migration",
            "sea-orm-arrow",
            "libsql",
            "libsql-sys",
            "mysqlclient-sys",
            "mysqlclient-src",
        ] {
            let on_core = inject_external_crate(metadata(), "oo-core", name);
            assert_package_rule(&check(&on_core), R_CORE_DB, "oo-core", name);
            let on_gateway = inject_external_crate(metadata(), "oo-gateway", name);
            assert_package_rule(
                &check(&on_gateway),
                R_GATEWAY_SOURCE_DRIVER,
                "oo-gateway",
                name,
            );
        }
    }

    #[test]
    fn schema_serde_and_arrow_are_not_database_credentials() {
        let mut metadata = metadata().clone();
        for name in [
            "serde",
            "serde_json",
            "schemars",
            "arrow",
            "arrow-array",
            "arrow-schema",
            "parquet",
        ] {
            metadata = inject_external_crate(&metadata, "oo-core", name);
        }
        let report = check(&metadata);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != R_CORE_DB),
            "{report}"
        );
        assert!(report.is_pass(), "{report}");
    }

    #[test]
    fn openai_package_is_a_model_sdk() {
        for name in ["openai", "openai-api-rs", "async-openai"] {
            let on_core = inject_external_crate(metadata(), "oo-core", name);
            assert_package_rule(&check(&on_core), R_CORE_MODEL, "oo-core", name);
            let on_gateway = inject_external_crate(metadata(), "oo-gateway", name);
            assert_package_rule(&check(&on_gateway), R_GATEWAY_RUNTIME, "oo-gateway", name);
        }
    }

    #[test]
    fn required_membership_is_seventeen_including_xtask() {
        assert_eq!(REQUIRED_MEMBERS.len(), 17);
        assert!(REQUIRED_MEMBERS.contains(&"xtask"));
        assert!(is_required_edge("xtask", "oo-core"));
        assert!(is_required_edge("xtask", "oo-arch-check"));
        assert!(!is_required_edge("oo-arch-check", "xtask"));
    }

    #[test]
    fn name_only_normal_edge_does_not_satisfy_required_edge() {
        let mut metadata = metadata().clone();
        let dependency = manifest_dependency_mut(&mut metadata, "oo-pipeline", "oo-connectors");
        assert!(dependency.path.is_some());
        dependency.path = None;
        dependency.source = None;
        dependency.kind = DependencyKind::Normal;
        dependency.optional = false;
        dependency.target = None;
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
    }

    #[test]
    fn name_only_normal_edge_cannot_hide_dev_only_path_edge() {
        let mut metadata = metadata().clone();
        set_manifest_edge(
            &mut metadata,
            "oo-pipeline",
            "oo-connectors",
            DependencyKind::Development,
            false,
            None,
        );
        push_name_only_member(&mut metadata, "oo-pipeline", "oo-connectors");
        let report = check(&metadata);
        assert_edge_missing(&report, "oo-pipeline", "oo-connectors");
        assert_eq!(
            violation_count(
                &report,
                R_EDGE_MISSING,
                "oo-pipeline",
                "oo-pipeline -> oo-connectors"
            ),
            1,
            "{report}"
        );
    }

    #[test]
    fn id_only_sqlx_on_gateway_is_forbidden_once() {
        let mut metadata = metadata().clone();
        let sqlx = add_external_package(&mut metadata, "sqlx");
        push_dependency_id(&mut metadata, "oo-gateway", &sqlx);
        push_dependency_id(&mut metadata, "oo-gateway", &sqlx);
        let node = resolve_node(&metadata, "oo-gateway");
        assert!(node.deps.iter().all(|dependency| dependency.pkg != sqlx));
        assert_eq!(
            node.dependencies.iter().filter(|id| *id == &sqlx).count(),
            2
        );
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", "sqlx"),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_DEP_KIND_UNKNOWN,
                "oo-gateway",
                "resolve dependency sqlx has an unknown kind"
            ),
            1,
            "{report}"
        );
    }

    #[test]
    fn id_only_sqlx_through_benign_external_is_forbidden_once() {
        let mut metadata = metadata().clone();
        let benign = add_external_package(&mut metadata, "schema-stub");
        let sqlx = add_external_package(&mut metadata, "sqlx");
        push_dependency_id(&mut metadata, "oo-gateway", &benign);
        push_dependency_id(&mut metadata, "schema-stub", &sqlx);
        assert!(
            resolve_node(&metadata, "oo-gateway")
                .deps
                .iter()
                .all(|dependency| dependency.pkg != benign)
        );
        assert!(
            resolve_node(&metadata, "schema-stub")
                .deps
                .iter()
                .all(|dependency| dependency.pkg != sqlx)
        );
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", "sqlx"),
            1,
            "{report}"
        );
        assert!(
            report.violations.iter().all(|violation| {
                violation.rule != R_GATEWAY_SOURCE_DRIVER
                    || !violation.detail.contains("schema-stub")
            }),
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_DEP_KIND_UNKNOWN,
                "oo-gateway",
                "resolve dependency schema-stub has an unknown kind"
            ),
            1,
            "{report}"
        );
    }

    #[test]
    fn id_only_gateway_pipeline_is_forbidden_once() {
        let mut metadata = metadata().clone();
        let pipeline = package_id_of(&metadata, "oo-pipeline");
        push_dependency_id(&mut metadata, "oo-gateway", &pipeline);
        push_dependency_id(&mut metadata, "oo-gateway", &pipeline);
        assert!(
            resolve_node(&metadata, "oo-gateway")
                .deps
                .iter()
                .all(|dependency| dependency.pkg != pipeline)
        );
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_PIPELINE, "oo-gateway", "oo-pipeline"),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(&report, R_GATEWAY_CONNECTOR, "oo-gateway", "oo-connectors"),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_EDGE_UNDECLARED,
                "oo-gateway",
                "oo-gateway -> oo-pipeline"
            ),
            1,
            "{report}"
        );
    }

    #[test]
    fn id_only_transitive_workspace_path_reaches_pipeline() {
        let mut metadata = metadata().clone();
        let sdk = package_id_of(&metadata, "oo-sdk");
        let pipeline = package_id_of(&metadata, "oo-pipeline");
        push_dependency_id(&mut metadata, "oo-gateway", &sdk);
        push_dependency_id(&mut metadata, "oo-sdk", &pipeline);
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_PIPELINE, "oo-gateway", "oo-pipeline"),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_EDGE_UNDECLARED,
                "oo-gateway",
                "oo-gateway -> oo-sdk"
            ),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_EDGE_UNDECLARED,
                "oo-sdk",
                "oo-sdk -> oo-pipeline"
            ),
            1,
            "{report}"
        );
    }

    #[test]
    fn id_only_cycle_is_reported_once() {
        let mut metadata = metadata().clone();
        let gateway = package_id_of(&metadata, "oo-gateway");
        push_dependency_id(&mut metadata, "oo-core", &gateway);
        push_dependency_id(&mut metadata, "oo-core", &gateway);
        let report = check(&metadata);
        let cycles = report
            .violations
            .iter()
            .filter(|violation| {
                violation.rule == R_CYCLE
                    && violation.detail.contains("oo-core")
                    && violation.detail.contains("oo-gateway")
            })
            .count();
        assert_eq!(cycles, 1, "{report}");
        assert_eq!(
            violation_count(&report, R_CORE_WORKSPACE_DEP, "oo-core", "oo-gateway"),
            1,
            "{report}"
        );
    }

    #[test]
    fn id_only_benign_edge_fails_closed_on_unknown_kind() {
        let mut metadata = metadata().clone();
        let benign = add_external_package(&mut metadata, "schema-stub");
        push_dependency_id(&mut metadata, "oo-gateway", &benign);
        let report = check(&metadata);
        assert_eq!(
            violation_count(
                &report,
                R_DEP_KIND_UNKNOWN,
                "oo-gateway",
                "resolve dependency schema-stub has an unknown kind"
            ),
            1,
            "{report}"
        );
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != R_GATEWAY_SOURCE_DRIVER),
            "{report}"
        );
    }

    #[test]
    fn duplicate_dangling_resolve_id_is_reported_once() {
        let mut metadata = metadata().clone();
        let missing = cargo_metadata::PackageId {
            repr: "missing-package-id".to_string(),
        };
        let node = resolve_node_mut(&mut metadata, "oo-gateway");
        node.deps
            .first_mut()
            .expect("gateway resolve dependency")
            .pkg = missing.clone();
        node.dependencies.push(missing);
        let report = check(&metadata);
        let count = report
            .violations
            .iter()
            .filter(|violation| {
                violation.rule == R_RESOLVE_DANGLING
                    && violation.detail.contains("missing-package-id")
            })
            .count();
        assert_eq!(count, 1, "{report}");
    }

    #[test]
    fn empty_dep_kinds_sqlx_stays_visible_without_a_duplicate() {
        let mut metadata = metadata().clone();
        let sqlx = add_external_package(&mut metadata, "sqlx");
        push_resolve_dep(&mut metadata, "oo-gateway", &sqlx, "friendly", true);
        push_dependency_id(&mut metadata, "oo-gateway", &sqlx);
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", "sqlx"),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_DEP_KIND_UNKNOWN,
                "oo-gateway",
                "resolve dependency sqlx has an unknown kind"
            ),
            1,
            "{report}"
        );
        assert!(
            report
                .violations
                .iter()
                .all(|violation| !violation.detail.contains("friendly")),
            "{report}"
        );
    }

    #[test]
    fn resolved_paths_and_rename_still_satisfy_required_edge() {
        let connectors = member_dir(metadata(), "oo-connectors");
        let pipeline = member_dir(metadata(), "oo-pipeline");
        for path in [
            connectors.clone(),
            format!("{connectors}/."),
            format!("{pipeline}/../oo-connectors"),
            "../oo-connectors".to_string(),
        ] {
            let mut metadata = metadata().clone();
            let dependency = manifest_dependency_mut(&mut metadata, "oo-pipeline", "oo-connectors");
            dependency.path = Some(cargo_metadata::camino::Utf8PathBuf::from(&path));
            dependency.source = None;
            dependency.kind = DependencyKind::Normal;
            dependency.optional = false;
            dependency.target = None;
            dependency.rename = Some("connectors_alias".to_string());
            let report = check(&metadata);
            assert!(report.is_pass(), "{path}\n{report}");
        }

        let mut renamed_name = metadata().clone();
        let dependency = manifest_dependency_mut(&mut renamed_name, "oo-pipeline", "oo-connectors");
        dependency.name = "connectors_alias".to_string();
        dependency.rename = Some("connectors_alias".to_string());
        assert!(dependency.path.is_some());
        let report = check(&renamed_name);
        assert!(report.is_pass(), "{report}");
    }

    #[test]
    fn name_only_forbidden_workspace_edge_stays_visible() {
        let mut metadata = metadata().clone();
        push_name_only_member(&mut metadata, "oo-gateway", "oo-pipeline");
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_PIPELINE, "oo-gateway", "oo-pipeline"),
            1,
            "{report}"
        );
        assert_eq!(
            violation_count(
                &report,
                R_EDGE_UNDECLARED,
                "oo-gateway",
                "oo-gateway -> oo-pipeline"
            ),
            1,
            "{report}"
        );
        assert!(
            !report.violations.iter().any(|violation| {
                violation.rule == R_EDGE_MISSING && violation.detail.contains("oo-pipeline")
            }),
            "{report}"
        );
    }

    #[test]
    fn resolve_alias_is_not_package_identity() {
        let mut metadata = metadata().clone();
        let benign = add_external_package(&mut metadata, "schema-stub");
        push_resolve_dep(&mut metadata, "oo-gateway", &benign, "sqlx", false);
        push_dependency_id(&mut metadata, "oo-gateway", &benign);
        let report = check(&metadata);
        assert!(
            report.violations.iter().all(|violation| {
                violation.rule != R_GATEWAY_SOURCE_DRIVER && !violation.detail.contains("sqlx")
            }),
            "{report}"
        );
        assert!(report.is_pass(), "{report}");
    }

    #[test]
    fn deps_only_renamed_sqlx_is_forbidden_once() {
        let mut metadata = metadata().clone();
        let sqlx = add_external_package(&mut metadata, "sqlx");
        push_resolve_dep(&mut metadata, "oo-gateway", &sqlx, "friendly", false);
        assert!(
            resolve_node(&metadata, "oo-gateway")
                .dependencies
                .iter()
                .all(|id| id != &sqlx)
        );
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", "sqlx"),
            1,
            "{report}"
        );
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != R_DEP_KIND_UNKNOWN
                    && !violation.detail.contains("friendly")),
            "{report}"
        );
    }

    #[test]
    fn deps_and_dependencies_duplicate_sqlx_is_one_violation() {
        let mut metadata = metadata().clone();
        let sqlx = add_external_package(&mut metadata, "sqlx");
        push_resolve_dep(&mut metadata, "oo-gateway", &sqlx, "friendly", false);
        push_dependency_id(&mut metadata, "oo-gateway", &sqlx);
        push_dependency_id(&mut metadata, "oo-gateway", &sqlx);
        let report = check(&metadata);
        assert_eq!(
            violation_count(&report, R_GATEWAY_SOURCE_DRIVER, "oo-gateway", "sqlx"),
            1,
            "{report}"
        );
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != R_DEP_KIND_UNKNOWN
                    && !violation.detail.contains("friendly")),
            "{report}"
        );
    }

    #[test]
    fn resolve_id_does_not_satisfy_required_edge() {
        let mut metadata = metadata().clone();
        let connectors = package_id_of(&metadata, "oo-connectors");
        metadata
            .packages
            .iter_mut()
            .find(|package| package.name == "oo-pipeline")
            .expect("oo-pipeline")
            .dependencies
            .retain(|dependency| dependency.name != "oo-connectors");
        let node = resolve_node_mut(&mut metadata, "oo-pipeline");
        node.deps.retain(|dependency| dependency.pkg != connectors);
        assert!(node.dependencies.iter().any(|id| id == &connectors));
        assert_edge_missing(&check(&metadata), "oo-pipeline", "oo-connectors");
    }
}
