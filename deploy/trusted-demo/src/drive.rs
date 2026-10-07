//! Public-fixture driver for the trusted-operator demo.
//!
//! It writes configs, binds declared review decisions to captured fingerprints,
//! and checks outputs of the reviewed CLIs. It does not insert target rows,
//! invent pack bytes, or treat the fixture label as a human approval.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXED: &str = "25dfc9022780c3deff12f37b7ad7ea4ed1fb6480";
const SOURCE_ID: &str = "public_shop";
const MARKER: &str = "甲注评";
const REVIEWER: &str = "public-fixture";

struct Deployment {
    owner: String,
    run_id: String,
}

impl Deployment {
    fn host(&self) -> String {
        format!("oo-{}-pg", self.owner)
    }

    fn database(&self) -> String {
        format!("oo_{}_src", self.owner)
    }

    fn dst(&self) -> String {
        format!("oo_{}_dst", self.owner)
    }

    fn reader(&self) -> String {
        format!("oo_{}_reader", self.owner)
    }

    fn runtime(&self) -> String {
        format!("oo_{}_runtime", self.owner)
    }

    fn login(&self) -> String {
        format!("oo_{}_login", self.owner)
    }

    fn secret_ref(&self) -> String {
        format!("secret://pg/{}/reader", self.owner)
    }

    fn shard(&self) -> String {
        format!("{}main", self.owner)
    }

    fn ont(&self) -> String {
        format!("ont-{}", self.owner)
    }

    fn cat(&self) -> String {
        format!("cat-{}", self.owner)
    }

    fn pol(&self) -> String {
        format!("pol-{}", self.owner)
    }

    fn know(&self) -> String {
        format!("know-{}", self.owner)
    }
}

fn canonical_deployment(owner: &str, run_id: &str) -> String {
    format!(
        "{{\"protocol\":\"oo.deploy.trusted-demo-deployment/v1\",\"project\":\"open-ontology\",\"fixed_source\":\"{FIXED}\",\"owner\":\"{owner}\",\"run_id\":\"{run_id}\",\"source_id\":\"public_shop\"}}"
    )
}

fn load_deployment(bytes: &[u8]) -> Result<Deployment, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "manifest utf-8".to_owned())?;
    let line = match text.strip_suffix('\n') {
        Some(rest) => rest,
        None => text,
    };
    if line.contains('\n') || line.contains('\r') {
        return Err("manifest must be one line".to_owned());
    }
    let value = parse_slice(line.as_bytes())?;
    let deployment = deployment_from(&value)?;
    if line != canonical_deployment(&deployment.owner, &deployment.run_id) {
        return Err("manifest is not canonical".to_owned());
    }
    Ok(deployment)
}

fn deployment_from(value: &Val) -> Result<Deployment, String> {
    let Val::Obj(items) = value else {
        return Err("manifest object".to_owned());
    };
    if items.len() != 6 {
        return Err("manifest fields".to_owned());
    }
    expect_str(value, "protocol", "oo.deploy.trusted-demo-deployment/v1")?;
    expect_str(value, "project", "open-ontology")?;
    expect_str(value, "fixed_source", FIXED)?;
    expect_str(value, "source_id", SOURCE_ID)?;
    let owner = str_field(value, "owner")?.to_owned();
    let run_id = str_field(value, "run_id")?.to_owned();
    if !owner_token(&owner) || !uuid_lower(&run_id) || !run_id.ends_with(&owner) {
        return Err("manifest identity".to_owned());
    }
    Ok(Deployment { owner, run_id })
}

fn owner_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 4 && bytes.iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn uuid_lower(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen_slot = matches!(index, 8 | 13 | 18 | 23);
        if hyphen_slot && *byte != b'-' {
            return false;
        }
        if !hyphen_slot && (!byte.is_ascii_hexdigit() || byte.is_ascii_uppercase()) {
            return false;
        }
    }
    true
}

fn load_deployment_file(path: &Path) -> Result<Deployment, String> {
    let bytes = fs::read(path).map_err(|_| "deployment manifest missing".to_owned())?;
    if bytes.len() > 512 {
        return Err("deployment manifest is too large".to_owned());
    }
    load_deployment(&bytes)
}

const DECISIONS: &[(&str, &str, &str, &str)] = &[
    ("shop.customer", "customer_id", "none", "identifier"),
    ("shop.customer", "display_name", "personal", "person_name"),
    ("shop.customer", "active", "none", "boolean_flag"),
    ("shop.order_doc", "order_id", "none", "identifier"),
    ("shop.order_doc", "customer_id", "none", "foreign_key"),
    ("shop.order_doc", "memo", "none", "free_text"),
    ("shop.order_doc", "posted", "none", "boolean_flag"),
    ("shop.order_doc", "amount", "none", "quantity"),
    ("shop.order_doc", "clerk_note", "sensitive", "free_text"),
];

fn main() {
    if let Err(err) = entry() {
        eprintln!("drive: {err}");
        std::process::exit(1);
    }
}

fn entry() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or_else(|| "usage: drive public-fixture|smoke".to_owned())?;
    if args.next().is_some() {
        return Err("usage: drive public-fixture|smoke".to_owned());
    }
    match mode.as_str() {
        "public-fixture" => public_fixture(),
        "smoke" => smoke(),
        other => Err(format!("usage: unknown mode {other}")),
    }
}

struct Paths {
    onboarding: PathBuf,
    bins: PathBuf,
    secrets: PathBuf,
    run: PathBuf,
    work: PathBuf,
    bad: PathBuf,
    declared: PathBuf,
    evidence: PathBuf,
}

fn paths() -> Result<Paths, String> {
    Ok(Paths {
        onboarding: env_path("OO_DEMO_ONBOARDING")?,
        bins: env_path("OO_DEMO_BINS")?,
        secrets: env_path("OO_DEMO_SECRETS")?,
        run: env_path("OO_DEMO_RUN")?,
        work: env_path("OO_DEMO_WORK")?,
        bad: env_path("OO_DEMO_BAD")?,
        declared: env_path("OO_DEMO_DECLARED")?,
        evidence: env_path("OO_DEMO_EVIDENCE")?,
    })
}

fn env_path(key: &str) -> Result<PathBuf, String> {
    let value = env::var(key).map_err(|_| format!("missing {key}"))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{key} is not absolute"));
    }
    Ok(path)
}

