use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::fail::{self, Fail, BOUND, CHECKSUM_MISMATCH, DUPLICATE, FAKE_CLAIM, GRAPH_MISMATCH, HASH_MISMATCH, HELPER_LOCK_CHANGED, INVALID_TIME, MALFORMED, MISSING_LICENSE_BODY, MISSING_THIRD_PARTY_NOTICE, OMITTED_APACHE_BODY, PATH_TRAVERSAL, PLACEHOLDER, SCANNER_OUTPUT_MISSING, UNKNOWN_PACKAGE, WRONG_DUAL_CHOICE, WRONG_EXPRESSION, WRONG_SOURCE};
use crate::json::{self, Val};
use crate::sha;
use crate::timeutil::{self, Stamp};
use crate::Paths;

const MAX_BYTES: u64 = 1_048_576;
const AUDIT_REL: &str = "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out";
const DENY_REL: &str = "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-deny-advisories.err";
const MANIFESTS: [&str; 2] = [
    "testing/harness/containment-supervisor/Cargo.toml",
    "testing/harness/containment-supervisor/Cargo.lock",
];
const GATES: [&str; 10] = [
    "named_owner",
    "security_exception",
    "protected_ci",
    "runsc",
    "image",
    "whole_graph",
    "secret_rotation",
    "release",
    "full_at_sec_05",
    "platform_c_library",
];

struct PinFile {
    basename: String,
    sha256: String,
    spdx: String,
    third_party: bool,
    in_record: bool,
}

struct PinPkg {
    name: String,
    version: String,
    checksum: String,
    repository: String,
    expression: String,
    choice: Option<String>,
    msrv: String,
    publication: String,
    age: i64,
    default_off: bool,
    selecting: Vec<String>,
    features: Vec<String>,
    graph_role: String,
    dependency_kind: String,
    selected_by: String,
    copyright: String,
    native_link: String,
    files: Vec<PinFile>,
}

struct Inventory {
    adoption: String,
    helper_lock_sha256: String,
    deny_toml_sha256: String,
    audit_json_sha256: String,
    deny_advisories_sha256: String,
    age_sha256: String,
    metadata_sha256: String,
    advisory_rev_sha256: String,
    rustsec_commit: String,
    rustsec_commit_time: String,
    recorded_at: String,
    undeclared_msrv: String,
    registry_source: String,
    packages: Vec<PinPkg>,
}

struct Resolved {
    kind: String,
    uses_default: bool,
    features: Vec<String>,
    selected_by: String,
}

pub fn run(paths: &Paths) -> Result<(), Fail> {
    reject_cli_traversal(paths)?;
    let as_of = timeutil::parse_rfc3339(&paths.as_of)?;
    let inventory = load_inventory(&paths.inventory)?;
    expect_hash(
        &paths.helper_lock,
        &inventory.helper_lock_sha256,
        "helper-lock-changed",
        HELPER_LOCK_CHANGED,
    )?;
    expect_hash(&paths.deny_config, &inventory.deny_toml_sha256, "hash-mismatch", HASH_MISMATCH)?;
    require_present(&paths.audit_json)?;
    require_present(&paths.deny_advisories)?;
    require_present(&paths.age)?;
    require_present(&paths.metadata)?;
    require_present(&paths.advisory_rev)?;
    expect_hash(&paths.audit_json, &inventory.audit_json_sha256, "hash-mismatch", HASH_MISMATCH)?;
    expect_hash(
        &paths.deny_advisories,
        &inventory.deny_advisories_sha256,
        "hash-mismatch",
        HASH_MISMATCH,
    )?;
    expect_hash(&paths.age, &inventory.age_sha256, "hash-mismatch", HASH_MISMATCH)?;
    expect_hash(&paths.metadata, &inventory.metadata_sha256, "hash-mismatch", HASH_MISMATCH)?;
    expect_hash(
        &paths.advisory_rev,
        &inventory.advisory_rev_sha256,
        "hash-mismatch",
        HASH_MISMATCH,
    )?;

    let lock = parse_lock(&paths.helper_lock)?;
    let ages = parse_age(&paths.age)?;
    let versions = parse_versions(&paths.version_meta, &inventory)?;
    let resolved = parse_metadata(&paths.metadata, &inventory)?;
    check_audit_file(&paths.audit_json, &inventory)?;
    check_deny_file(&paths.deny_advisories)?;
    check_advisory_rev(&paths.advisory_rev, &inventory)?;
    check_helper_manifest(&paths.helper_manifest, &inventory)?;
    check_inputs_agree(&inventory, &lock, &ages, &versions, &resolved)?;

    let records = load_records(&paths.records, &inventory)?;
    for package in &inventory.packages {
        let doc = records.get(&package.name).ok_or_else(|| {
            fail::fail("unknown-package", UNKNOWN_PACKAGE, format!("missing record {}", package.name))
        })?;
        check_record(doc, package, &inventory, &records, paths, as_of)?;
    }
    Ok(())
}

fn reject_cli_traversal(paths: &Paths) -> Result<(), Fail> {
    let list = [
        &paths.inventory,
        &paths.records,
        &paths.licenses,
        &paths.retained_licenses,
        &paths.helper_lock,
        &paths.helper_manifest,
        &paths.deny_config,
        &paths.metadata,
        &paths.age,
        &paths.audit_json,
        &paths.deny_advisories,
        &paths.advisory_rev,
        &paths.version_meta,
        &paths.repo_root,
    ];
    for path in list {
        if path.components().any(|component| component.as_os_str() == "..") {
            return Err(fail::fail("path-traversal", PATH_TRAVERSAL, "cli path contains .."));
        }
    }
    Ok(())
}

