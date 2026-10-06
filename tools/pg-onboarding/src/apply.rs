use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::Stop;
use crate::outcome::{self, failed_body};
use crate::sha256::sha256_hex;
use crate::spawn::{
    self, hex64, load_password, parse_json_slice, physical_ok, pin_binary, plain_token, run_child, text_field,
    BinPin, ChildRequest,
};

const APPLY_PROTOCOL: &str = "oo.onboarding.apply-reviewed/v1";
const POLICY_PREFIX: &[u8] = b"{\"protocol\":\"oo.gateway.reviewed-policy-cli/v1\",\"policies\":";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyConfig {
    protocol: String,
    caller: String,
    binaries: ApplyBins,
    reviews_file: String,
    decisions_file: String,
    settings: Value,
    schema: String,
    query_login: LoginCfg,
    roles: Vec<RoleCfg>,
    analyze: AnalyzeCfg,
    #[serde(default)]
    deadlines_ms: ApplyDeadlines,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyBins {
    review_pg_draft: BinPin,
    reviewed_effective_oos: BinPin,
    reviewed_policy: BinPin,
    install_column_acl: BinPin,
    provision_mapping_reader: BinPin,
    analyze_pg: BinPin,
    role_pack: BinPin,
    role_pack_bundle: BinPin,
    query_pg: BinPin,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginCfg {
    user: String,
    password_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleCfg {
    slug: String,
    allow: Vec<AllowCfg>,
    visibility_file: String,
    #[serde(default)]
    queries: Vec<QueryCfg>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowCfg {
    dataset: String,
    column: String,
    explicit_allow: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryCfg {
    name: String,
    when: String,
    expected_exit: i32,
    #[serde(default)]
    expected_stderr: Option<String>,
    statement: StatementCfg,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatementCfg {
    source: String,
    #[serde(default)]
    sql: Option<String>,
    #[serde(default)]
    sql_file: Option<String>,
    max_rows: u32,
    max_result_bytes: u64,
    wall_clock_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeCfg {
    statement_timeout_ms: u64,
    lock_timeout_ms: u64,
    connect_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ApplyDeadlines {
    #[serde(default)]
    review_ms: Option<u64>,
    #[serde(default)]
    effective_ms: Option<u64>,
    #[serde(default)]
    policy_ms: Option<u64>,
    #[serde(default)]
    install_ms: Option<u64>,
    #[serde(default)]
    provision_ms: Option<u64>,
    #[serde(default)]
    analyze_ms: Option<u64>,
    #[serde(default)]
    role_pack_ms: Option<u64>,
    #[serde(default)]
    bundle_ms: Option<u64>,
    #[serde(default)]
    query_ms: Option<u64>,
}

struct Prepared<'a> {
    cfg: &'a ApplyConfig,
    manifest: Value,
    catalog: Vec<u8>,
    import_bytes: Vec<u8>,
    captured: Vec<u8>,
    secrets: Vec<Vec<u8>>,
    bins: Bins,
    child_started: Cell<bool>,
}

struct Bins {
    review: PathBuf,
    effective: PathBuf,
    policy: PathBuf,
    install: PathBuf,
    provision: PathBuf,
    analyze: PathBuf,
    role_pack: PathBuf,
    bundle: PathBuf,
    query: PathBuf,
}

pub fn apply_reviewed(config_path: &Path, run_dir: &Path) -> Result<(), Stop> {
    let cfg_bytes = read_config_bytes(config_path)?;
    let cfg: ApplyConfig = decode_config(&cfg_bytes)?;
    validate_apply(&cfg)?;
    if !run_dir.is_absolute() {
        return Err(Stop::new("run", "config"));
    }
    let manifest_bytes = read_manifest_bytes(&run_dir.join("inspect-manifest.json"))?;
    let sidecar = fs::read(run_dir.join("inspect-manifest.json.sha256")).map_err(|_| Stop::new("manifest", "manifest"))?;
    let sidecar_hex = String::from_utf8_lossy(&sidecar).trim().to_string();
    if sidecar_hex != sha256_hex(&manifest_bytes) {
        return Err(Stop::new("manifest", "manifest"));
    }
    let manifest = crate::json_doc::parse(&manifest_bytes, crate::json_doc::OPERATOR_JSON_MAX)
        .map_err(|fault| Stop::new("manifest", fault.detail()))?;
    if manifest["protocol"] != "oo.onboarding.inspect-manifest/v1" || manifest["phase"] != "awaiting_review" {
        return Err(Stop::new("manifest", "protocol"));
    }
    let outcome = outcome::read_json(
        &run_dir.join("outcome.json"),
        crate::json_doc::OPERATOR_JSON_MAX,
        "outcome",
    )?;
    if outcome::status_of(&outcome)? != "awaiting_review" {
        return Err(Stop::new("outcome", "terminal"));
    }
    if run_dir.join("apply-outcome.json").exists() {
        return Err(Stop::new("outcome", "terminal"));
    }
    refuse_existing_apply_steps(run_dir)?;
    let secrets = apply_secrets(&cfg, &manifest)?;
    if spawn::contains_secret(&cfg_bytes, &secrets) || spawn::contains_secret(&manifest_bytes, &secrets) {
        return Err(Stop::new("config", "secret"));
    }
    let catalog = read_hashed(run_dir, "catalog.json", &manifest, "catalog_sha256")?;
    let import_bytes = read_hashed(run_dir, "import-result.json", &manifest, "import_result_sha256")?;
    let captured = read_hashed(run_dir, "captured-draft.json", &manifest, "captured_draft_sha256")?;
    let bins = pin_apply(&cfg)?;
    let prepared = Prepared {
        cfg: &cfg,
        catalog,
        import_bytes,
        captured,
        secrets,
        bins,
        manifest,
        child_started: Cell::new(false),
    };
    let result = apply_in(run_dir, &prepared);
    if let Err(stop) = &result {
        let _ = outcome::write_json_new(
            &run_dir.join("apply-outcome.json"),
            &failed_body(stop, "apply-reviewed", prepared.child_started.get()),
        );
    }
    result
}

fn read_config_bytes(path: &Path) -> Result<Vec<u8>, Stop> {
    let max = crate::json_doc::OPERATOR_JSON_MAX;
    let meta = fs::symlink_metadata(path).map_err(|_| Stop::new("config", "config"))?;
    if !meta.file_type().is_file() {
        return Err(Stop::new("config", "config"));
    }
    if meta.len() > max as u64 {
        return Err(Stop::new("config", "budget"));
    }
    let bytes = fs::read(path).map_err(|_| Stop::new("config", "config"))?;
    if bytes.len() > max {
        return Err(Stop::new("config", "budget"));
    }
    Ok(bytes)
}

fn decode_config(bytes: &[u8]) -> Result<ApplyConfig, Stop> {
    let value = crate::json_doc::parse(bytes, crate::json_doc::OPERATOR_JSON_MAX)
        .map_err(|fault| Stop::new("config", fault.detail()))?;
    serde_json::from_value(value).map_err(|_| Stop::new("config", "config"))
}

fn read_manifest_bytes(path: &Path) -> Result<Vec<u8>, Stop> {
    let max = crate::json_doc::OPERATOR_JSON_MAX;
    let meta = fs::symlink_metadata(path).map_err(|_| Stop::new("manifest", "manifest"))?;
    if !meta.file_type().is_file() {
        return Err(Stop::new("manifest", "manifest"));
    }
    if meta.len() > max as u64 {
        return Err(Stop::new("manifest", "budget"));
    }
    let bytes = fs::read(path).map_err(|_| Stop::new("manifest", "manifest"))?;
    if bytes.len() > max {
        return Err(Stop::new("manifest", "budget"));
    }
    Ok(bytes)
}

struct ManifestCut {
    source_cut: i64,
    target_fence: i64,
    mapping_run: i64,
    digest: String,
    schema_hash: String,
}

struct EffectiveRun {
    stdout: Vec<u8>,
    hidden: Value,
    effective_oos: Value,
    identity: Value,
    sealed: String,
}

struct PolicyBind<'a> {
    command: &'a str,
    artifact: &'a str,
    decisions: &'a str,
    identity: &'a Value,
    sealed: &'a str,
    schema_hash: &'a str,
    digest: &'a str,
    generation: i64,
}

struct PackOut {
    command: Vec<u8>,
    value: Value,
}

fn apply_in(run_dir: &Path, prep: &Prepared<'_>) -> Result<(), Stop> {
    let cut = manifest_cut(&prep.manifest)?;
    let captured_value = parse_json_slice(&prep.captured, "captured-pg-draft")?;
    if text_field(&captured_value["wrapper"], "schema_hash", "captured-pg-draft")? != cut.schema_hash {
        return Err(Stop::new("captured-pg-draft", "schema_hash"));
    }
    let captured_draft_hash = text_field(&captured_value["wrapper"], "draft_hash", "captured-pg-draft")?.to_string();
    let import_value = parse_json_slice(&prep.import_bytes, "import-pg")?;
    if import_value["source_generation"] != cut.source_cut || import_value["target_generation"] != cut.target_fence {
        return Err(Stop::new("import-pg", "generation"));
    }
    let decisions_bytes = outcome::read_capped(
        Path::new(&prep.cfg.decisions_file),
        crate::json_doc::OPERATOR_JSON_MAX,
        "decisions",
    )?;
    let decisions_value = crate::json_doc::parse(&decisions_bytes, crate::json_doc::OPERATOR_JSON_MAX)
        .map_err(|fault| Stop::new("decisions", fault.detail()))?;
    if !decisions_value.is_array() {
        return Err(Stop::new("decisions", "shape"));
    }
    let reviewed_stdout = run_review(prep, run_dir, &captured_draft_hash, &cut.schema_hash)?;
    let effective = run_effective(prep, run_dir, &reviewed_stdout, &decisions_bytes, &cut.schema_hash)?;
    let policy_arrays = compile_policies(prep, run_dir, &reviewed_stdout, &effective, &decisions_bytes, &cut)?;
    install_roles(prep, run_dir, &policy_arrays, &import_value)?;
    let provision_effects = provision_roles(prep, run_dir, &policy_arrays)?;
    run_queries(prep, run_dir, "before_analyze", &policy_arrays, &cut.digest)?;
    run_analyze(prep, run_dir, &import_value, &cut)?;
    let packs = role_packs(prep, run_dir, &effective.effective_oos)?;
    bundle_roles(prep, run_dir, &packs)?;
    run_queries(prep, run_dir, "after_bundle", &policy_arrays, &cut.digest)?;
    let ready = json!({
        "protocol": "oo.onboarding.outcome/v1",
        "phase": "apply-reviewed",
        "status": "ready_for_trusted_query",
        "source_cut": cut.source_cut,
        "target_fence": cut.target_fence,
        "mapping_run_generation": cut.mapping_run,
        "config_identity_sha256": cut.digest,
        "schema_hash": cut.schema_hash,
        "hidden": effective.hidden,
        "provision": provision_effects,
        "analyze_published": false,
        "estimate_is_exact": false,
        "database_state": "child_compilers_applied_no_release",
        "database_rollback": false,
        "database_release": false,
        "non_claims": outcome::non_claims(false)
    });
    outcome::write_json_new(&run_dir.join("apply-outcome.json"), &ready)?;
    Ok(())
}

fn manifest_cut(manifest: &Value) -> Result<ManifestCut, Stop> {
    let source_cut = manifest["source_cut"].as_i64().ok_or_else(|| Stop::new("manifest", "generation"))?;
    let target_fence = manifest["target_fence"].as_i64().ok_or_else(|| Stop::new("manifest", "generation"))?;
    let mapping_run = manifest["mapping_run_generation"].as_i64().ok_or_else(|| Stop::new("manifest", "generation"))?;
    if mapping_run != source_cut || mapping_run == target_fence {
        return Err(Stop::new("manifest", "fence"));
    }
    let digest = text_field(manifest, "config_identity_sha256", "manifest")?;
    if !hex64(digest) {
        return Err(Stop::new("manifest", "shape"));
    }
    Ok(ManifestCut {
        source_cut,
        target_fence,
        mapping_run,
        digest: digest.to_string(),
        schema_hash: text_field(manifest, "schema_hash", "manifest")?.to_string(),
    })
}

fn run_review(prep: &Prepared<'_>, run_dir: &Path, captured_draft_hash: &str, schema_hash: &str) -> Result<Vec<u8>, Stop> {
    let reviews = outcome::read_json(
        Path::new(&prep.cfg.reviews_file),
        crate::json_doc::OPERATOR_JSON_MAX,
        "reviews",
    )?;
    if !reviews.is_array() {
        return Err(Stop::new("review-pg-draft", "shape"));
    }
    let catalog_value = parse_json_slice(&prep.catalog, "catalog-pg")?;
    let review_stdin = serde_json::to_vec(&json!({"catalog": catalog_value, "reviews": reviews}))
        .map_err(|_| Stop::new("review-pg-draft", "io"))?;
    let reviewed = child(
        prep,
        run_dir,
        "review-pg-draft",
        &prep.bins.review,
        &["review-pg-draft"],
        review_stdin,
        millis(prep.cfg.deadlines_ms.review_ms, 60_000)?,
    )?;
    spawn::require_code("review-pg-draft", &reviewed, 0)?;
    if reviewed.stdout.ends_with(b"\n") {
        return Err(Stop::new("review-pg-draft", "shape"));
    }
    let reviewed_value = parse_json_slice(&reviewed.stdout, "review-pg-draft")?;
    if reviewed_value["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
        return Err(Stop::new("review-pg-draft", "protocol"));
    }
    let reviewed_hash = text_field(&reviewed_value["wrapper"], "draft_hash", "review-pg-draft")?;
    if !hex64(reviewed_hash) || reviewed_hash == captured_draft_hash {
        return Err(Stop::new("review-pg-draft", "draft_hash"));
    }
    if text_field(&reviewed_value["wrapper"], "schema_hash", "review-pg-draft")? != schema_hash {
        return Err(Stop::new("review-pg-draft", "schema_hash"));
    }
    Ok(reviewed.stdout)
}

fn run_effective(
    prep: &Prepared<'_>,
    run_dir: &Path,
    reviewed_stdout: &[u8],
    decisions_bytes: &[u8],
    schema_hash: &str,
) -> Result<EffectiveRun, Stop> {
    let reviewed_value = parse_json_slice(reviewed_stdout, "review-pg-draft")?;
    let decisions_value = parse_json_slice(decisions_bytes, "reviewed-effective-oos")?;
    if !decisions_value.is_array() || !prep.cfg.settings.is_object() {
        return Err(Stop::new("reviewed-effective-oos", "shape"));
    }
    let effective_stdin = serde_json::to_vec(&json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": reviewed_value,
        "settings": prep.cfg.settings,
        "decisions": decisions_value
    }))
    .map_err(|_| Stop::new("reviewed-effective-oos", "io"))?;
    let effective = child(
        prep,
        run_dir,
        "reviewed-effective-oos",
        &prep.bins.effective,
        &[],
        effective_stdin,
        millis(prep.cfg.deadlines_ms.effective_ms, 60_000)?,
    )?;
    spawn::require_code("reviewed-effective-oos", &effective, 0)?;
    if effective.stdout.ends_with(b"\n") {
        return Err(Stop::new("reviewed-effective-oos", "shape"));
    }
    let effective_value = parse_json_slice(&effective.stdout, "reviewed-effective-oos")?;
    if effective_value["protocol"] != "oo.builder.reviewed-effective-oos-command/v1" {
        return Err(Stop::new("reviewed-effective-oos", "protocol"));
    }
    check_provenance(&effective_value)?;
    if effective_value["provenance"]["schema_hash"] != schema_hash {
        return Err(Stop::new("reviewed-effective-oos", "schema_hash"));
    }
    let sealed = text_field(&effective_value["provenance"], "sealed_source_sha256", "reviewed-effective-oos")?;
    if !hex64(sealed) {
        return Err(Stop::new("reviewed-effective-oos", "shape"));
    }
    let effective_oos = effective_value
        .get("effective_oos")
        .cloned()
        .ok_or_else(|| Stop::new("reviewed-effective-oos", "shape"))?;
    let identity = identity_of(&effective_oos)?;
    Ok(EffectiveRun {
        stdout: effective.stdout,
        hidden: effective_value["hidden"].clone(),
        effective_oos,
        identity,
        sealed: sealed.to_string(),
    })
}

fn compile_policies(
    prep: &Prepared<'_>,
    run_dir: &Path,
    reviewed_stdout: &[u8],
    effective: &EffectiveRun,
    decisions_bytes: &[u8],
    cut: &ManifestCut,
) -> Result<Vec<Vec<u8>>, Stop> {
    let command = std::str::from_utf8(&effective.stdout).map_err(|_| Stop::new("reviewed-policy", "shape"))?;
    let artifact = std::str::from_utf8(reviewed_stdout).map_err(|_| Stop::new("reviewed-policy", "shape"))?;
    let decisions = std::str::from_utf8(decisions_bytes).map_err(|_| Stop::new("reviewed-policy", "shape"))?;
    let bind = PolicyBind {
        command,
        artifact,
        decisions,
        identity: &effective.identity,
        sealed: &effective.sealed,
        schema_hash: &cut.schema_hash,
        digest: &cut.digest,
        generation: cut.source_cut,
    };
    let mut policy_arrays = Vec::new();
    for role in &prep.cfg.roles {
        policy_arrays.push(one_policy(prep, run_dir, &bind, role, cut)?);
    }
    Ok(policy_arrays)
}

fn one_policy(
    prep: &Prepared<'_>,
    run_dir: &Path,
    bind: &PolicyBind<'_>,
    role: &RoleCfg,
    cut: &ManifestCut,
) -> Result<Vec<u8>, Stop> {
    let step = format!("reviewed-policy-{}", role.slug);
    let request = policy_request(bind, prep, role)?;
    let output = child(
        prep,
        run_dir,
        &step,
        &prep.bins.policy,
        &[],
        request,
        millis(prep.cfg.deadlines_ms.policy_ms, 60_000)?,
    )?;
    spawn::require_code(&step, &output, 0)?;
    if output.stdout.ends_with(b"\n") {
        return Err(Stop::new(&step, "shape"));
    }
    let array = policies_bytes(&step, &output.stdout)?;
    let parsed_value = parse_json_slice(array, &step)?;
    let Some(parsed) = parsed_value.as_array() else {
        return Err(Stop::new(&step, "shape"));
    };
    for policy in parsed {
        check_policy(
            policy,
            cut.source_cut,
            cut.target_fence,
            &cut.digest,
            &role.slug,
            &prep.cfg.query_login.user,
            &prep.cfg.schema,
        )?;
    }
    Ok(array.to_vec())
}

fn install_roles(prep: &Prepared<'_>, run_dir: &Path, policy_arrays: &[Vec<u8>], import_value: &Value) -> Result<(), Stop> {
    for (role, policies) in prep.cfg.roles.iter().zip(policy_arrays.iter()) {
        install_one(prep, run_dir, role, policies, import_value)?;
    }
    Ok(())
}

fn install_one(
    prep: &Prepared<'_>,
    run_dir: &Path,
    role: &RoleCfg,
    policies: &[u8],
    import_value: &Value,
) -> Result<(), Stop> {
    let step = format!("install-column-acl-{}", role.slug);
    let document = install_document(prep, policies)?;
    let output = child(
        prep,
        run_dir,
        &step,
        &prep.bins.install,
        &[],
        document,
        millis(prep.cfg.deadlines_ms.install_ms, 120_000)?,
    )?;
    spawn::require_code(&step, &output, 0)?;
    let value = parse_json_slice(&output.stdout, &step)?;
    let admin = text_field(&prep.manifest["target"], "administrator", &step)?;
    check_install(&step, &value, import_value, policies, admin, &role.slug)
}

fn provision_roles(prep: &Prepared<'_>, run_dir: &Path, policy_arrays: &[Vec<u8>]) -> Result<Vec<Value>, Stop> {
    let mut effects = Vec::new();
    for (role, policies) in prep.cfg.roles.iter().zip(policy_arrays.iter()) {
        effects.push(provision_one(prep, run_dir, role, policies)?);
    }
    Ok(effects)
}

fn provision_one(prep: &Prepared<'_>, run_dir: &Path, role: &RoleCfg, policies: &[u8]) -> Result<Value, Stop> {
    let step = format!("provision-mapping-reader-{}", role.slug);
    let parsed = parse_json_slice(policies, &step)?;
    if !parsed.is_array() {
        return Err(Stop::new(&step, "shape"));
    }
    let admin = text_field(&prep.manifest["target"], "administrator", &step)?;
    let document = serde_json::to_vec(&json!({
        "protocol": "oo.gateway.provision-mapping-reader-cli/v1",
        "policies": parsed,
        "login": prep.cfg.query_login.user,
        "endpoint": endpoint_user(prep, admin),
        "password_file": prep.manifest["target"]["admin_password_file"],
        "ca_file": prep.manifest["target"]["ca_file"]
    }))
    .map_err(|_| Stop::new(&step, "io"))?;
    let output = child(
        prep,
        run_dir,
        &step,
        &prep.bins.provision,
        &[],
        document,
        millis(prep.cfg.deadlines_ms.provision_ms, 120_000)?,
    )?;
    spawn::require_code(&step, &output, 0)?;
    let value = parse_json_slice(&output.stdout, &step)?;
    if value["protocol"] != "oo.gateway.provision-mapping-reader-cli/v1" {
        return Err(Stop::new(&step, "protocol"));
    }
    let effect = text_field(&value, "effect", &step)?;
    if effect != "installed" && effect != "replay" {
        return Err(Stop::new(&step, "shape"));
    }
    if value["login"] != prep.cfg.query_login.user || value["role"] != format!("oo_r_{}", role.slug) {
        return Err(Stop::new(&step, "shape"));
    }
    Ok(json!({"slug": role.slug, "effect": effect}))
}

fn run_analyze(prep: &Prepared<'_>, run_dir: &Path, import_value: &Value, cut: &ManifestCut) -> Result<(), Stop> {
    let body = analyze_body(prep, import_value, &cut.digest, cut.source_cut)?;
    let analyze = child(
        prep,
        run_dir,
        "analyze-pg",
        &prep.bins.analyze,
        &[],
        body,
        millis(prep.cfg.deadlines_ms.analyze_ms, 120_000)?,
    )?;
    spawn::require_code("analyze-pg", &analyze, 0)?;
    let receipt = parse_json_slice(&analyze.stdout, "analyze-pg")?;
    if receipt_rejected(&receipt, &cut.digest, cut.source_cut) {
        return Err(Stop::new("analyze-pg", "shape"));
    }
    Ok(())
}

fn receipt_rejected(receipt: &Value, digest: &str, source_cut: i64) -> bool {
    receipt["protocol"] != "oo.pipeline.analyze-pg-result/v1"
        || receipt["published"] != false
        || receipt["estimate_is_exact"] != false
        || receipt["config_digest"] != digest
        || receipt["source_generation"] != source_cut
}

fn role_packs(prep: &Prepared<'_>, run_dir: &Path, effective_oos: &Value) -> Result<Vec<PackOut>, Stop> {
    let mut packs = Vec::new();
    for role in &prep.cfg.roles {
        packs.push(role_pack_one(prep, run_dir, role, effective_oos)?);
    }
    Ok(packs)
}

fn role_pack_one(prep: &Prepared<'_>, run_dir: &Path, role: &RoleCfg, effective_oos: &Value) -> Result<PackOut, Stop> {
    let step = format!("role-pack-{}", role.slug);
    let visibility = outcome::read_json(
        Path::new(&role.visibility_file),
        crate::json_doc::OPERATOR_JSON_MAX,
        &step,
    )?;
    let command = serde_json::to_vec(&json!({
        "protocol": "oo.builder.role-pack-command/v1",
        "effective_oos": effective_oos,
        "visibility": visibility
    }))
    .map_err(|_| Stop::new(&step, "io"))?;
    let output = child(
        prep,
        run_dir,
        &step,
        &prep.bins.role_pack,
        &["role-pack"],
        command.clone(),
        millis(prep.cfg.deadlines_ms.role_pack_ms, 60_000)?,
    )?;
    spawn::require_code(&step, &output, 0)?;
    if output.stdout.ends_with(b"\n") {
        return Err(Stop::new(&step, "shape"));
    }
    let value = parse_json_slice(&output.stdout, &step)?;
    check_pack(&step, &value)?;
    Ok(PackOut { command, value })
}

fn bundle_roles(prep: &Prepared<'_>, run_dir: &Path, packs: &[PackOut]) -> Result<(), Stop> {
    fs::create_dir(run_dir.join("bundles")).map_err(|_| Stop::new("role-pack-bundle", "io"))?;
    for (role, pack) in prep.cfg.roles.iter().zip(packs.iter()) {
        bundle_one(prep, run_dir, role, pack)?;
    }
    Ok(())
}

fn bundle_one(prep: &Prepared<'_>, run_dir: &Path, role: &RoleCfg, pack: &PackOut) -> Result<(), Stop> {
    let step = format!("role-pack-bundle-{}", role.slug);
    let request_path = run_dir.join("steps").join(format!("role-pack-{}.stdin", role.slug));
    let dest = run_dir.join("bundles").join(format!("{}.json", role.slug));
    if dest.exists() {
        return Err(Stop::new(&step, "io"));
    }
    if fs::read(&request_path).ok().as_deref() != Some(pack.command.as_slice()) {
        return Err(Stop::new(&step, "shape"));
    }
    let request_arg = request_path.to_string_lossy().into_owned();
    let dest_arg = dest.to_string_lossy().into_owned();
    let output = child(
        prep,
        run_dir,
        &step,
        &prep.bins.bundle,
        &["--request", &request_arg, "--output", &dest_arg],
        Vec::new(),
        millis(prep.cfg.deadlines_ms.bundle_ms, 60_000)?,
    )?;
    spawn::require_code(&step, &output, 0)?;
    if !output.stdout.is_empty() || !output.stderr.is_empty() {
        return Err(Stop::new(&step, "shape"));
    }
    let bundled = outcome::read_json(&dest, outcome::CAPTURE_MAX, &step)?;
    if bundled["protocol"] != "oo.builder.role-pack-bundle/v1" {
        return Err(Stop::new(&step, "protocol"));
    }
    if bundled["pack_digest"] != pack.value["pack_digest"] || bundled["manifest_sha256"] != pack.value["manifest_sha256"] {
        return Err(Stop::new(&step, "shape"));
    }
    if bundled["statuses"]["authentication"] != "not_claimed"
        || bundled["statuses"]["publication"] != "not_claimed"
        || bundled["statuses"]["mount"] != "not_performed"
    {
        return Err(Stop::new(&step, "shape"));
    }
    Ok(())
}

fn run_queries(
    prep: &Prepared<'_>,
    run_dir: &Path,
    when: &str,
    policies: &[Vec<u8>],
    digest: &str,
) -> Result<(), Stop> {
    ensure_dir(&run_dir.join("queries"))?;
    ensure_dir(&run_dir.join("audit"))?;
    for (role, policy_bytes) in prep.cfg.roles.iter().zip(policies.iter()) {
        let parsed = policies_for_query(policy_bytes)?;
        for query in role.queries.iter().filter(|query| query.when == when) {
            run_one_query(prep, run_dir, query, &parsed, digest)?;
        }
    }
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), Stop> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) => Err(Stop::new("query-pg", "io")),
    }
}

fn policies_for_query(policy_bytes: &[u8]) -> Result<Vec<Value>, Stop> {
    let parsed_value = parse_json_slice(policy_bytes, "query-pg")?;
    let Some(parsed) = parsed_value.as_array() else {
        return Err(Stop::new("query-pg", "shape"));
    };
    let mut parsed = parsed.clone();
    for policy in &mut parsed {
        strip_null_row_filter(policy);
    }
    Ok(parsed)
}

fn strip_null_row_filter(policy: &mut Value) {
    if let Some(object) = policy.as_object_mut()
        && object.get("row_filter").is_some_and(Value::is_null)
    {
        object.remove("row_filter");
    }
}

fn run_one_query(prep: &Prepared<'_>, run_dir: &Path, query: &QueryCfg, parsed: &[Value], digest: &str) -> Result<(), Stop> {
    let step = format!("query-{}", query.name);
    let audit = run_dir.join("audit").join(format!("{}.jsonl", query.name));
    if audit.exists() {
        return Err(Stop::new(&step, "io"));
    }
    let (request, stdin) = query_request(prep, query, parsed, digest, &audit)?;
    if spawn::contains_secret(&request, &prep.secrets) {
        return Err(Stop::new(&step, "secret"));
    }
    let request_path = run_dir.join("queries").join(format!("{}.request.json", query.name));
    outcome::write_new(&request_path, &request)?;
    let request_arg = request_path.to_string_lossy().into_owned();
    let output = child(
        prep,
        run_dir,
        &step,
        &prep.bins.query,
        &["--request", &request_arg],
        stdin,
        millis(prep.cfg.deadlines_ms.query_ms, 120_000)?,
    )?;
    accept_query_output(&step, query, &output)
}

fn accept_query_output(step: &str, query: &QueryCfg, output: &spawn::ChildOutput) -> Result<(), Stop> {
    if output.timed_out {
        return Err(Stop::new(step, "deadline"));
    }
    if output.code != Some(query.expected_exit) {
        return Err(Stop::new(step, "exit"));
    }
    if let Some(needle) = &query.expected_stderr
        && !output.stderr.windows(needle.len()).any(|window| window == needle.as_bytes())
    {
        return Err(Stop::new(step, "stderr"));
    }
    Ok(())
}

fn query_request(
    prep: &Prepared<'_>,
    query: &QueryCfg,
    policies: &[Value],
    digest: &str,
    audit: &Path,
) -> Result<(Vec<u8>, Vec<u8>), Stop> {
    let project = text_field(&prep.manifest["source"], "source_id", "query-pg")?;
    let mut statement = Map::new();
    statement.insert("source".to_string(), json!(query.statement.source));
    statement.insert("max_rows".to_string(), json!(query.statement.max_rows));
    statement.insert("max_result_bytes".to_string(), json!(query.statement.max_result_bytes));
    statement.insert("wall_clock_ms".to_string(), json!(query.statement.wall_clock_ms));
    let stdin = match query.statement.source.as_str() {
        "request" => {
            let sql = query.statement.sql.as_deref().ok_or_else(|| Stop::new("query-pg", "config"))?;
            statement.insert("sql".to_string(), json!(sql));
            Vec::new()
        }
        "stdin" => {
            let path = query.statement.sql_file.as_deref().ok_or_else(|| Stop::new("query-pg", "config"))?;
            outcome::read_file(Path::new(path), outcome::CAPTURE_MAX)?
        }
        _ => return Err(Stop::new("query-pg", "config")),
    };
    let role_slug = prep
        .cfg
        .roles
        .iter()
        .find(|role| role.queries.iter().any(|item| item.name == query.name))
        .map(|role| role.slug.as_str())
        .ok_or_else(|| Stop::new("query-pg", "config"))?;
    let body = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": endpoint_user(prep, &prep.cfg.query_login.user),
        "password_file": prep.cfg.query_login.password_file,
        "ca_file": prep.manifest["target"]["ca_file"],
        "audit_file": audit,
        "identity": {
            "project": project,
            "role": format!("oo_r_{role_slug}"),
            "policy_id": digest
        },
        "policies": policies,
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1000000,
            "unknown_rows": "refuse"
        },
        "statement": statement
    });
    let bytes = serde_json::to_vec(&body).map_err(|_| Stop::new("query-pg", "io"))?;
    Ok((bytes, stdin))
}