fn public_fixture() -> Result<(), String> {
    let paths = paths()?;
    let deployment = load_deployment_file(Path::new("/deployment.json"))?;
    let fixed = fs::read_to_string("/src/.fixed-base-sha").map_err(|_| "fixed sha missing".to_owned())?;
    if fixed.trim() != FIXED {
        return Err("archive is not the fixed commit".to_owned());
    }
    if paths.run.exists() {
        return Err("run directory already exists".to_owned());
    }
    fs::create_dir_all(&paths.work).map_err(|_| "work dir".to_owned())?;
    let parent = paths.run.parent().ok_or_else(|| "run parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|_| "run parent".to_owned())?;

    let pins = pin_all(&paths.bins)?;
    let inspect_path = paths.work.join("inspect.json");
    fs::write(&inspect_path, inspect_json(&deployment, &pins, &paths.secrets)?).map_err(|_| "write inspect".to_owned())?;
    let inspect = run_bin(
        &paths.onboarding,
        &["inspect", "--config", inspect_path.to_str().ok_or("path")?, "--run", paths.run.to_str().ok_or("path")?],
    )?;
    if !inspect.status.success() {
        return Err(format!("inspect exit {}", status_code(&inspect.status)));
    }
    let outcome = parse_file(&paths.run.join("outcome.json"))?;
    expect_str(&outcome, "status", "awaiting_review")?;
    expect_str(&outcome, "phase", "inspect")?;
    let manifest = parse_file(&paths.run.join("inspect-manifest.json"))?;
    expect_num(&manifest, "source_cut", "48")?;
    expect_num(&manifest, "mapping_run_generation", "48")?;
    let fence = num_str(get(&manifest, "target_fence")?)?;
    if fence == "48" {
        return Err("target fence collided with source cut".to_owned());
    }
    let identity = str_field(&manifest, "config_identity_sha256")?.to_owned();
    let recomputed = sha256_bytes(identity_preimage(&deployment).as_bytes());
    if identity != recomputed {
        return Err("config identity mismatch".to_owned());
    }
    refuse_marker(&paths.run.join("catalog.json"))?;
    refuse_marker(&paths.run.join("captured-draft.json"))?;
    fs::copy(paths.run.join("outcome.json"), paths.work.join("inspect-outcome.json")).map_err(|_| "copy outcome".to_owned())?;

    let captured = parse_file(&paths.run.join("captured-draft.json"))?;
    let declared = parse_file(&paths.declared)?;
    let bound = bind_declared(&captured, &declared)?;
    fs::write(paths.work.join("reviews.json"), &bound.reviews).map_err(|_| "write reviews".to_owned())?;
    fs::write(paths.work.join("decisions.json"), &bound.decisions).map_err(|_| "write decisions".to_owned())?;
    fs::write(paths.work.join("bad-decisions.json"), &bound.bad_decisions).map_err(|_| "write bad decisions".to_owned())?;
    write_visibility(&paths.work.join("vis-north.json"), &deployment, "oo_r_north", true)?;
    write_visibility(&paths.work.join("vis-narrow.json"), &deployment, "oo_r_narrow", false)?;
    write_operator_stage(&paths)?;

    copy_tree(&paths.run, &paths.bad)?;
    let bad_apply = paths.work.join("bad-apply.json");
    fs::write(&bad_apply, apply_json(&deployment, &pins, &paths, "bad-decisions.json")?).map_err(|_| "write bad apply".to_owned())?;
    let bad = run_bin(
        &paths.onboarding,
        &["apply-reviewed", "--config", bad_apply.to_str().ok_or("path")?, "--run", paths.bad.to_str().ok_or("path")?],
    )?;
    check_bad_review(&paths.bad, &bad)?;

    let apply_path = paths.work.join("apply.json");
    fs::write(&apply_path, apply_json(&deployment, &pins, &paths, "decisions.json")?).map_err(|_| "write apply".to_owned())?;
    let apply = run_bin(
        &paths.onboarding,
        &[
            "apply-reviewed",
            "--config",
            apply_path.to_str().ok_or("path")?,
            "--run",
            paths.run.to_str().ok_or("path")?,
        ],
    )?;
    if !apply.status.success() {
        return Err(format!("apply-reviewed exit {}", status_code(&apply.status)));
    }
    check_ready(&paths.run)?;
    check_queries(&paths.run)?;
    let secrets = load_secrets(&paths.secrets)?;
    let scan_path = paths.evidence.join("secret-scan.txt");
    scan_tree(&paths.evidence, &secrets, &scan_path)?;
    write_receipt(&paths, &deployment, &identity, &bound.schema_hash)?;
    println!("ready_for_trusted_query");
    Ok(())
}

fn smoke() -> Result<(), String> {
    let paths = paths()?;
    let deployment = load_deployment_file(Path::new("/deployment.json"))?;
    let receipt = parse_file(&paths.evidence.join("live/receipt.json"))?;
    expect_str(&receipt, "status", "ready_for_trusted_query")?;
    expect_str(&receipt, "fixed_commit", FIXED)?;
    expect_str(&receipt, "project", "open-ontology")?;
    expect_str(&receipt, "owner", &deployment.owner)?;
    expect_str(&receipt, "run_id", &deployment.run_id)?;
    expect_str(&receipt, "source_id", SOURCE_ID)?;
    let north = str_field(&receipt, "role_pack_north_sha256")?;
    let narrow = str_field(&receipt, "role_pack_narrow_sha256")?;
    let north_now = sha256_file(&paths.run.join("steps/role-pack-north.stdout"))?;
    let narrow_now = sha256_file(&paths.run.join("steps/role-pack-narrow.stdout"))?;
    if north_now != north || narrow_now != narrow {
        return Err("role pack hash changed".to_owned());
    }
    let request = fs::read(paths.run.join("queries/north-customer.request.json")).map_err(|_| "request missing".to_owned())?;
    let mut value = parse_slice(&request)?;
    let audit = paths.evidence.join("live/smoke/north-customer.jsonl");
    if let Some(parent) = audit.parent() {
        fs::create_dir_all(parent).map_err(|_| "smoke dir".to_owned())?;
    }
    let audit_text = audit.to_str().ok_or("audit path")?.to_owned();
    set_str(&mut value, "audit_file", &audit_text)?;
    let rewritten = paths.work.join("smoke-north-customer.request.json");
    fs::write(&rewritten, val_to_json(&value)).map_err(|_| "write smoke request".to_owned())?;
    let query_bin = paths.bins.join("query-pg");
    let repeated = run_bin(&query_bin, &["--request", rewritten.to_str().ok_or("path")?])?;
    if !repeated.status.success() {
        return Err(format!("smoke query exit {}", status_code(&repeated.status)));
    }
    fs::write(paths.evidence.join("live/smoke/north-customer.stdout"), &repeated.stdout).map_err(|_| "write smoke stdout".to_owned())?;
    same_rows(
        &paths.run.join("steps/query-north-customer.stdout"),
        &paths.evidence.join("live/smoke/north-customer.stdout"),
    )?;
    let secrets = load_secrets(&paths.secrets)?;
    scan_tree(&paths.evidence, &secrets, &paths.evidence.join("smoke-secret-scan.txt"))?;
    fs::write(paths.evidence.join("live/smoke/smoke-ok.txt"), b"role_pack_unchanged\nquery_rows_repeated\n").map_err(|_| "smoke marker".to_owned())?;
    println!("smoke_repeated");
    Ok(())
}

fn pin_all(dir: &Path) -> Result<Vec<(&'static str, String, String)>, String> {
    let names = [
        "init-pg",
        "oo-pipeline",
        "oo-builder",
        "reviewed-effective-oos",
        "reviewed-policy",
        "install-column-acl",
        "provision-mapping-reader",
        "analyze-pg",
        "role-pack-bundle",
        "query-pg",
    ];
    let mut pins = Vec::new();
    for name in names {
        let path = dir.join(name);
        let digest = sha256_file(&path)?;
        pins.push((name, path.to_str().ok_or("pin path")?.to_owned(), digest));
    }
    Ok(pins)
}

fn pin<'a>(pins: &'a [(&str, String, String)], name: &str) -> Result<(&'a String, &'a String), String> {
    pins.iter()
        .find(|item| item.0 == name)
        .map(|item| (&item.1, &item.2))
        .ok_or_else(|| format!("missing pin {name}"))
}

fn inspect_json(deployment: &Deployment, pins: &[(&str, String, String)], secrets: &Path) -> Result<String, String> {
    let mut out = String::from("{\"protocol\":\"oo.onboarding.inspect/v1\",\"caller\":\"trusted_local_admin\",\"binaries\":{");
    push_named_pin(&mut out, "init_pg", pin(pins, "init-pg")?)?;
    out.push(',');
    push_named_pin(&mut out, "import_pg", pin(pins, "oo-pipeline")?)?;
    out.push(',');
    push_named_pin(&mut out, "catalog_pg", pin(pins, "oo-pipeline")?)?;
    out.push(',');
    push_named_pin(&mut out, "captured_pg_draft", pin(pins, "oo-builder")?)?;
    out.push_str("},\"source\":{\"source_id\":");
    push_string(&mut out, SOURCE_ID);
    out.push_str(",\"host\":");
    push_string(&mut out, &deployment.host());
    out.push_str(",\"port\":5432,\"database\":");
    push_string(&mut out, &deployment.database());
    out.push_str(",\"username\":");
    push_string(&mut out, &deployment.reader());
    out.push_str(",\"secret_ref\":");
    push_string(&mut out, &deployment.secret_ref());
    out.push_str(",\"password_file\":");
    push_string(&mut out, &path_str(&secrets.join("reader.pass"))?);
    out.push_str(",\"ca_file\":");
    push_string(&mut out, &path_str(&secrets.join("ca.crt"))?);
    out.push_str(",\"tables\":[{\"schema\":\"shop\",\"table\":\"customer\"},{\"schema\":\"shop\",\"table\":\"order_doc\"}]},\"target\":{\"host\":");
    push_string(&mut out, &deployment.host());
    out.push_str(",\"port\":5432,\"database\":");
    push_string(&mut out, &deployment.dst());
    out.push_str(",\"administrator\":\"oo_fixture_admin\",\"runtime_principal\":");
    push_string(&mut out, &deployment.runtime());
    out.push_str(",\"admin_password_file\":");
    push_string(&mut out, &path_str(&secrets.join("admin.pass"))?);
    out.push_str(",\"runtime_password_file\":");
    push_string(&mut out, &path_str(&secrets.join("runtime.pass"))?);
    out.push_str(",\"ca_file\":");
    push_string(&mut out, &path_str(&secrets.join("ca.crt"))?);
    out.push_str("},\"shard\":");
    push_string(&mut out, &deployment.shard());
    out.push_str(",\"owner\":");
    push_string(&mut out, &deployment.run_id);
    out.push_str(",\"source_generation\":48,\"connect_ms\":30000,\"runtime_membership_ready_file\":");
    push_string(&mut out, "/ready/runtime.ready");
    out.push_str(",\"membership_wait_ms\":180000,\"comment_markers\":[");
    push_string(&mut out, MARKER);
    out.push_str("],\"deadlines_ms\":{\"import_not_ready_ms\":180000,\"init_ms\":180000,\"import_ms\":180000,\"catalog_ms\":180000,\"captured_ms\":180000}}");
    Ok(out)
}