fn load_inventory(path: &Path) -> Result<Inventory, Fail> {
    let root = read_json(path, "malformed-record")?;
    let packages_val = need(&root, "packages")?;
    let mut packages = Vec::new();
    for item in packages_val.as_array()? {
        let mut files = Vec::new();
        for file in need(item, "files")?.as_array()? {
            files.push(PinFile {
                basename: need_string(file, "basename")?,
                sha256: need_string(file, "sha256")?,
                spdx: need_string(file, "spdx")?,
                third_party: need_bool(file, "third_party_notice")?,
                in_record: need_bool(file, "record_evidence")?,
            });
        }
        packages.push(PinPkg {
            name: need_string(item, "name")?,
            version: need_string(item, "version")?,
            checksum: need_string(item, "checksum")?,
            repository: need_string(item, "repository")?,
            expression: need_string(item, "expression")?,
            choice: optional_string(item, "choice")?,
            msrv: need_string(item, "msrv")?,
            publication: need_string(item, "publication_date")?,
            age: need_int(item, "age_days_at_adoption")?,
            default_off: need_bool(item, "default_features_disabled")?,
            selecting: string_list(need(item, "selecting_features")?)?,
            features: string_list(need(item, "features_enabled")?)?,
            graph_role: need_string(item, "graph_role")?,
            dependency_kind: need_string(item, "dependency_kind")?,
            selected_by: need_string(item, "selected_by")?,
            copyright: need_string(item, "copyright_notice")?,
            native_link: need_string(item, "native_link")?,
            files,
        });
    }
    if packages.len() != 5 {
        return Err(fail::fail("unknown-package", UNKNOWN_PACKAGE, "inventory is not the five-crate graph"));
    }
    Ok(Inventory {
        adoption: need_string(&root, "adoption_date")?,
        helper_lock_sha256: need_string(&root, "helper_lock_sha256")?,
        deny_toml_sha256: need_string(&root, "deny_toml_sha256")?,
        audit_json_sha256: need_string(&root, "audit_json_sha256")?,
        deny_advisories_sha256: need_string(&root, "deny_advisories_sha256")?,
        age_sha256: need_string(&root, "age_sha256")?,
        metadata_sha256: need_string(&root, "metadata_sha256")?,
        advisory_rev_sha256: need_string(&root, "advisory_rev_sha256")?,
        rustsec_commit: need_string(&root, "rustsec_commit")?,
        rustsec_commit_time: need_string(&root, "rustsec_commit_time")?,
        recorded_at: need_string(&root, "recorded_at")?,
        undeclared_msrv: need_string(&root, "undeclared_msrv")?,
        registry_source: need_string(&root, "registry_source")?,
        packages,
    })
}

fn check_inputs_agree(
    inventory: &Inventory,
    lock: &BTreeMap<String, (String, String)>,
    ages: &BTreeMap<String, (String, String)>,
    versions: &BTreeMap<String, VersionFact>,
    resolved: &BTreeMap<String, Resolved>,
) -> Result<(), Fail> {
    if lock.len() != 5 {
        return Err(fail::fail("checksum-mismatch", CHECKSUM_MISMATCH, "lock registry package count"));
    }
    for package in &inventory.packages {
        let (version, checksum) = lock.get(&package.name).ok_or_else(|| {
            fail::fail("checksum-mismatch", CHECKSUM_MISMATCH, format!("lock missing {}", package.name))
        })?;
        if version != &package.version || checksum != &package.checksum {
            return Err(fail::fail(
                "checksum-mismatch",
                CHECKSUM_MISMATCH,
                format!("lock disagrees with inventory for {}", package.name),
            ));
        }
        let (created, second) = ages.get(&package.name).ok_or_else(|| {
            fail::fail("invalid-time", INVALID_TIME, format!("age file missing {}", package.name))
        })?;
        if created != &package.publication || created == second {
            return Err(fail::fail(
                "invalid-time",
                INVALID_TIME,
                format!("age file publication for {}", package.name),
            ));
        }
        let fact = versions.get(&package.name).ok_or_else(|| {
            fail::fail("wrong-source", WRONG_SOURCE, format!("version json missing {}", package.name))
        })?;
        if fact.created_at != package.publication || fact.checksum != package.checksum || fact.repository != package.repository
            || fact.license != package.expression || fact.yanked
        {
            return Err(fail::fail(
                "wrong-source",
                WRONG_SOURCE,
                format!("version json disagrees for {}", package.name),
            ));
        }
        if fact.rust_version.as_deref() != Some(package.msrv.as_str()) && !(fact.rust_version.is_none() && package.msrv == inventory.undeclared_msrv)
        {
            return Err(fail::fail("fake-claim", FAKE_CLAIM, format!("msrv pin for {}", package.name)));
        }
        let edge = resolved.get(&package.name).ok_or_else(|| {
            fail::fail("graph-mismatch", GRAPH_MISMATCH, format!("metadata missing {}", package.name))
        })?;
        if edge.kind != package.dependency_kind
            || edge.selected_by != package.selected_by
            || edge.uses_default == package.default_off
            || edge.features != package.selecting
        {
            return Err(fail::fail(
                "graph-mismatch",
                GRAPH_MISMATCH,
                format!("metadata edge for {}", package.name),
            ));
        }
    }
    Ok(())
}

struct VersionFact {
    created_at: String,
    checksum: String,
    repository: String,
    license: String,
    rust_version: Option<String>,
    yanked: bool,
}