fn policy_request(bind: &PolicyBind<'_>, prep: &Prepared<'_>, role: &RoleCfg) -> Result<Vec<u8>, Stop> {
    let allow: Vec<Value> = role
        .allow
        .iter()
        .map(|item| {
            json!({
                "dataset": item.dataset,
                "column": item.column,
                "explicit_allow": item.explicit_allow
            })
        })
        .collect();
    let source_id = text_field(&prep.manifest["source"], "source_id", "reviewed-policy")?;
    serde_json::to_vec(&json!({
        "protocol": "oo.gateway.reviewed-policy-cli/v1",
        "command": bind.command,
        "artifact": bind.artifact,
        "decisions": bind.decisions,
        "binding": {
            "identity": bind.identity,
            "seal": {
                "source_id": source_id,
                "schema": prep.cfg.schema,
                "schema_hash": bind.schema_hash,
                "artifact_sha256": bind.sealed
            },
            "config_digest": bind.digest,
            "generation": bind.generation,
            "role": role.slug,
            "login": prep.cfg.query_login.user,
            "allow": allow
        }
    }))
    .map_err(|_| Stop::new("reviewed-policy", "io"))
}

fn identity_of(effective: &Value) -> Result<Value, Stop> {
    Ok(json!({
        "project": text_field(effective, "project", "reviewed-effective-oos")?,
        "ontology_version": text_field(effective, "ontology_version", "reviewed-effective-oos")?,
        "catalog_version": text_field(effective, "catalog_version", "reviewed-effective-oos")?,
        "policy_id": text_field(&effective["policy"], "id", "reviewed-effective-oos")?,
        "policy_version": text_field(&effective["policy"], "version", "reviewed-effective-oos")?,
        "knowledge_id": text_field(&effective["knowledge"], "id", "reviewed-effective-oos")?,
        "knowledge_version": text_field(&effective["knowledge"], "version", "reviewed-effective-oos")?
    }))
}