fn apply_json(deployment: &Deployment, pins: &[(&str, String, String)], paths: &Paths, decisions_name: &str) -> Result<String, String> {
    let mut out = String::from("{\"protocol\":\"oo.onboarding.apply-reviewed/v1\",\"caller\":\"trusted_local_admin\",\"binaries\":{");
    let pairs = [
        ("review_pg_draft", "oo-builder"),
        ("reviewed_effective_oos", "reviewed-effective-oos"),
        ("reviewed_policy", "reviewed-policy"),
        ("install_column_acl", "install-column-acl"),
        ("provision_mapping_reader", "provision-mapping-reader"),
        ("analyze_pg", "analyze-pg"),
        ("role_pack", "oo-builder"),
        ("role_pack_bundle", "role-pack-bundle"),
        ("query_pg", "query-pg"),
    ];
    for (index, (field, bin)) in pairs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_named_pin(&mut out, field, pin(pins, bin)?)?;
    }
    out.push_str("},\"reviews_file\":");
    push_string(&mut out, &path_str(&paths.work.join("reviews.json"))?);
    out.push_str(",\"decisions_file\":");
    push_string(&mut out, &path_str(&paths.work.join(decisions_name))?);
    out.push_str(",\"settings\":");
    out.push_str(&settings_json(deployment));
    out.push_str(",\"schema\":\"shop\",\"query_login\":{\"user\":");
    push_string(&mut out, &deployment.login());
    out.push_str(",\"password_file\":");
    push_string(&mut out, &path_str(&paths.secrets.join("login.pass"))?);
    out.push_str("},\"roles\":[");
    push_role(&mut out, "north", true, &path_str(&paths.work.join("vis-north.json"))?)?;
    out.push(',');
    push_role(&mut out, "narrow", false, &path_str(&paths.work.join("vis-narrow.json"))?)?;
    out.push_str("],\"analyze\":{\"statement_timeout_ms\":30000,\"lock_timeout_ms\":30000,\"connect_ms\":30000}}");
    Ok(out)
}

fn settings_json(deployment: &Deployment) -> String {
    let mut out = String::from("{\"project\":");
    push_string(&mut out, SOURCE_ID);
    out.push_str(",\"ontology_version\":");
    push_string(&mut out, &deployment.ont());
    out.push_str(",\"catalog_version\":");
    push_string(&mut out, &deployment.cat());
    out.push_str(",\"policy\":{\"id\":");
    push_string(&mut out, &deployment.pol());
    out.push_str(",\"version\":\"1\"},\"knowledge\":{\"id\":");
    push_string(&mut out, &deployment.know());
    out.push_str(",\"version\":\"1\"},\"ontology\":{\"spec_version\":\"oos/v1\",\"name\":\"shop\",\"locale\":\"en-US\",\"include_schemas\":[\"shop\"],\"default_tier_preference\":[\"raw\"],\"publish\":{\"include_unreviewed\":[]},\"classification\":{\"auto_accept\":\"rule_high_confidence\"},\"lifecycle\":{\"deprecation_min_releases\":2},\"compile\":{\"per_role\":true,\"budgets\":{\"readme_tokens\":2000,\"table_file_tokens\":3000},\"sample_values\":{\"default\":\"masked\",\"max_per_column\":0}}}}");
    out
}

fn push_named_pin(out: &mut String, field: &str, pin: (&String, &String)) -> Result<(), String> {
    out.push('"');
    out.push_str(field);
    out.push_str("\":{\"path\":");
    push_string(out, pin.0);
    out.push_str(",\"sha256\":");
    push_string(out, pin.1);
    out.push('}');
    Ok(())
}

fn push_role(out: &mut String, slug: &str, north: bool, visibility: &str) -> Result<(), String> {
    out.push_str("{\"slug\":");
    push_string(out, slug);
    out.push_str(",\"allow\":[");
    let columns: &[(&str, &str)] = if north { NORTH_COLUMNS } else { NARROW_COLUMNS };
    for (index, (dataset, column)) in columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let pii = decision_pii(dataset, column)?;
        out.push_str("{\"dataset\":");
        push_string(out, dataset);
        out.push_str(",\"column\":");
        push_string(out, column);
        out.push_str(",\"explicit_allow\":");
        out.push_str(if pii == "personal" || pii == "sensitive" { "true" } else { "false" });
        out.push('}');
    }
    out.push_str("],\"visibility_file\":");
    push_string(out, visibility);
    out.push_str(",\"queries\":[");
    let queries: &[QuerySpec] = if north { NORTH_QUERIES } else { NARROW_QUERIES };
    for (index, query) in queries.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_query(out, query);
    }
    out.push_str("]}");
    Ok(())
}

fn decision_pii(dataset: &str, column: &str) -> Result<&'static str, String> {
    DECISIONS
        .iter()
        .find(|item| item.0 == dataset && item.1 == column)
        .map(|item| item.2)
        .ok_or_else(|| format!("allow column {dataset}.{column} is not declared"))
}

struct QuerySpec {
    name: &'static str,
    when: &'static str,
    exit_code: i32,
    stderr: Option<&'static str>,
    sql: &'static str,
}

const NORTH_COLUMNS: &[(&str, &str)] = &[
    ("shop.customer", "customer_id"),
    ("shop.customer", "display_name"),
    ("shop.customer", "active"),
    ("shop.order_doc", "order_id"),
    ("shop.order_doc", "customer_id"),
    ("shop.order_doc", "memo"),
    ("shop.order_doc", "posted"),
    ("shop.order_doc", "amount"),
    ("shop.order_doc", "clerk_note"),
];

const NARROW_COLUMNS: &[(&str, &str)] = &[
    ("shop.customer", "customer_id"),
    ("shop.customer", "display_name"),
    ("shop.order_doc", "order_id"),
    ("shop.order_doc", "customer_id"),
    ("shop.order_doc", "posted"),
];

const NORTH_QUERIES: &[QuerySpec] = &[
    QuerySpec { name: "north-before", when: "before_analyze", exit_code: 5, stderr: Some("query-pg: policy"), sql: "SELECT customer_id FROM shop.customer ORDER BY customer_id" },
    QuerySpec { name: "north-hidden", when: "after_bundle", exit_code: 6, stderr: Some("query-pg: guard"), sql: "SELECT private_note FROM shop.customer" },
    QuerySpec { name: "north-insert", when: "after_bundle", exit_code: 6, stderr: Some("query-pg: guard"), sql: "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (99, 'x', true)" },
    QuerySpec { name: "north-pos", when: "after_bundle", exit_code: 6, stderr: Some("query-pg: guard"), sql: "SELECT customer_id FROM shop.customer ORDER BY _oo_pos" },
    QuerySpec { name: "north-customer", when: "after_bundle", exit_code: 0, stderr: None, sql: "SELECT customer_id, display_name, active FROM shop.customer ORDER BY customer_id" },
    QuerySpec { name: "north-orders", when: "after_bundle", exit_code: 0, stderr: None, sql: "SELECT order_id, customer_id, memo, posted, amount, clerk_note FROM shop.order_doc ORDER BY order_id" },
    QuerySpec { name: "north-count", when: "after_bundle", exit_code: 0, stderr: None, sql: "SELECT count(*) FROM shop.order_doc" },
];