fn parse_versions(dir: &Path, inventory: &Inventory) -> Result<BTreeMap<String, VersionFact>, Fail> {
    let mut out = BTreeMap::new();
    for package in &inventory.packages {
        let path = dir.join(format!("version-{}.json", package.name));
        if !path.is_file() {
            return Err(fail::fail("scanner-output-missing", SCANNER_OUTPUT_MISSING, "version json missing"));
        }
        let root = read_json(&path, "malformed-record")?;
        let version = need(&root, "version")?;
        let rust_version = match version.get("rust_version") {
            Some(Val::Null) | None => None,
            Some(Val::String(text)) => Some(text.clone()),
            Some(_) => return Err(fail::fail("fake-claim", FAKE_CLAIM, "rust_version is not a string")),
        };
        out.insert(
            package.name.clone(),
            VersionFact {
                created_at: need_string(version, "created_at")?,
                checksum: need_string(version, "checksum")?,
                repository: need_string(version, "repository")?,
                license: need_string(version, "license")?,
                rust_version,
                yanked: need_bool(version, "yanked")?,
            },
        );
    }
    Ok(out)
}

fn parse_metadata(path: &Path, inventory: &Inventory) -> Result<BTreeMap<String, Resolved>, Fail> {
    let root = read_json(path, "malformed-record")?;
    let packages = need(&root, "packages")?.as_array()?;
    if packages.len() > 16 {
        return Err(fail::fail("bound", BOUND, "metadata package count"));
    }
    let mut by_name: BTreeMap<String, &Val> = BTreeMap::new();
    for package in packages {
        let name = need_string(package, "name")?;
        if by_name.insert(name.clone(), package).is_some() {
            return Err(fail::fail("duplicate", DUPLICATE, "duplicate metadata package"));
        }
    }
    if !by_name.contains_key("containment-supervisor") {
        return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "helper package missing from metadata"));
    }
    for name in by_name.keys() {
        if matches!(name.as_str(), "cc" | "bindgen" | "cmake" | "cc-rs") {
            return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "c compiler crate in graph"));
        }
    }
    let mut resolved = BTreeMap::new();
    let helper = by_name.get("containment-supervisor").unwrap();
    let nix_edge = direct_edge(helper, "nix")?;
    if nix_edge.kind != "normal" || nix_edge.uses_default || nix_edge.features != ["signal".to_string()] {
        return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "helper nix edge"));
    }
    resolved.insert(
        "nix".to_string(),
        Resolved {
            kind: nix_edge.kind,
            uses_default: nix_edge.uses_default,
            features: nix_edge.features,
            selected_by: "containment-supervisor".to_string(),
        },
    );
    let nix = by_name.get("nix").ok_or_else(|| fail::fail("graph-mismatch", GRAPH_MISMATCH, "nix missing"))?;
    let mut children = Vec::new();
    for dep in need(nix, "dependencies")?.as_array()? {
        if need_bool(dep, "optional")? || dep_kind(dep)? == "dev" {
            continue;
        }
        let name = need_string(dep, "name")?;
        children.push(name.clone());
        let kind = dep_kind(dep)?;
        let uses_default = need_bool(dep, "uses_default_features")?;
        let features = string_list(need(dep, "features")?)?;
        let feature_deps = feature_map(by_name.get(name.as_str()).copied())?;
        let activated = activate(&feature_deps, uses_default, &features)?;
        let expected = inventory
            .packages
            .iter()
            .find(|package| package.name == name)
            .ok_or_else(|| fail::fail("graph-mismatch", GRAPH_MISMATCH, "unexpected nix dependency"))?;
        if activated != expected.features.iter().cloned().collect::<BTreeSet<_>>() {
            return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, format!("features for {name}")));
        }
        resolved.insert(
            name,
            Resolved {
                kind,
                uses_default,
                features,
                selected_by: "nix".to_string(),
            },
        );
    }
    children.sort();
    if children.iter().map(String::as_str).collect::<Vec<_>>() != ["bitflags", "cfg-if", "cfg_aliases", "libc"] {
        return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "nix dependency set"));
    }
    Ok(resolved)
}

struct Edge {
    kind: String,
    uses_default: bool,
    features: Vec<String>,
}

fn direct_edge(package: &Val, name: &str) -> Result<Edge, Fail> {
    for dep in need(package, "dependencies")?.as_array()? {
        if need_string(dep, "name")? != name || need_bool(dep, "optional")? {
            continue;
        }
        return Ok(Edge {
            kind: dep_kind(dep)?,
            uses_default: need_bool(dep, "uses_default_features")?,
            features: string_list(need(dep, "features")?)?,
        });
    }
    Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "direct edge missing"))
}

fn dep_kind(dep: &Val) -> Result<String, Fail> {
    match dep.get("kind") {
        None | Some(Val::Null) => Ok("normal".to_string()),
        Some(Val::String(text)) => Ok(text.clone()),
        Some(_) => Err(fail::fail("malformed-record", MALFORMED, "dependency kind")),
    }
}

fn feature_map(package: Option<&Val>) -> Result<BTreeMap<String, Vec<String>>, Fail> {
    let package = package.ok_or_else(|| fail::fail("graph-mismatch", GRAPH_MISMATCH, "feature package missing"))?;
    let mut map = BTreeMap::new();
    for (name, value) in need(package, "features")?.as_object()? {
        map.insert(name.clone(), string_list(value)?);
    }
    Ok(map)
}