fn policies_bytes<'a>(step: &str, stdout: &'a [u8]) -> Result<&'a [u8], Stop> {
    if !stdout.starts_with(POLICY_PREFIX) || !stdout.ends_with(b"}") {
        return Err(Stop::new(step, "shape"));
    }
    let array = &stdout[POLICY_PREFIX.len()..stdout.len() - 1];
    if !array.starts_with(b"[") || !array.ends_with(b"]") {
        return Err(Stop::new(step, "shape"));
    }
    Ok(array)
}

fn check_policy(
    policy: &Value,
    source_cut: i64,
    target_fence: i64,
    digest: &str,
    slug: &str,
    login: &str,
    schema: &str,
) -> Result<(), Stop> {
    if policy["generation"] != source_cut || policy["generation"] == target_fence {
        return Err(Stop::new("reviewed-policy", "fence"));
    }
    if policy["config_digest"] != digest || policy["role"] != slug || policy["login"] != login {
        return Err(Stop::new("reviewed-policy", "shape"));
    }
    if policy.get("row_filter") != Some(&Value::Null) {
        return Err(Stop::new("reviewed-policy", "shape"));
    }
    if policy["table"]["namespace"] != schema {
        return Err(Stop::new("reviewed-policy", "shape"));
    }
    Ok(())
}