const NARROW_QUERIES: &[QuerySpec] = &[
    QuerySpec { name: "narrow-customer", when: "after_bundle", exit_code: 0, stderr: None, sql: "SELECT customer_id, display_name FROM shop.customer ORDER BY customer_id" },
    QuerySpec { name: "narrow-orders", when: "after_bundle", exit_code: 0, stderr: None, sql: "SELECT order_id, customer_id, posted FROM shop.order_doc ORDER BY order_id" },
];

fn push_query(out: &mut String, query: &QuerySpec) {
    out.push_str("{\"name\":");
    push_string(out, query.name);
    out.push_str(",\"when\":");
    push_string(out, query.when);
    out.push_str(",\"expected_exit\":");
    out.push_str(&query.exit_code.to_string());
    if let Some(stderr) = query.stderr {
        out.push_str(",\"expected_stderr\":");
        push_string(out, stderr);
    }
    out.push_str(",\"statement\":{\"source\":\"request\",\"sql\":");
    push_string(out, query.sql);
    out.push_str(",\"max_rows\":20,\"max_result_bytes\":8192,\"wall_clock_ms\":30000}}");
}

fn write_visibility(path: &Path, deployment: &Deployment, role: &str, north: bool) -> Result<(), String> {
    let mut out = String::from("{\"protocol\":\"oo.builder.role-visibility/v1\",\"project\":\"public_shop\",\"principal\":\"fixture_operator\",\"role\":");
    push_string(&mut out, role);
    out.push_str(",\"ontology_version\":");
    push_string(&mut out, &deployment.ont());
    out.push_str(",\"catalog_version\":");
    push_string(&mut out, &deployment.cat());
    out.push_str(",\"policy\":{\"id\":");
    push_string(&mut out, &deployment.pol());
    out.push_str(",\"version\":\"1\"},\"knowledge\":{\"id\":");
    push_string(&mut out, &deployment.know());
    out.push_str(",\"version\":\"1\"},\"issued_for_compiler\":\"oo-builder/role-pack/v1\",\"datasets\":[");
    if north {
        out.push_str("{\"dataset\":\"shop.customer\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":[\"customer_id\",\"display_name\",\"active\"]},{\"dataset\":\"shop.order_doc\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":[\"order_id\",\"customer_id\",\"memo\",\"posted\",\"amount\",\"clerk_note\"]}");
    } else {
        out.push_str("{\"dataset\":\"shop.customer\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":[\"customer_id\",\"display_name\"]},{\"dataset\":\"shop.order_doc\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":[\"order_id\",\"customer_id\",\"posted\"]}");
    }
    out.push_str("],\"limits\":{\"max_file_bytes\":262144,\"max_total_bytes\":1048576,\"max_entries\":128}}");
    fs::write(path, out).map_err(|_| "write visibility".to_owned())
}

fn write_operator_stage(paths: &Paths) -> Result<(), String> {
    let mut text = String::new();
    text.push_str("算子审阅阶段\n");
    text.push_str("下列决定来自 fixture/declared-review.json。审查者标记 public-fixture 是公开夹具声明，不是人类审批，不是认证，不是授权，不是发布。\n");
    text.push_str("自然语句、模型供应商和 harness 在本演示中不受支持，没有调用。\n");
    text.push_str("inspect 已完成，状态 awaiting_review。指纹只从 captured-draft.json 抄录。\n");
    text.push_str("pg-onboarding inspect --config ");
    text.push_str(&path_str(&paths.work.join("inspect.json"))?);
    text.push_str(" --run ");
    text.push_str(&path_str(&paths.run)?);
    text.push('\n');
    text.push_str("pg-onboarding apply-reviewed --config ");
    text.push_str(&path_str(&paths.work.join("apply.json"))?);
    text.push_str(" --run ");
    text.push_str(&path_str(&paths.run)?);
    text.push('\n');
    text.push_str("坏决定探针先于上述 apply，使用 bad-decisions.json，不得安装列 ACL。\n");
    fs::write(paths.work.join("operator-stage.txt"), text).map_err(|_| "operator stage".to_owned())
}

#[derive(Debug)]
struct Bound {
    reviews: String,
    decisions: String,
    bad_decisions: String,
    schema_hash: String,
}

fn bind_declared(captured: &Val, declared: &Val) -> Result<Bound, String> {
    expect_str(declared, "protocol", "oo.deploy.trusted-demo-declared-review/v1")?;
    expect_str(declared, "human_approval", "not_claimed")?;
    expect_str(declared, "authentication", "not_claimed")?;
    expect_str(declared, "natural_language", "unsupported")?;
    expect_str(declared, "model_provider", "unsupported")?;
    expect_str(declared, "harness", "not_performed")?;
    let reviewer = get(declared, "reviewer")?;
    expect_str(reviewer, "by", REVIEWER)?;
    expect_str(reviewer, "at", "2026-10-06")?;
    expect_string_set(get(declared, "accept_kinds")?, &["structural_column", "structural_primary_key", "candidate_relationship"])?;
    expect_string_set(get(declared, "skip_prefixes")?, &["pii/", "semantic/", "type/", "classification/"])?;
    let file_decisions = as_arr(get(declared, "decisions")?)?;
    if file_decisions.len() != DECISIONS.len() {
        return Err("declared decision count drifted".to_owned());
    }
    for (item, expected) in file_decisions.iter().zip(DECISIONS.iter()) {
        expect_str(item, "dataset", expected.0)?;
        expect_str(item, "column", expected.1)?;
        expect_str(item, "pii", expected.2)?;
        expect_str(item, "semantic_type", expected.3)?;
    }
    let bad = get(declared, "bad_probe")?;
    expect_str(bad, "dataset", "shop.customer")?;
    expect_str(bad, "column", "not_a_column")?;
    let schema_hash = str_field(get(captured, "wrapper")?, "schema_hash")?.to_owned();
    if !hex64(&schema_hash) {
        return Err("schema hash shape".to_owned());
    }
    let queue = as_arr(get(get(captured, "draft")?, "review_queue")?)?;
    let mut reviews = String::from("[");
    let mut review_count = 0_usize;
    for item in queue {
        let kind = str_field(item, "kind")?;
        let item_id = str_field(item, "item_id")?;
        let structural = matches!(kind, "structural_column" | "structural_primary_key" | "candidate_relationship");
        let skipped = item_id.starts_with("pii/") || item_id.starts_with("semantic/") || item_id.starts_with("type/") || item_id.starts_with("classification/");
        if !structural || skipped {
            continue;
        }
        if review_count > 0 {
            reviews.push(',');
        }
        let fingerprint = str_field(item, "fact_fingerprint")?;
        if !hex64(fingerprint) {
            return Err("fingerprint shape".to_owned());
        }
        reviews.push_str("{\"item_id\":");
        push_string(&mut reviews, item_id);
        reviews.push_str(",\"action\":\"accept\",\"schema_hash\":");
        push_string(&mut reviews, &schema_hash);
        reviews.push_str(",\"fact_fingerprint\":");
        push_string(&mut reviews, fingerprint);
        reviews.push('}');
        review_count += 1;
    }
    reviews.push(']');
    if review_count == 0 {
        return Err("no structural review item".to_owned());
    }
    let mut decisions = String::from("[");
    let mut first_fingerprint = String::new();
    for (index, (dataset, column, pii, semantic)) in DECISIONS.iter().enumerate() {
        let item_id = format!("column/{dataset}/{column}");
        let fingerprint = queue_fingerprint(queue, &item_id, "structural_column")?;
        if index == 0 {
            first_fingerprint = fingerprint.clone();
        }
        if index > 0 {
            decisions.push(',');
        }
        push_decision(&mut decisions, dataset, column, &schema_hash, &fingerprint, pii, semantic);
    }
    decisions.push(']');
    let mut bad_decisions = decisions.clone();
    bad_decisions.pop();
    bad_decisions.push(',');
    push_decision(&mut bad_decisions, "shop.customer", "not_a_column", &schema_hash, &first_fingerprint, "none", "identifier");
    bad_decisions.push(']');
    Ok(Bound { reviews, decisions, bad_decisions, schema_hash })
}

fn queue_fingerprint(queue: &[Val], item_id: &str, kind: &str) -> Result<String, String> {
    for item in queue {
        if str_field(item, "item_id")? == item_id && str_field(item, "kind")? == kind {
            let fingerprint = str_field(item, "fact_fingerprint")?.to_owned();
            if !hex64(&fingerprint) {
                return Err("fingerprint shape".to_owned());
            }
            return Ok(fingerprint);
        }
    }
    Err(format!("missing fingerprint {item_id}"))
}

fn push_decision(out: &mut String, dataset: &str, column: &str, schema_hash: &str, fingerprint: &str, pii: &str, semantic: &str) {
    out.push_str("{\"dataset\":");
    push_string(out, dataset);
    out.push_str(",\"column\":");
    push_string(out, column);
    out.push_str(",\"schema_hash\":");
    push_string(out, schema_hash);
    out.push_str(",\"fact_fingerprint\":");
    push_string(out, fingerprint);
    out.push_str(",\"pii\":");
    push_string(out, pii);
    out.push_str(",\"semantic_type\":");
    push_string(out, semantic);
    out.push_str(",\"reviewer\":{\"by\":");
    push_string(out, REVIEWER);
    out.push_str(",\"at\":\"2026-10-06\"}}");
}

fn check_bad_review(run: &Path, output: &std::process::Output) -> Result<(), String> {
    if output.status.success() {
        return Err("bad review was applied".to_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.contains("pg-onboarding: reviewed-effective-oos exit") {
        return Err(format!("bad review stderr was {stderr}"));
    }
    for name in [
        "install-column-acl-north.stdout",
        "install-column-acl-narrow.stdout",
        "provision-mapping-reader-north.stdout",
        "analyze-pg.stdout",
        "role-pack-north.stdout",
        "query-north-customer.stdout",
    ] {
        if run.join("steps").join(name).exists() {
            return Err(format!("bad review wrote {name}"));
        }
    }
    let body = parse_file(&run.join("apply-outcome.json"))?;
    expect_str(&body, "status", "failed")?;
    expect_str(&body, "step", "reviewed-effective-oos")?;
    expect_str(&body, "detail", "exit")?;
    expect_str(&body, "database_state", "changes_may_have_been_applied")?;
    let child = fs::read(run.join("steps/reviewed-effective-oos.stderr")).map_err(|_| "bad effective stderr missing".to_owned())?;
    if !child.windows(b"decision.unknown".len()).any(|window| window == b"decision.unknown") {
        return Err("bad review did not report decision.unknown".to_owned());
    }
    Ok(())
}

fn check_ready(run: &Path) -> Result<(), String> {
    let applied = parse_file(&run.join("apply-outcome.json"))?;
    expect_str(&applied, "status", "ready_for_trusted_query")?;
    expect_str(&applied, "protocol", "oo.onboarding.outcome/v1")?;
    expect_num(&applied, "mapping_run_generation", "48")?;
    expect_bool(&applied, "analyze_published", false)?;
    expect_bool(&applied, "estimate_is_exact", false)?;
    expect_bool(&applied, "database_rollback", false)?;
    expect_bool(&applied, "database_release", false)?;
    expect_str(&applied, "database_state", "child_compilers_applied_no_release")?;
    let claims = get(&applied, "non_claims")?;
    expect_bool(claims, "parent_acceptance", false)?;
    expect_bool(claims, "public_auth", false)?;
    expect_str(claims, "authentication", "not_claimed")?;
    expect_str(claims, "authorization", "not_claimed")?;
    expect_str(claims, "natural_language", "missing")?;
    expect_str(claims, "harness_mount", "not_performed")?;
    expect_str(claims, "semantic_publication", "missing")?;
    expect_str(claims, "m1_i01", "not_accepted")?;
    expect_str(claims, "m1_i02", "not_accepted")?;
    let hidden = as_arr(get(&applied, "hidden")?)?;
    let mut found = false;
    for item in hidden {
        if str_field(item, "dataset")? == "shop.customer" && str_field(item, "column")? == "private_note" {
            expect_str(item, "reason", "classification.absent")?;
            found = true;
        }
    }
    if !found {
        return Err("private_note was not hidden".to_owned());
    }
    Ok(())
}

fn check_queries(run: &Path) -> Result<(), String> {
    expect_rows(&run.join("steps/query-north-customer.stdout"), &[
        &[Cell::Int("41"), Cell::Text("丙号"), Cell::Bool(true)],
        &[Cell::Int("42"), Cell::Null, Cell::Bool(false)],
        &[Cell::Int("43"), Cell::Text("丁铺"), Cell::Bool(true)],
    ])?;
    expect_rows(&run.join("steps/query-north-orders.stdout"), &[
        &[Cell::Int("701"), Cell::Int("41"), Cell::Text("首单"), Cell::Bool(true), Cell::Int("-9223372036854775808"), Cell::Text("柜员甲")],
        &[Cell::Int("702"), Cell::Int("41"), Cell::Null, Cell::Bool(false), Cell::Int("9223372036854775807"), Cell::Null],
        &[Cell::Int("703"), Cell::Int("43"), Cell::Text("零单"), Cell::Bool(true), Cell::Int("0"), Cell::Text("柜员乙")],
    ])?;
    expect_rows(&run.join("steps/query-north-count.stdout"), &[&[Cell::Int("3")]])?;
    expect_rows(&run.join("steps/query-narrow-customer.stdout"), &[
        &[Cell::Int("41"), Cell::Text("丙号")],
        &[Cell::Int("42"), Cell::Null],
        &[Cell::Int("43"), Cell::Text("丁铺")],
    ])?;
    expect_rows(&run.join("steps/query-narrow-orders.stdout"), &[
        &[Cell::Int("701"), Cell::Int("41"), Cell::Bool(true)],
        &[Cell::Int("702"), Cell::Int("41"), Cell::Bool(false)],
        &[Cell::Int("703"), Cell::Int("43"), Cell::Bool(true)],
    ])?;
    for name in ["north-before", "north-hidden", "north-insert", "north-pos"] {
        let bytes = fs::read(run.join("steps").join(format!("query-{name}.stdout"))).map_err(|_| format!("missing {name}"))?;
        if !bytes.is_empty() {
            return Err(format!("{name} wrote a result"));
        }
    }
    let orders = fs::read(run.join("steps/query-north-orders.stdout")).map_err(|_| "orders".to_owned())?;
    if !contains_text(&orders, "柜员甲") {
        return Err("north orders lost the source clerk note".to_owned());
    }
    for name in ["narrow-customer", "narrow-orders", "north-customer", "north-count"] {
        let bytes = fs::read(run.join("steps").join(format!("query-{name}.stdout"))).map_err(|_| format!("missing {name}"))?;
        if contains_text(&bytes, "柜员甲") || contains_text(&bytes, "内部甲") {
            return Err(format!("{name} leaked a hidden value"));
        }
    }
    Ok(())
}

fn write_receipt(paths: &Paths, deployment: &Deployment, identity: &str, schema_hash: &str) -> Result<(), String> {
    let north = sha256_file(&paths.run.join("steps/role-pack-north.stdout"))?;
    let narrow = sha256_file(&paths.run.join("steps/role-pack-narrow.stdout"))?;
    let request = path_str(&paths.run.join("queries/north-customer.request.json"))?;
    let query_bin = path_str(&paths.bins.join("query-pg"))?;
    let mut out = String::from("{\"protocol\":\"oo.deploy.trusted-demo-receipt/v1\",\"status\":\"ready_for_trusted_query\",\"fixed_commit\":");
    push_string(&mut out, FIXED);
    out.push_str(",\"project\":");
    push_string(&mut out, "open-ontology");
    out.push_str(",\"owner\":");
    push_string(&mut out, &deployment.owner);
    out.push_str(",\"run_id\":");
    push_string(&mut out, &deployment.run_id);
    out.push_str(",\"source_id\":");
    push_string(&mut out, SOURCE_ID);
    out.push_str(",\"human_approval\":\"not_claimed\",\"authentication\":\"not_claimed\",\"authorization\":\"not_claimed\",\"semantic_publication\":\"missing\",\"natural_language\":\"unsupported\",\"model_provider\":\"unsupported\",\"harness\":\"not_performed\",\"m1_i01\":\"not_accepted\",\"m1_i02\":\"not_accepted\",\"parent_acceptance\":false,\"config_identity_sha256\":");
    push_string(&mut out, identity);
    out.push_str(",\"schema_hash\":");
    push_string(&mut out, schema_hash);
    out.push_str(",\"role_pack_north_sha256\":");
    push_string(&mut out, &north);
    out.push_str(",\"role_pack_narrow_sha256\":");
    push_string(&mut out, &narrow);
    out.push_str(",\"locations\":{\"run\":");
    push_string(&mut out, &path_str(&paths.run)?);
    out.push_str(",\"inspect_outcome\":");
    push_string(&mut out, &path_str(&paths.run.join("outcome.json"))?);
    out.push_str(",\"apply_outcome\":");
    push_string(&mut out, &path_str(&paths.run.join("apply-outcome.json"))?);
    out.push_str(",\"operator_stage\":");
    push_string(&mut out, &path_str(&paths.work.join("operator-stage.txt"))?);
    out.push_str(",\"role_pack_north\":");
    push_string(&mut out, &path_str(&paths.run.join("steps/role-pack-north.stdout"))?);
    out.push_str(",\"catalog\":");
    push_string(&mut out, &path_str(&paths.run.join("catalog.json"))?);
    out.push_str(",\"captured_draft\":");
    push_string(&mut out, &path_str(&paths.run.join("captured-draft.json"))?);
    out.push_str("},\"trusted_query\":{\"argv\":[");
    push_string(&mut out, &query_bin);
    out.push_str(",\"--request\",");
    push_string(&mut out, &request);
    out.push_str("],\"sql\":");
    push_string(&mut out, NORTH_QUERIES[4].sql);
    out.push_str(",\"role\":\"oo_r_north\",\"repeat\":\"start.sh --manifest <deployment> --smoke\"}}");
    let receipt = paths.evidence.join("live/receipt.json");
    if let Some(parent) = receipt.parent() {
        fs::create_dir_all(parent).map_err(|_| "receipt dir".to_owned())?;
    }
    fs::write(receipt, out).map_err(|_| "write receipt".to_owned())
}

fn identity_preimage(deployment: &Deployment) -> String {
    let mut body = String::from("oo-pg-config-identity/v1\n");
    body.push_str("source_id=");
    body.push_str(SOURCE_ID);
    body.push('\n');
    body.push_str("host=");
    body.push_str(&deployment.host());
    body.push_str("\nport=5432\ndatabase=");
    body.push_str(&deployment.database());
    body.push_str("\nusername=");
    body.push_str(&deployment.reader());
    body.push_str("\nsecret_ref=");
    body.push_str(&deployment.secret_ref());
    body.push_str("\nconnect_ms=30000\nstatement_ms=60000\nlock_ms=30000\nmax_rows=10000\nmax_bytes=8388608\n");
    body.push_str("table\tshop\tcustomer\n");
    body.push_str("table\tshop\torder_doc\n");
    body
}

fn run_bin(bin: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = Command::new(bin);
    command.args(args).env_clear().env("HOME", "/tmp").env("LANG", "C.UTF-8").env(
        "PATH",
        "/cargo-home/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    command.output().map_err(|_| format!("spawn {}", bin.display()))
}

fn status_code(status: &std::process::ExitStatus) -> String {
    status.code().map(|code| code.to_string()).unwrap_or_else(|| "signal".to_owned())
}

fn path_str(path: &Path) -> Result<String, String> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| "path utf-8".to_owned())
}

fn refuse_marker(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|_| format!("read {}", path.display()))?;
    if contains_text(&bytes, MARKER) {
        return Err(format!("{} contains the comment marker", path.display()));
    }
    Ok(())
}

fn contains_text(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle.as_bytes())
}

fn load_secrets(dir: &Path) -> Result<Vec<Vec<u8>>, String> {
    let mut secrets = Vec::new();
    for name in ["admin.pass", "reader.pass", "writer.pass", "runtime.pass", "login.pass"] {
        let mut bytes = fs::read(dir.join(name)).map_err(|_| format!("secret file {name}"))?;
        if bytes.ends_with(b"\n") {
            bytes.pop();
            if bytes.ends_with(b"\r") {
                bytes.pop();
            }
        }
        if bytes.is_empty() {
            return Err(format!("empty secret {name}"));
        }
        secrets.push(bytes);
    }
    Ok(secrets)
}

fn scan_tree(root: &Path, secrets: &[Vec<u8>], report: &Path) -> Result<(), String> {
    let mut hits = Vec::new();
    let mut files = 0_u64;
    walk_scan(root, report, secrets, &mut hits, &mut files)?;
    let mut text = String::new();
    if hits.is_empty() {
        text.push_str("secret_absent files=");
        text.push_str(&files.to_string());
        text.push('\n');
    } else {
        text.push_str("secret_present\n");
        for hit in &hits {
            text.push_str(hit);
            text.push('\n');
        }
    }
    if let Some(parent) = report.parent() {
        fs::create_dir_all(parent).map_err(|_| "scan dir".to_owned())?;
    }
    fs::write(report, &text).map_err(|_| "scan report".to_owned())?;
    for secret in secrets {
        let written = fs::read(report).map_err(|_| "reread scan".to_owned())?;
        if written.windows(secret.len()).any(|window| window == secret.as_slice()) {
            return Err("scan report contains a secret".to_owned());
        }
    }
    if !hits.is_empty() {
        return Err("fixture secret appeared in output".to_owned());
    }
    Ok(())
}

fn walk_scan(dir: &Path, report: &Path, secrets: &[Vec<u8>], hits: &mut Vec<String>, files: &mut u64) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|_| format!("scan {}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|_| "scan entry".to_owned())?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "scan meta".to_owned())?;
        if meta.file_type().is_symlink() {
            return Err(format!("scan symlink {}", path.display()));
        }
        if meta.is_dir() {
            walk_scan(&path, report, secrets, hits, files)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if path == report {
            continue;
        }
        *files += 1;
        if file_contains_secret(&path, secrets)? {
            hits.push(format!("path={}", path.display()));
        }
    }
    Ok(())
}

fn file_contains_secret(path: &Path, secrets: &[Vec<u8>]) -> Result<bool, String> {
    let mut file = File::open(path).map_err(|_| format!("scan open {}", path.display()))?;
    let mut overlap = [0_u8; 128];
    let mut carry = 0_usize;
    let mut buf = [0_u8; 8192];
    loop {
        let read = file.read(&mut buf).map_err(|_| "scan read".to_owned())?;
        if read == 0 {
            return Ok(false);
        }
        let mut window = Vec::with_capacity(carry + read);
        window.extend_from_slice(&overlap[..carry]);
        window.extend_from_slice(&buf[..read]);
        for secret in secrets {
            if !secret.is_empty() && window.windows(secret.len()).any(|item| item == secret.as_slice()) {
                return Ok(true);
            }
        }
        let keep = overlap.len().min(window.len());
        overlap[..keep].copy_from_slice(&window[window.len() - keep..]);
        carry = keep;
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Err("bad run already exists".to_owned());
    }
    fs::create_dir(to).map_err(|_| "bad run dir".to_owned())?;
    copy_children(from, to)
}

fn copy_children(from: &Path, to: &Path) -> Result<(), String> {
    let entries = fs::read_dir(from).map_err(|_| "copy dir".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "copy entry".to_owned())?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "copy meta".to_owned())?;
        if meta.file_type().is_symlink() {
            return Err("copy symlink".to_owned());
        }
        let dest = to.join(entry.file_name());
        if meta.is_dir() {
            fs::create_dir(&dest).map_err(|_| "copy mkdir".to_owned())?;
            copy_children(&path, &dest)?;
        } else if meta.is_file() {
            fs::copy(&path, &dest).map_err(|_| "copy file".to_owned())?;
        }
    }
    Ok(())
}

fn same_rows(left: &Path, right: &Path) -> Result<(), String> {
    let left_rows = row_lines(left)?;
    let right_rows = row_lines(right)?;
    if left_rows != right_rows {
        return Err("repeated query rows differ".to_owned());
    }
    if left_rows.is_empty() {
        return Err("repeated query has no rows".to_owned());
    }
    Ok(())
}

fn row_lines(path: &Path) -> Result<Vec<String>, String> {
    let bytes = fs::read(path).map_err(|_| format!("rows {}", path.display()))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "rows utf-8".to_owned())?;
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value = parse_slice(line.as_bytes())?;
        if str_field(&value, "record")? == "row" && num_str(get(&value, "turn")?)? == "0" {
            rows.push(line.to_owned());
        }
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
enum Cell {
    Null,
    Bool(bool),
    Int(&'static str),
    Text(&'static str),
}

fn expect_rows(path: &Path, expect: &[&[Cell]]) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|_| format!("query {}", path.display()))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "query utf-8".to_owned())?;
    let mut rows = Vec::new();
    let mut summary_rows = None;
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value = parse_slice(line.as_bytes())?;
        let record = str_field(&value, "record")?;
        if num_str(get(&value, "turn")?)? != "0" {
            continue;
        }
        if record == "row" {
            rows.push(value);
        } else if record == "summary" {
            expect_str(&value, "status", "succeeded")?;
            expect_bool(&value, "truncated", false)?;
            summary_rows = Some(num_str(get(&value, "returned_rows")?)?.to_owned());
        }
    }
    if rows.len() != expect.len() {
        return Err(format!("{} row count", path.display()));
    }
    if summary_rows.as_deref() != Some(&expect.len().to_string()) {
        return Err(format!("{} summary", path.display()));
    }
    for (got, wanted) in rows.iter().zip(expect.iter()) {
        let cells = as_arr(get(got, "cells")?)?;
        if cells.len() != wanted.len() {
            return Err(format!("{} width", path.display()));
        }
        for (cell, expect_cell) in cells.iter().zip(wanted.iter()) {
            if !cell_matches(cell, *expect_cell)? {
                return Err(format!("{} cell", path.display()));
            }
        }
    }
    Ok(())
}