fn activate(map: &BTreeMap<String, Vec<String>>, uses_default: bool, requested: &[String]) -> Result<BTreeSet<String>, Fail> {
    let mut set = BTreeSet::new();
    if uses_default {
        set.insert("default".to_string());
    }
    for name in requested {
        set.insert(name.clone());
    }
    for _ in 0..8 {
        let current: Vec<String> = set.iter().cloned().collect();
        let mut grew = false;
        for name in current {
            if let Some(deps) = map.get(&name) {
                for dep in deps {
                    let feat = dep.split('/').next().unwrap_or(dep);
                    let feat = feat.strip_prefix("dep:").unwrap_or(feat);
                    if set.insert(feat.to_string()) {
                        grew = true;
                    }
                }
            }
        }
        if !grew {
            break;
        }
    }
    Ok(set)
}

fn check_audit_file(path: &Path, inventory: &Inventory) -> Result<(), Fail> {
    let root = read_json(path, "malformed-record")?;
    let database = need(&root, "database")?;
    let commit = need_string(database, "last-commit")?;
    let updated = need_string(database, "last-updated")?;
    if commit != inventory.rustsec_commit || updated != inventory.rustsec_commit_time {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "audit database identity"));
    }
    let count = need(need(&root, "lockfile")?, "dependency-count")?.as_int()?;
    if count != 6 {
        return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "audit dependency count"));
    }
    let vulns = need(&root, "vulnerabilities")?;
    if need_bool(vulns, "found")? || need(vulns, "count")?.as_int()? != 0 || !need(vulns, "list")?.as_array()?.is_empty() {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "audit reported a vulnerability"));
    }
    Ok(())
}

fn check_deny_file(path: &Path) -> Result<(), Fail> {
    let root = read_json(path, "malformed-record")?;
    let advisories = need(need(&root, "fields")?, "advisories")?;
    if need(advisories, "errors")?.as_int()? != 0 || need(advisories, "warnings")?.as_int()? != 0 {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "deny advisories were not empty"));
    }
    Ok(())
}

fn check_advisory_rev(path: &Path, inventory: &Inventory) -> Result<(), Fail> {
    let text = read_text(path)?;
    if !text.lines().any(|line| line.trim() == inventory.rustsec_commit)
        || !text.contains(&inventory.rustsec_commit_time)
        || !text.contains(&format!("time={}", inventory.recorded_at))
    {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "advisory rev does not match the scan"));
    }
    Ok(())
}

fn check_helper_manifest(path: &Path, _inventory: &Inventory) -> Result<(), Fail> {
    let text = read_text(path)?;
    let licenses: Vec<_> = text.lines().filter(|line| line.trim_start().starts_with("license ")).collect();
    if licenses != ["license = \"Apache-2.0\""] {
        return Err(fail::fail("wrong-expression", WRONG_EXPRESSION, "helper manifest license"));
    }
    Ok(())
}

fn load_records(dir: &Path, inventory: &Inventory) -> Result<BTreeMap<String, Val>, Fail> {
    let entries = fs::read_dir(dir).map_err(|_| fail::fail("scanner-output-missing", SCANNER_OUTPUT_MISSING, "records directory"))?;
    let mut files = Vec::new();
    let mut count = 0usize;
    for entry in entries {
        count += 1;
        if count > 16 {
            return Err(fail::fail("bound", BOUND, "too many record files"));
        }
        let entry = entry.map_err(|_| fail::fail("path-traversal", PATH_TRAVERSAL, "record directory entry"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        files.push(path);
    }
    files.sort();
    let mut out = BTreeMap::new();
    for path in files {
        let doc = read_json(&path, "malformed-record")?;
        let name = need_string(need(&doc, "package")?, "name")?;
        if !inventory.packages.iter().any(|package| package.name == name) {
            return Err(fail::fail("unknown-package", UNKNOWN_PACKAGE, name));
        }
        if out.insert(name.clone(), doc).is_some() {
            return Err(fail::fail("duplicate", DUPLICATE, name));
        }
    }
    Ok(out)
}

fn check_record(
    doc: &Val,
    package: &PinPkg,
    inventory: &Inventory,
    records: &BTreeMap<String, Val>,
    paths: &Paths,
    as_of: Stamp,
) -> Result<(), Fail> {
    let body = need(doc, "package")?;
    scan_sources(body)?;
    check_flags(doc, inventory)?;
    if need_string(body, "name")? != package.name || need_string(body, "version")? != package.version {
        return Err(fail::fail("unknown-package", UNKNOWN_PACKAGE, package.name.clone()));
    }
    if need_string(body, "official_source")? != inventory.registry_source || need_string(body, "repository")? != package.repository {
        return Err(fail::fail("wrong-source", WRONG_SOURCE, package.name.clone()));
    }
    let manifests = string_list(need(body, "manifests")?)?;
    if manifests != MANIFESTS {
        return Err(fail::fail("wrong-source", WRONG_SOURCE, format!("manifests {}", package.name)));
    }
    let expression = need_string(body, "license_spdx")?;
    if expression != package.expression {
        return Err(fail::fail("wrong-expression", WRONG_EXPRESSION, package.name.clone()));
    }
    let choice = optional_string(body, "dual_license_choice")?;
    check_choice(&expression, choice.as_deref(), package.choice.as_deref(), &package.name)?;
    check_time(body, package, inventory, as_of)?;
    check_msrv(body, package, inventory)?;
    if need_bool(body, "yanked")? {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "yanked crate"));
    }
    if need_bool(body, "default_features_disabled")? != package.default_off
        || string_list(need(body, "features_enabled")?)? != package.features
        || string_list(need(body, "selecting_features")?)? != package.selecting
        || need_string(body, "graph_role")? != package.graph_role
        || need_string(body, "dependency_kind")? != package.dependency_kind
        || need_string(body, "selected_by")? != package.selected_by
        || need_string(body, "native_link")? != package.native_link
        || need_bool(body, "c_compiler_crate_in_graph")?
    {
        return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, package.name.clone()));
    }
    let digest = need_string(body, "digest")?;
    if digest != format!("sha256:{}", package.checksum) {
        return Err(fail::fail("checksum-mismatch", CHECKSUM_MISMATCH, package.name.clone()));
    }
    check_license_files(body, package, paths)?;
    check_obligations(body, package)?;
    check_native(body, package, records)?;
    Ok(())
}