fn install_document(prep: &Prepared<'_>, policies: &[u8]) -> Result<Vec<u8>, Stop> {
    let admin = text_field(&prep.manifest["target"], "administrator", "install-column-acl")?;
    let host = text_field(&prep.manifest["target"], "host", "install-column-acl")?;
    let database = text_field(&prep.manifest["target"], "database", "install-column-acl")?;
    let password = text_field(&prep.manifest["target"], "admin_password_file", "install-column-acl")?;
    let ca = text_field(&prep.manifest["target"], "ca_file", "install-column-acl")?;
    let port = prep.manifest["target"]["port"].as_u64().ok_or_else(|| Stop::new("install-column-acl", "shape"))?;
    for token in [admin, host, database, password, ca] {
        if !plain_token(token) {
            return Err(Stop::new("install-column-acl", "config"));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"protocol\":\"oo.gateway.install-column-acl-cli/v1\",\"policies\":");
    out.extend_from_slice(policies);
    let tail = format!(
        ",\"endpoint\":{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\",\"user\":\"{admin}\"}},\"password_file\":\"{password}\",\"ca_file\":\"{ca}\"}}"
    );
    out.extend_from_slice(tail.as_bytes());
    Ok(out)
}

fn check_install(
    step: &str,
    value: &Value,
    import_value: &Value,
    policies: &[u8],
    admin: &str,
    slug: &str,
) -> Result<(), Stop> {
    if value["protocol"] != "oo.gateway.install-column-acl-cli/v1" || value["effect"] != "installed" {
        return Err(Stop::new(step, "shape"));
    }
    if value["role"] != format!("oo_r_{slug}") || value["grantor"] == "oo_target_owner" || value["grantor"] != admin {
        return Err(Stop::new(step, "grantor"));
    }
    let parsed_value = parse_json_slice(policies, step)?;
    let Some(parsed) = parsed_value.as_array() else {
        return Err(Stop::new(step, "shape"));
    };
    let granted = value["tables"].as_array().ok_or_else(|| Stop::new(step, "shape"))?;
    if granted.len() != parsed.len() {
        return Err(Stop::new(step, "shape"));
    }
    let imported = import_value["tables"].as_array().ok_or_else(|| Stop::new(step, "shape"))?;
    for (policy, table) in parsed.iter().zip(granted.iter()) {
        let namespace = policy["table"]["namespace"].as_str().ok_or_else(|| Stop::new(step, "shape"))?;
        let name = policy["table"]["name"].as_str().ok_or_else(|| Stop::new(step, "shape"))?;
        let physical = text_field(table, "physical_table", step)?;
        let expect = imported
            .iter()
            .find(|item| item["schema"] == namespace && item["name"] == name)
            .ok_or_else(|| Stop::new(step, "shape"))?;
        if text_field(expect, "physical_name", step)? != physical || !physical_ok(physical, "pt_") {
            return Err(Stop::new(step, "shape"));
        }
        let logical = policy["table"]["columns"].as_array().ok_or_else(|| Stop::new(step, "shape"))?;
        let columns = table["physical_columns"].as_array().ok_or_else(|| Stop::new(step, "shape"))?;
        if logical.len() != columns.len() {
            return Err(Stop::new(step, "shape"));
        }
        let import_columns = expect["columns"].as_array().ok_or_else(|| Stop::new(step, "shape"))?;
        for (column, got) in logical.iter().zip(columns.iter()) {
            let logical_name = column["name"].as_str().ok_or_else(|| Stop::new(step, "shape"))?;
            let got_name = got.as_str().ok_or_else(|| Stop::new(step, "shape"))?;
            let expect_col = import_columns
                .iter()
                .find(|item| item["name"] == logical_name)
                .ok_or_else(|| Stop::new(step, "shape"))?;
            if text_field(expect_col, "physical_name", step)? != got_name || !physical_ok(got_name, "pc_") {
                return Err(Stop::new(step, "shape"));
            }
        }
    }
    Ok(())
}

fn analyze_body(prep: &Prepared<'_>, import_value: &Value, digest: &str, source_cut: i64) -> Result<Vec<u8>, Stop> {
    let tables = import_value["tables"].as_array().ok_or_else(|| Stop::new("analyze-pg", "shape"))?;
    let listed: Vec<Value> = tables
        .iter()
        .map(|table| {
            json!({
                "namespace": table["schema"],
                "name": table["name"]
            })
        })
        .collect();
    let source_id = text_field(&prep.manifest["source"], "source_id", "analyze-pg")?;
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.analyze-pg/v1",
        "project": source_id,
        "source": source_id,
        "config_digest": digest,
        "source_generation": source_cut,
        "tables": listed,
        "statement_timeout_ms": prep.cfg.analyze.statement_timeout_ms,
        "lock_timeout_ms": prep.cfg.analyze.lock_timeout_ms,
        "connect_ms": prep.cfg.analyze.connect_ms,
        "target": {
            "host": prep.manifest["target"]["host"],
            "port": prep.manifest["target"]["port"],
            "database": prep.manifest["target"]["database"],
            "operator": prep.manifest["target"]["administrator"],
            "password_file": prep.manifest["target"]["admin_password_file"],
            "ca_file": prep.manifest["target"]["ca_file"]
        }
    }))
    .map_err(|_| Stop::new("analyze-pg", "io"))
}