fn cell_matches(got: &Val, expect: Cell) -> Result<bool, String> {
    let kind = str_field(got, "kind")?;
    Ok(match expect {
        Cell::Null => kind == "null",
        Cell::Bool(value) => kind == "bool" && bool_field(got, "value")? == value,
        Cell::Int(value) => kind == "int" && str_field(got, "value")? == value,
        Cell::Text(value) => kind == "text" && str_field(got, "value")? == value,
    })
}

fn parse_file(path: &Path) -> Result<Val, String> {
    let bytes = fs::read(path).map_err(|_| format!("read {}", path.display()))?;
    if bytes.len() > 8_388_608 {
        return Err(format!("budget {}", path.display()));
    }
    parse_slice(&bytes)
}

fn parse_slice(bytes: &[u8]) -> Result<Val, String> {
    let mut parser = Parser { bytes, index: 0, depth: 0 };
    parser.skip();
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err("trailing json".to_owned());
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    depth: usize,
}

impl Parser<'_> {
    fn skip(&mut self) {
        while matches!(self.bytes.get(self.index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn value(&mut self) -> Result<Val, String> {
        if self.depth > 64 {
            return Err("json depth".to_owned());
        }
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'n') => self.literal(b"null", Val::Null),
            Some(b't') => self.literal(b"true", Val::Bool(true)),
            Some(b'f') => self.literal(b"false", Val::Bool(false)),
            Some(b'"') => Ok(Val::Str(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'8' | b'9') => self.number(),
            _ => Err("json value".to_owned()),
        }
    }

    fn literal(&mut self, text: &[u8], value: Val) -> Result<Val, String> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err("json literal".to_owned())
        }
    }

    fn array(&mut self) -> Result<Val, String> {
        self.index += 1;
        self.depth += 1;
        let mut items = Vec::new();
        self.skip();
        if self.bytes.get(self.index) == Some(&b']') {
            self.index += 1;
            self.depth -= 1;
            return Ok(Val::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip();
            match self.bytes.get(self.index) {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    self.depth -= 1;
                    return Ok(Val::Arr(items));
                }
                _ => return Err("json array".to_owned()),
            }
        }
    }

    fn object(&mut self) -> Result<Val, String> {
        self.index += 1;
        self.depth += 1;
        let mut items = Vec::new();
        self.skip();
        if self.bytes.get(self.index) == Some(&b'}') {
            self.index += 1;
            self.depth -= 1;
            return Ok(Val::Obj(items));
        }
        loop {
            self.skip();
            if self.bytes.get(self.index) != Some(&b'"') {
                return Err("json key".to_owned());
            }
            let key = self.string()?;
            if items.iter().any(|(existing, _)| existing == &key) {
                return Err("duplicate json key".to_owned());
            }
            self.skip();
            if self.bytes.get(self.index) != Some(&b':') {
                return Err("json colon".to_owned());
            }
            self.index += 1;
            let value = self.value()?;
            items.push((key, value));
            self.skip();
            match self.bytes.get(self.index) {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    self.depth -= 1;
                    return Ok(Val::Obj(items));
                }
                _ => return Err("json object".to_owned()),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.index += 1;
        let mut out = Vec::new();
        loop {
            let byte = *self.bytes.get(self.index).ok_or_else(|| "json string".to_owned())?;
            self.index += 1;
            match byte {
                b'"' => return String::from_utf8(out).map_err(|_| "json utf-8".to_owned()),
                b'\\' => {
                    let escaped = *self.bytes.get(self.index).ok_or_else(|| "json escape".to_owned())?;
                    self.index += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = self.bytes.get(self.index..self.index + 4).ok_or_else(|| "json unicode".to_owned())?;
                            self.index += 4;
                            let text = std::str::from_utf8(hex).map_err(|_| "json unicode".to_owned())?;
                            let code = u32::from_str_radix(text, 16).map_err(|_| "json unicode".to_owned())?;
                            let ch = char::from_u32(code).ok_or_else(|| "json unicode".to_owned())?;
                            let mut buf = [0_u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err("json escape".to_owned()),
                    }
                }
                byte if byte < 0x20 => return Err("json control".to_owned()),
                byte => out.push(byte),
            }
        }
    }

    fn number(&mut self) -> Result<Val, String> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let digits = self.take_digits()?;
        if digits == 0 {
            return Err("json number".to_owned());
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            if self.take_digits()? == 0 {
                return Err("json number".to_owned());
            }
        }
        if matches!(self.bytes.get(self.index), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            if self.take_digits()? == 0 {
                return Err("json number".to_owned());
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "json number".to_owned())?;
        Ok(Val::Num(text.to_owned()))
    }

    fn take_digits(&mut self) -> Result<usize, String> {
        let start = self.index;
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        Ok(self.index - start)
    }
}