fn scan_sources(body: &Val) -> Result<(), Fail> {
    let mut pending = vec![need(body, "license_evidence")?];
    if let Some(native) = body.get("native_transitive") {
        for entry in native.as_array()? {
            if let Some(evidence) = entry.get("license_evidence") {
                pending.push(evidence);
            }
        }
    }
    for evidence in pending {
        for item in evidence.as_array()? {
            let source = need_string(item, "source")?;
            if source_is_traversal(&source) {
                return Err(fail::fail("path-traversal", PATH_TRAVERSAL, "license source"));
            }
        }
    }
    Ok(())
}

fn source_is_traversal(source: &str) -> bool {
    source.is_empty()
        || source.starts_with('/')
        || source.contains('\\')
        || source.contains('\0')
        || source.contains(':')
        || source.split('/').any(|part| part.is_empty() || part == "." || part == "..")
}

fn check_flags(doc: &Val, inventory: &Inventory) -> Result<(), Fail> {
    if need_bool(doc, "audited")?
        || need_bool(doc, "template")?
        || need_bool(doc, "admission")?
        || need_bool(doc, "full_at_sec_05")?
        || need_bool(doc, "release_signoff")?
        || !need_bool(doc, "do_not_treat_as_baseline")?
        || need_string(doc, "license_record_status")? != "complete-for-review"
    {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "record claims clearance"));
    }
    let historical = need(doc, "historical_scan")?;
    if need_bool(historical, "fresh_current_scan")?
        || need_string(historical, "rustsec_commit")? != inventory.rustsec_commit
        || need_string(historical, "rustsec_commit_time")? != inventory.rustsec_commit_time
        || need_string(historical, "recorded_at")? != inventory.recorded_at
        || need_string(historical, "cargo_audit")? != "0.22.2"
        || need_string(historical, "cargo_deny")? != "0.20.2"
    {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "historical scan claim"));
    }
    let gates = need(doc, "open_gates")?;
    for gate in GATES {
        if gates.get(gate).and_then(|value| value.as_str().ok()) != Some("open") {
            return Err(fail::fail("fake-claim", FAKE_CLAIM, gate));
        }
    }
    if !need(doc, "reviewers_assigned")?.as_array()?.is_empty() || !need(doc, "exception")?.is_null() {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "owner or exception"));
    }
    let body = need(doc, "package")?;
    if need_bool(body, "prohibited_license")? || !need(body, "weak_copyleft_review")?.is_null() {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "license clearance flag"));
    }
    if need_string(body, "seven_day_rule")? != "pass" {
        return Err(fail::fail("age-floor", fail::AGE_FLOOR, "seven day rule"));
    }
    let advisories = need(doc, "advisories")?;
    if !need_bool(advisories, "scan_recorded")?
        || need_string(advisories, "scanner")? != "cargo-audit 0.22.2"
        || need_string(advisories, "evidence")? != AUDIT_REL
        || need_string(advisories, "deny_evidence")? != DENY_REL
        || need_string(advisories, "disposition")? != "no-known-advisories"
        || need_bool(advisories, "known_exploitable_high_severity")?
    {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "advisory block"));
    }
    Ok(())
}

fn check_choice(expression: &str, choice: Option<&str>, expected: Option<&str>, name: &str) -> Result<(), Fail> {
    let has_or = expression.contains(" OR ");
    match (has_or, choice, expected) {
        (true, None, _) => Err(fail::fail("undeclared-choice", fail::UNDECLARED_CHOICE, name)),
        (true, Some(choice), Some(expected)) if choice == expected => Ok(()),
        (false, None, None) => Ok(()),
        _ => Err(fail::fail("wrong-dual-choice", WRONG_DUAL_CHOICE, name)),
    }
}

fn check_time(body: &Val, package: &PinPkg, inventory: &Inventory, as_of: Stamp) -> Result<(), Fail> {
    let published = timeutil::parse_rfc3339(&need_string(body, "publication_date")?)?;
    let adopted = timeutil::parse_rfc3339(&need_string(body, "adoption_date")?)?;
    if need_string(body, "publication_date")? != package.publication || need_string(body, "adoption_date")? != inventory.adoption {
        return Err(fail::fail("invalid-time", INVALID_TIME, package.name.clone()));
    }
    if published.nanos > as_of.nanos || adopted.nanos > as_of.nanos || adopted.nanos < published.nanos {
        return Err(fail::fail("invalid-time", INVALID_TIME, package.name.clone()));
    }
    let elapsed = adopted.nanos - published.nanos;
    let floor = timeutil::completed_days(elapsed);
    let stored = need_int(body, "age_days_at_adoption")?;
    if elapsed < timeutil::SEVEN_NS || stored != floor || stored != package.age {
        return Err(fail::fail("age-floor", fail::AGE_FLOOR, package.name.clone()));
    }
    Ok(())
}