fn endpoint_user(prep: &Prepared<'_>, user: &str) -> Value {
    json!({
        "host": prep.manifest["target"]["host"],
        "port": prep.manifest["target"]["port"],
        "database": prep.manifest["target"]["database"],
        "user": user
    })
}

fn check_provenance(value: &Value) -> Result<(), Stop> {
    for key in ["authentication", "authorization", "grants", "publication", "semantic_publication"] {
        if value["provenance"][key] != "not_claimed" {
            return Err(Stop::new("reviewed-effective-oos", "provenance"));
        }
    }
    Ok(())
}

fn check_pack(step: &str, value: &Value) -> Result<(), Stop> {
    if value["protocol"] != "oo.builder.role-pack-command/v1" {
        return Err(Stop::new(step, "protocol"));
    }
    let digest = text_field(value, "pack_digest", step)?;
    let manifest = text_field(value, "manifest_sha256", step)?;
    if !hex64(digest) || !hex64(manifest) {
        return Err(Stop::new(step, "shape"));
    }
    let body = &value["manifest"];
    if body["authentication"] != "not_claimed"
        || body["authorization"] != "not_claimed"
        || body["publication"] != "not_claimed"
        || body["mount"] != "not_performed"
        || body["samples"] != "omitted"
        || body["row_security_reproven"] != false
    {
        return Err(Stop::new(step, "shape"));
    }
    Ok(())
}