#[derive(Clone, Debug)]
enum Val {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

fn get<'a>(value: &'a Val, key: &str) -> Result<&'a Val, String> {
    match value {
        Val::Obj(items) => items.iter().find(|(name, _)| name == key).map(|(_, item)| item).ok_or_else(|| format!("missing {key}")),
        _ => Err(format!("object {key}")),
    }
}

fn set_str(value: &mut Val, key: &str, text: &str) -> Result<(), String> {
    match value {
        Val::Obj(items) => {
            for (name, item) in items.iter_mut() {
                if name == key {
                    *item = Val::Str(text.to_owned());
                    return Ok(());
                }
            }
            Err(format!("missing {key}"))
        }
        _ => Err("object".to_owned()),
    }
}

fn as_arr(value: &Val) -> Result<&[Val], String> {
    match value {
        Val::Arr(items) => Ok(items),
        _ => Err("array".to_owned()),
    }
}

fn str_field<'a>(value: &'a Val, key: &str) -> Result<&'a str, String> {
    match get(value, key)? {
        Val::Str(text) => Ok(text),
        _ => Err(format!("string {key}")),
    }
}

fn expect_str(value: &Val, key: &str, expect: &str) -> Result<(), String> {
    let got = str_field(value, key)?;
    if got == expect {
        Ok(())
    } else {
        Err(format!("{key} was {got}"))
    }
}