fn check_msrv(body: &Val, package: &PinPkg, inventory: &Inventory) -> Result<(), Fail> {
    let msrv = need_string(body, "msrv")?;
    if msrv != package.msrv {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, format!("msrv {}", package.name)));
    }
    if package.msrv == inventory.undeclared_msrv && msrv.split('.').all(|part| part.chars().all(|ch| ch.is_ascii_digit())) {
        return Err(fail::fail("fake-claim", FAKE_CLAIM, "invented msrv"));
    }
    Ok(())
}

fn check_license_files(body: &Val, package: &PinPkg, paths: &Paths) -> Result<(), Fail> {
    let evidence = need(body, "license_evidence")?.as_array()?;
    let mut by_base: BTreeMap<String, &Val> = BTreeMap::new();
    for item in evidence {
        let source = need_string(item, "source")?;
        let base = source.rsplit('/').next().unwrap_or(source.as_str()).to_string();
        if by_base.insert(base, item).is_some() {
            return Err(fail::fail("duplicate", DUPLICATE, "duplicate license evidence"));
        }
    }
    let mut saw_notice = false;
    for file in &package.files {
        let canonical = paths
            .licenses
            .join(format!("{}-{}", package.name, package.version))
            .join(&file.basename);
        if !canonical.is_file() {
            return Err(missing_file(package, file));
        }
        if file.third_party {
            saw_notice = true;
        }
        let digest = sha::sha256_file(&canonical)?;
        if digest != file.sha256 {
            return Err(fail::fail("hash-mismatch", HASH_MISMATCH, format!("{} {}", package.name, file.basename)));
        }
        if file.in_record {
            let item = by_base.get(&file.basename).ok_or_else(|| missing_file(package, file))?;
            if need_string(item, "sha256")? != digest || need_string(item, "spdx")? != file.spdx {
                return Err(fail::fail("hash-mismatch", HASH_MISMATCH, format!("{} evidence {}", package.name, file.basename)));
            }
            let source = need_string(item, "source")?;
            let joined = join_under(&paths.repo_root, &paths.licenses, &source)?;
            if sha::sha256_file(&joined)? != digest {
                return Err(fail::fail("hash-mismatch", HASH_MISMATCH, package.name.clone()));
            }
        }
        let retained_name = if file.basename == "NOTICE" { "NOTICES.md" } else { file.basename.as_str() };
        let retained = paths
            .retained_licenses
            .join(format!("{}-{}", package.name, package.version))
            .join(retained_name);
        if !retained.is_file() {
            return Err(missing_file(package, file));
        }
        if read_bounded(&canonical)? != read_bounded(&retained)? || sha::sha256_file(&retained)? != digest {
            return Err(fail::fail("hash-mismatch", HASH_MISMATCH, format!("retained {}", package.name)));
        }
        check_body(package, file, &read_bounded(&canonical)?)?;
    }
    if package.name == "cfg_aliases" && !saw_notice {
        return Err(fail::fail("missing-third-party-notice", MISSING_THIRD_PARTY_NOTICE, "cfg_aliases"));
    }
    Ok(())
}

fn missing_file(package: &PinPkg, file: &PinFile) -> Fail {
    if file.spdx == "Apache-2.0" || file.basename.contains("APACHE") {
        fail::fail("omitted-apache-body", OMITTED_APACHE_BODY, package.name.clone())
    } else if file.third_party || file.basename.contains("NOTICE") {
        fail::fail("missing-third-party-notice", MISSING_THIRD_PARTY_NOTICE, package.name.clone())
    } else {
        fail::fail("missing-license-body", MISSING_LICENSE_BODY, format!("{} {}", package.name, file.basename))
    }
}

fn check_body(package: &PinPkg, file: &PinFile, bytes: &[u8]) -> Result<(), Fail> {
    let text = String::from_utf8_lossy(bytes);
    if file.basename == "LICENSE" || file.basename == "LICENSE-MIT" {
        if !text.contains(&package.copyright) || !text.contains("Permission is hereby granted") {
            return Err(fail::fail("placeholder", PLACEHOLDER, format!("copyright {}", package.name)));
        }
        if package.copyright.contains("[yyyy]") || package.copyright.contains("[name of copyright owner]") {
            return Err(fail::fail("placeholder", PLACEHOLDER, package.name.clone()));
        }
    }
    if file.basename.contains("APACHE")
        && (!text.contains("Apache License")
            || !text.contains("Version 2.0")
            || !text.contains("END OF TERMS AND CONDITIONS"))
    {
        return Err(fail::fail("omitted-apache-body", OMITTED_APACHE_BODY, package.name.clone()));
    }
    if file.third_party && (!text.contains("tectonic_cfg_support") || !text.contains("Permission is hereby granted")) {
        return Err(fail::fail("missing-third-party-notice", MISSING_THIRD_PARTY_NOTICE, package.name.clone()));
    }
    Ok(())
}