fn child(
    prep: &Prepared<'_>,
    run_dir: &Path,
    step: &str,
    bin: &Path,
    args: &[&str],
    stdin: Vec<u8>,
    deadline: Duration,
) -> Result<spawn::ChildOutput, Stop> {
    run_child(&ChildRequest {
        step: step.to_string(),
        bin: bin.to_path_buf(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        stdin,
        run_dir: run_dir.to_path_buf(),
        deadline,
        secrets: prep.secrets.clone(),
    }, &prep.child_started)
}

fn validate_apply(cfg: &ApplyConfig) -> Result<(), Stop> {
    if cfg.protocol != APPLY_PROTOCOL || cfg.caller != "trusted_local_admin" {
        return Err(Stop::new("config", "caller"));
    }
    if !plain_token(&cfg.schema) || cfg.roles.is_empty() || !cfg.settings.is_object() {
        return Err(Stop::new("config", "config"));
    }
    if !plain_token(&cfg.query_login.user) || !Path::new(&cfg.query_login.password_file).is_absolute() {
        return Err(Stop::new("config", "config"));
    }
    if !Path::new(&cfg.reviews_file).is_absolute() || !Path::new(&cfg.decisions_file).is_absolute() {
        return Err(Stop::new("config", "config"));
    }
    if !(1..=120_000).contains(&cfg.analyze.connect_ms) {
        return Err(Stop::new("config", "config"));
    }
    let mut names = Vec::new();
    for role in &cfg.roles {
        if !spawn::safe_name(&role.slug) || role.allow.is_empty() || !Path::new(&role.visibility_file).is_absolute() {
            return Err(Stop::new("config", "config"));
        }
        for query in &role.queries {
            validate_query(query, &mut names)?;
        }
    }
    Ok(())
}

fn validate_query(query: &QueryCfg, names: &mut Vec<String>) -> Result<(), Stop> {
    if !spawn::safe_name(&query.name) || (query.when != "before_analyze" && query.when != "after_bundle") {
        return Err(Stop::new("config", "config"));
    }
    if query_limits_rejected(query) || names.contains(&query.name) {
        return Err(Stop::new("config", "config"));
    }
    names.push(query.name.clone());
    Ok(())
}

fn query_limits_rejected(query: &QueryCfg) -> bool {
    query.statement.max_rows > 2000
        || query.statement.max_result_bytes > 262_144
        || !(1..=30_000).contains(&query.statement.wall_clock_ms)
}

fn apply_secrets(cfg: &ApplyConfig, manifest: &Value) -> Result<Vec<Vec<u8>>, Stop> {
    let mut secrets = Vec::new();
    for key in ["admin_password_file", "runtime_password_file"] {
        let path = text_field(&manifest["target"], key, "manifest")?;
        secrets.push(load_password(Path::new(path))?);
    }
    let source_password = text_field(&manifest["source"], "password_file", "manifest")?;
    secrets.push(load_password(Path::new(source_password))?);
    secrets.push(load_password(Path::new(&cfg.query_login.password_file))?);
    Ok(secrets)
}

fn pin_apply(cfg: &ApplyConfig) -> Result<Bins, Stop> {
    Ok(Bins {
        review: pin_binary(&cfg.binaries.review_pg_draft)?,
        effective: pin_binary(&cfg.binaries.reviewed_effective_oos)?,
        policy: pin_binary(&cfg.binaries.reviewed_policy)?,
        install: pin_binary(&cfg.binaries.install_column_acl)?,
        provision: pin_binary(&cfg.binaries.provision_mapping_reader)?,
        analyze: pin_binary(&cfg.binaries.analyze_pg)?,
        role_pack: pin_binary(&cfg.binaries.role_pack)?,
        bundle: pin_binary(&cfg.binaries.role_pack_bundle)?,
        query: pin_binary(&cfg.binaries.query_pg)?,
    })
}

fn read_hashed(run_dir: &Path, name: &str, manifest: &Value, field: &str) -> Result<Vec<u8>, Stop> {
    let bytes = outcome::read_file(&run_dir.join(name), outcome::CAPTURE_MAX)?;
    let expect = text_field(manifest, field, "manifest")?;
    if sha256_hex(&bytes) != expect {
        return Err(Stop::new("manifest", "manifest"));
    }
    Ok(bytes)
}

fn refuse_existing_apply_steps(run_dir: &Path) -> Result<(), Stop> {
    let steps = run_dir.join("steps");
    let entries = fs::read_dir(&steps).map_err(|_| Stop::new("run", "io"))?;
    for entry in entries {
        let entry = entry.map_err(|_| Stop::new("run", "io"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stem = name.split('.').next().unwrap_or("");
        let inspect_step = matches!(
            stem,
            "import-not-ready" | "init-pg" | "import-pg" | "catalog-pg" | "captured-pg-draft"
        );
        if !inspect_step {
            return Err(Stop::new("run", "retry"));
        }
    }
    Ok(())
}

fn millis(value: Option<u64>, default_ms: u64) -> Result<Duration, Stop> {
    let ms = value.unwrap_or(default_ms);
    if !(1..=180_000).contains(&ms) {
        return Err(Stop::new("config", "config"));
    }
    Ok(Duration::from_millis(ms))
}