fn num_str(value: &Val) -> Result<&str, String> {
    match value {
        Val::Num(text) => Ok(text),
        _ => Err("number".to_owned()),
    }
}

fn expect_num(value: &Val, key: &str, expect: &str) -> Result<(), String> {
    let got = num_str(get(value, key)?)?;
    if got == expect {
        Ok(())
    } else {
        Err(format!("{key} was {got}"))
    }
}

fn expect_bool(value: &Val, key: &str, expect: bool) -> Result<(), String> {
    if bool_field(value, key)? == expect {
        Ok(())
    } else {
        Err(format!("bool {key}"))
    }
}

fn bool_field(value: &Val, key: &str) -> Result<bool, String> {
    match get(value, key)? {
        Val::Bool(item) => Ok(*item),
        _ => Err(format!("bool {key}")),
    }
}

fn expect_string_set(value: &Val, expect: &[&str]) -> Result<(), String> {
    let items = as_arr(value)?;
    if items.len() != expect.len() {
        return Err("string set length".to_owned());
    }
    for (item, expected) in items.iter().zip(expect.iter()) {
        match item {
            Val::Str(text) if text == expected => {}
            _ => return Err("string set drift".to_owned()),
        }
    }
    Ok(())
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u");
                out.push_str(&format!("{code:04x}", code = c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn val_to_json(value: &Val) -> String {
    let mut out = String::new();
    write_val(&mut out, value);
    out
}

fn write_val(out: &mut String, value: &Val) {
    match value {
        Val::Null => out.push_str("null"),
        Val::Bool(true) => out.push_str("true"),
        Val::Bool(false) => out.push_str("false"),
        Val::Num(text) => out.push_str(text),
        Val::Str(text) => push_string(out, text),
        Val::Arr(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_val(out, item);
            }
            out.push(']');
        }
        Val::Obj(items) => {
            out.push('{');
            for (index, (key, item)) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                write_val(out, item);
            }
            out.push('}');
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|_| format!("hash {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 8192];
    loop {
        let read = file.read(&mut buf).map_err(|_| "hash read".to_owned())?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.hex())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.hex()
}

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    filled: usize,
    bits: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            buffer: [0; 64],
            filled: 0,
            bits: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.bits = self.bits.wrapping_add((bytes.len() as u64).wrapping_mul(8));
        self.absorb(bytes);
    }

    fn absorb(&mut self, mut bytes: &[u8]) {
        if self.filled > 0 {
            let need = 64 - self.filled;
            let take = need.min(bytes.len());
            self.buffer[self.filled..self.filled + take].copy_from_slice(&bytes[..take]);
            self.filled += take;
            bytes = &bytes[take..];
            if self.filled == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.filled = 0;
            }
        }
        while bytes.len() >= 64 {
            let mut block = [0_u8; 64];
            block.copy_from_slice(&bytes[..64]);
            self.compress(&block);
            bytes = &bytes[64..];
        }
        if !bytes.is_empty() {
            self.buffer[..bytes.len()].copy_from_slice(bytes);
            self.filled = bytes.len();
        }
    }

    fn hex(mut self) -> String {
        let bits = self.bits;
        let zeros = [0_u8; 64];
        // Padding is not message bytes. Counting it would change the length field.
        self.absorb(&[0x80]);
        if self.filled > 56 {
            let pad = 64 - self.filled;
            self.absorb(&zeros[..pad]);
        }
        let rest = 56 - self.filled;
        self.absorb(&zeros[..rest]);
        self.absorb(&bits.to_be_bytes());
        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];
        let mut words = [0_u32; 64];
        for (index, word) in words.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *word = u32::from_be_bytes([block[start], block[start + 1], block[start + 2], block[start + 3]]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(choose).wrapping_add(K[index]).wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_abc_and_empty() {
        assert_eq!(sha256_bytes(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_bytes(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    fn historical_manifest() -> &'static str {
        "{\"protocol\":\"oo.deploy.trusted-demo-deployment/v1\",\"project\":\"open-ontology\",\"fixed_source\":\"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480\",\"owner\":\"a305\",\"run_id\":\"eef831aa-dba4-4af2-a78b-541bc9e0a305\",\"source_id\":\"public_shop\"}"
    }

    fn fresh_manifest() -> &'static str {
        "{\"protocol\":\"oo.deploy.trusted-demo-deployment/v1\",\"project\":\"open-ontology\",\"fixed_source\":\"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480\",\"owner\":\"a312\",\"run_id\":\"eef831aa-dba4-4af2-a78b-541bc9e0a312\",\"source_id\":\"public_shop\"}"
    }

    #[test]
    fn identity_preimage_has_no_secret_material() {
        let deployment = load_deployment(historical_manifest().as_bytes()).unwrap();
        let text = identity_preimage(&deployment);
        assert!(text.starts_with("oo-pg-config-identity/v1\n"));
        assert!(text.contains("source_id=public_shop\n"));
        assert!(text.contains("host=oo-a305-pg\n"));
        assert!(text.contains("database=oo_a305_src\n"));
        assert!(text.contains("username=oo_a305_reader\n"));
        assert!(text.contains("secret_ref=secret://pg/a305/reader\n"));
        assert!(text.contains("table\tshop\torder_doc\n"));
        assert!(!text.contains("password"));
        assert!(!text.contains("BEGIN"));
    }

    #[test]
    fn fresh_manifest_does_not_reuse_historical_names() {
        let historical = load_deployment(historical_manifest().as_bytes()).unwrap();
        let fresh = load_deployment(fresh_manifest().as_bytes()).unwrap();
        assert_eq!(fresh.host(), "oo-a312-pg");
        assert_eq!(fresh.database(), "oo_a312_src");
        assert_eq!(fresh.dst(), "oo_a312_dst");
        assert_eq!(fresh.reader(), "oo_a312_reader");
        assert_eq!(fresh.runtime(), "oo_a312_runtime");
        assert_eq!(fresh.login(), "oo_a312_login");
        assert_eq!(fresh.secret_ref(), "secret://pg/a312/reader");
        assert_eq!(fresh.shard(), "a312main");
        assert_eq!(fresh.ont(), "ont-a312");
        assert_eq!(fresh.pol(), "pol-a312");
        assert_ne!(fresh.host(), historical.host());
        assert!(!identity_preimage(&fresh).contains("a305"));
        assert!(settings_json(&historical).contains("\"ontology_version\":\"ont-a305\""));
        assert!(settings_json(&fresh).contains("\"id\":\"pol-a312\""));
    }

    #[test]
    fn manifest_rejects_foreign_owner_and_injection() {
        let swapped = historical_manifest().replace(
            "\"owner\":\"a305\",\"run_id\":\"eef831aa-dba4-4af2-a78b-541bc9e0a305\"",
            "\"owner\":\"a305\",\"run_id\":\"eef831aa-dba4-4af2-a78b-541bc9e0a312\"",
        );
        assert!(load_deployment(swapped.as_bytes()).is_err());
        let upper = historical_manifest().replace("a305", "A305");
        assert!(load_deployment(upper.as_bytes()).is_err());
        let extra = historical_manifest().replace('}', ",\"password_file\":\"/tmp/x\"}");
        assert!(load_deployment(extra.as_bytes()).is_err());
        let spaced = historical_manifest().replace(':', ": ");
        assert!(load_deployment(spaced.as_bytes()).is_err());
        assert!(load_deployment(b"not-json").is_err());
    }

    #[test]
    fn duplicate_key_and_unicode_escape() {
        let err = parse_slice(br#"{"a":1,"a":2}"#).unwrap_err();
        assert_eq!(err, "duplicate json key");
        let value = parse_slice(br#"[0, true, null, "\u4e19"]"#).unwrap();
        let items = as_arr(&value).unwrap();
        assert!(matches!(&items[3], Val::Str(text) if text == "丙"));
    }

    #[test]
    fn declared_binding_uses_captured_fingerprint_only() {
        let hash = "ab".repeat(32);
        let captured = format!(
            "{{\"wrapper\":{{\"schema_hash\":\"{hash}\"}},\"draft\":{{\"review_queue\":[{{\"item_id\":\"column/shop.customer/customer_id\",\"kind\":\"structural_column\",\"fact_fingerprint\":\"{hash}\"}},{{\"item_id\":\"pii/shop.customer/customer_id\",\"kind\":\"pii_unknown\",\"fact_fingerprint\":\"{hash}\"}}]}}}}"
        );
        let declared = fs::read("deploy/trusted-demo/fixture/declared-review.json").or_else(|_| fs::read("/deploy/fixture/declared-review.json")).expect("declared fixture");
        let bound = bind_declared(&parse_slice(captured.as_bytes()).unwrap(), &parse_slice(&declared).unwrap());
        let err = bound.unwrap_err();
        assert!(err.contains("missing fingerprint"), "{err}");
    }

    #[test]
    fn secret_report_omits_secret_bytes() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("demo-scan-{pid}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("out")).unwrap();
        let secret = b"0123456789abcdef0123456789abcdef01234567".to_vec();
        fs::write(root.join("out/hit.txt"), &secret).unwrap();
        fs::write(root.join("out/clean.txt"), b"public_shop\n").unwrap();
        let report = root.join("report.txt");
        let result = scan_tree(&root, &[secret.clone()], &report);
        assert!(result.is_err());
        let text = fs::read(&report).unwrap();
        assert!(!text.windows(secret.len()).any(|window| window == secret.as_slice()));
        assert!(text.windows(b"secret_present".len()).any(|window| window == b"secret_present"));
        let _ = fs::remove_dir_all(&root);
    }
}