fn check_obligations(body: &Val, package: &PinPkg) -> Result<(), Fail> {
    let rows = need(body, "license_obligations")?.as_array()?;
    let mut covered = BTreeSet::new();
    for row in rows {
        let spdx = need_string(row, "spdx")?;
        let duties = need(row, "obligations")?.as_array()?;
        if duties.is_empty() {
            return Err(fail::fail("placeholder", PLACEHOLDER, package.name.clone()));
        }
        for duty in duties {
            let text = duty.as_str()?;
            if obligation_placeholder(text) {
                return Err(fail::fail("placeholder", PLACEHOLDER, package.name.clone()));
            }
        }
        let joined = duties.iter().map(|item| item.as_str().unwrap_or("")).collect::<Vec<_>>().join(" ");
        if spdx == "MIT" && (!joined.contains("copyright notice") || !joined.contains("permission notice")) {
            return Err(fail::fail("placeholder", PLACEHOLDER, format!("mit duty {}", package.name)));
        }
        if spdx == "Apache-2.0"
            && (!joined.contains("copy of this License") || !joined.contains("prominent notices") || !joined.contains("NOTICE"))
        {
            return Err(fail::fail("placeholder", PLACEHOLDER, format!("apache duty {}", package.name)));
        }
        covered.insert(spdx);
    }
    for id in declared(&package.expression)? {
        if !covered.contains(id) {
            return Err(fail::fail("placeholder", PLACEHOLDER, format!("missing duty {id}")));
        }
    }
    if package.name == "cfg_aliases" {
        let joined = rows
            .iter()
            .flat_map(|row| need(row, "obligations").ok().into_iter().flat_map(|value| value.as_array().ok().into_iter().flatten()))
            .filter_map(|item| item.as_str().ok())
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.contains("tectonic_cfg_support") || !joined.contains("NOTICES.md") || !joined.contains("Katharos Technology") {
            return Err(fail::fail("missing-third-party-notice", MISSING_THIRD_PARTY_NOTICE, "cfg_aliases duty"));
        }
    }
    Ok(())
}

fn declared(expression: &str) -> Result<Vec<&str>, Fail> {
    match expression {
        "MIT" => Ok(vec!["MIT"]),
        "MIT OR Apache-2.0" => Ok(vec!["MIT", "Apache-2.0"]),
        _ => Err(fail::fail("wrong-expression", WRONG_EXPRESSION, expression)),
    }
}

fn check_native(body: &Val, package: &PinPkg, records: &BTreeMap<String, Val>) -> Result<(), Fail> {
    let native = need(body, "native_transitive")?.as_array()?;
    if package.name != "nix" {
        if !native.is_empty() {
            return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, package.name.clone()));
        }
        return Ok(());
    }
    let mut names = Vec::new();
    for entry in native {
        let name = need_string(entry, "name")?;
        names.push(name.clone());
        let child = records.get(&name).ok_or_else(|| fail::fail("graph-mismatch", GRAPH_MISMATCH, name.clone()))?;
        let child_body = need(child, "package")?;
        if need_string(entry, "version")? != need_string(child_body, "version")?
            || need_string(entry, "license_spdx")? != need_string(child_body, "license_spdx")?
            || optional_string(entry, "dual_license_choice")? != optional_string(child_body, "dual_license_choice")?
            || need_string(entry, "digest")? != need_string(child_body, "digest")?
            || need_string(entry, "dependency_kind")? != need_string(child_body, "dependency_kind")?
        {
            return Err(fail::fail("wrong-expression", WRONG_EXPRESSION, format!("native {name}")));
        }
        if !need(entry, "weak_copyleft_review")?.is_null() {
            return Err(fail::fail("fake-claim", FAKE_CLAIM, "native weak copyleft"));
        }
    }
    names.sort();
    if names.iter().map(String::as_str).collect::<Vec<_>>() != ["bitflags", "cfg-if", "cfg_aliases", "libc"] {
        return Err(fail::fail("graph-mismatch", GRAPH_MISMATCH, "nix native set"));
    }
    Ok(())
}

fn obligation_placeholder(value: &str) -> bool {
    let folded = value
        .chars()
        .map(|ch| match ch {
            '/' | '_' | '.' | '-' => ' ',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    let compact = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(
        compact.as_str(),
        "" | "n a"
            | "na"
            | "no"
            | "none"
            | "not applicable"
            | "not reviewed"
            | "pending"
            | "true"
            | "unknown"
            | "yes"
    )
}

fn join_under(repo: &Path, licenses: &Path, source: &str) -> Result<PathBuf, Fail> {
    if source_is_traversal(source) {
        return Err(fail::fail("path-traversal", PATH_TRAVERSAL, "license source"));
    }
    let joined = repo.join(source);
    let repo_root = repo
        .canonicalize()
        .map_err(|_| fail::fail("path-traversal", PATH_TRAVERSAL, "repo root"))?;
    let license_root = licenses
        .canonicalize()
        .map_err(|_| fail::fail("missing-license-body", MISSING_LICENSE_BODY, "license root"))?;
    let canon = joined
        .canonicalize()
        .map_err(|_| fail::fail("missing-license-body", MISSING_LICENSE_BODY, "license source missing"))?;
    if !canon.starts_with(&repo_root) || !canon.starts_with(&license_root) {
        return Err(fail::fail("path-traversal", PATH_TRAVERSAL, "license source escapes"));
    }
    Ok(canon)
}

fn parse_lock(path: &Path) -> Result<BTreeMap<String, (String, String)>, Fail> {
    let text = read_text(path)?;
    let mut out = BTreeMap::new();
    let mut name = None;
    let mut version = None;
    let mut checksum = None;
    let mut source = None;
    let finish = |name: &mut Option<String>, version: &mut Option<String>, checksum: &mut Option<String>, source: &mut Option<String>, out: &mut BTreeMap<String, (String, String)>| -> Result<(), Fail> {
        if let Some(name) = name.take() {
            let version = version.take().ok_or_else(|| fail::fail("malformed-record", MALFORMED, "lock version"))?;
            if name == "containment-supervisor" {
                checksum.take();
                source.take();
            } else {
                let checksum = checksum.take().ok_or_else(|| fail::fail("checksum-mismatch", CHECKSUM_MISMATCH, name.clone()))?;
                let source = source.take().unwrap_or_default();
                if source != "registry+https://github.com/rust-lang/crates.io-index" {
                    return Err(fail::fail("wrong-source", WRONG_SOURCE, name));
                }
                if out.insert(name.clone(), (version, checksum)).is_some() {
                    return Err(fail::fail("duplicate", DUPLICATE, name));
                }
            }
        }
        Ok(())
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            finish(&mut name, &mut version, &mut checksum, &mut source, &mut out)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            name = Some(unquote(value)?);
        } else if let Some(value) = line.strip_prefix("version = ") {
            if name.is_some() && version.is_none() {
                version = Some(unquote(value)?);
            }
        } else if let Some(value) = line.strip_prefix("source = ") {
            source = Some(unquote(value)?);
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            checksum = Some(unquote(value)?);
        }
    }
    finish(&mut name, &mut version, &mut checksum, &mut source, &mut out)?;
    if out.keys().any(|name| name == "MISSING_CRATE") {
        return Err(fail::fail("unknown-package", UNKNOWN_PACKAGE, "MISSING_CRATE"));
    }
    Ok(out)
}

fn unquote(value: &str) -> Result<String, Fail> {
    let value = value.trim().trim_end_matches(',');
    let value = value.strip_prefix('"').and_then(|text| text.strip_suffix('"')).ok_or_else(|| {
        fail::fail("malformed-record", MALFORMED, "lock string")
    })?;
    Ok(value.to_string())
}

fn parse_age(path: &Path) -> Result<BTreeMap<String, (String, String)>, Fail> {
    let text = read_text(path)?;
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if !line.starts_with("spec=") || !line.contains("version_created_at=") {
            continue;
        }
        let mut spec = None;
        let mut created = None;
        let mut second = None;
        for token in line.split_whitespace() {
            if let Some(value) = token.strip_prefix("spec=") {
                spec = Some(value.to_string());
            } else if let Some(value) = token.strip_prefix("version_created_at=") {
                created = Some(value.to_string());
            } else if let Some(value) = token.strip_prefix("second_created_at=") {
                second = Some(value.to_string());
            }
        }
        let spec = spec.ok_or_else(|| fail::fail("invalid-time", INVALID_TIME, "age spec"))?;
        let created = created.ok_or_else(|| fail::fail("invalid-time", INVALID_TIME, "age created"))?;
        let second = second.ok_or_else(|| fail::fail("invalid-time", INVALID_TIME, "age second"))?;
        if out.insert(spec.clone(), (created, second)).is_some() {
            return Err(fail::fail("duplicate", DUPLICATE, spec));
        }
    }
    Ok(out)
}

fn expect_hash(path: &Path, expected: &str, code: &'static str, exit_code: i32) -> Result<(), Fail> {
    if !path.is_file() {
        return Err(fail::fail(code, exit_code, "required file missing"));
    }
    let digest = sha::sha256_file(path)?;
    if digest != expected {
        return Err(fail::fail(code, exit_code, "digest mismatch"));
    }
    Ok(())
}

fn require_present(path: &Path) -> Result<(), Fail> {
    if path.is_file() {
        Ok(())
    } else {
        Err(fail::fail("scanner-output-missing", SCANNER_OUTPUT_MISSING, "required scanner output missing"))
    }
}

fn read_text(path: &Path) -> Result<String, Fail> {
    let bytes = read_bounded(path)?;
    String::from_utf8(bytes).map_err(|_| fail::fail("malformed-record", MALFORMED, "utf-8"))
}

fn read_json(path: &Path, _code: &str) -> Result<Val, Fail> {
    json::parse(&read_text(path)?)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, Fail> {
    let meta = fs::symlink_metadata(path).map_err(|_| fail::fail("missing-license-body", MISSING_LICENSE_BODY, "unreadable file"))?;
    if !meta.file_type().is_file() {
        return Err(fail::fail("path-traversal", PATH_TRAVERSAL, "refusing symlink or non-file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(fail::fail("bound", BOUND, "file too large"));
    }
    fs::read(path).map_err(|_| fail::fail("missing-license-body", MISSING_LICENSE_BODY, "read failed"))
}

fn need<'a>(value: &'a Val, key: &str) -> Result<&'a Val, Fail> {
    value
        .get(key)
        .ok_or_else(|| fail::fail("malformed-record", MALFORMED, format!("missing {key}")))
}

fn need_string(value: &Val, key: &str) -> Result<String, Fail> {
    Ok(need(value, key)?.as_str()?.to_string())
}

fn need_bool(value: &Val, key: &str) -> Result<bool, Fail> {
    need(value, key)?.as_bool()
}

fn need_int(value: &Val, key: &str) -> Result<i64, Fail> {
    let item = need(value, key)?;
    match item {
        Val::Int(number) => Ok(*number),
        Val::Bool(_) | Val::Frac => Err(fail::fail("age-floor", fail::AGE_FLOOR, key)),
        _ => Err(fail::fail("malformed-record", MALFORMED, key)),
    }
}

fn optional_string(value: &Val, key: &str) -> Result<Option<String>, Fail> {
    let item = need(value, key)?;
    if item.is_null() {
        Ok(None)
    } else {
        Ok(Some(item.as_str()?.to_string()))
    }
}

fn string_list(value: &Val) -> Result<Vec<String>, Fail> {
    let mut out = Vec::new();
    for item in value.as_array()? {
        out.push(item.as_str()?.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::obligation_placeholder;

    #[test]
    fn placeholders_are_not_duties() {
        assert!(obligation_placeholder("n/a"));
        assert!(obligation_placeholder(" Not-Reviewed "));
        assert!(!obligation_placeholder(
            "Include the copyright notice and this permission notice in all copies or substantial portions of the Software."
        ));
    }
}
