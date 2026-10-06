//! Compiled documents: catalog, review, effective OOS, role packs, bundles.
//!
//! Human decisions are fixture inputs. Confirmed labels in the effective
//! document are whatever `reviewed-effective-oos` emitted.

use std::fs;

use serde_json::{json, Value};

use crate::bridge::hidden_reason;
use crate::fixture::{self, DECISIONS};
use crate::live::{Live, GENERATION, OWNER, READER, SCHEMA, SHARD, SOURCE_DB, SOURCE_ID, TARGET_DB};
use crate::proc::{
    contains_bytes, parse_json, require_exit, require_reject, run_cmd, save_cmd, sha256_hex, stderr_text, CmdOut,
};
use crate::stop::{disk, Stop};

pub struct Docs {
    pub catalog_sha: String,
    pub captured_draft_hash: String,
    pub reviewed_draft_hash: String,
    pub schema_hash: String,
    pub decisions_sha256: String,
    pub sealed_source_sha256: String,
    pub effective_oos: Value,
    pub command_stdout: Vec<u8>,
    pub artifact_stdout: Vec<u8>,
    pub decisions_json: Vec<u8>,
    pub hidden_private: String,
    pub clerk_digest: String,
    pub peek_digest: String,
    pub clerk_manifest: String,
    pub peek_manifest: String,
    pub clerk_bundle_sha: String,
    pub peek_bundle_sha: String,
    pub clerk_pack_sha256: String,
    pub peek_pack_sha256: String,
    pub second_exit: i32,
    pub second_destination: bool,
    pub clerk_has_fk: bool,
    pub peek_has_fk: bool,
}

pub fn compile(live: &Live) -> Result<Docs, Stop> {
    let (catalog_sha, catalog, captured, reviewed, artifact_stdout) = catalog_review(live)?;
    reject_aliases(live, &reviewed)?;
    let classified = classify(live, &reviewed)?;
    let hidden_private = hidden_reason(&classified.command, "shop.customer", "private_note")?;
    let packs = seal_packs(live, &classified.effective)?;
    let mut docs = assemble(catalog, captured, reviewed, classified.command, classified.effective, packs);
    docs.catalog_sha = catalog_sha;
    docs.hidden_private = hidden_private;
    docs.command_stdout = classified.stdout;
    docs.artifact_stdout = artifact_stdout;
    docs.decisions_json = classified.decisions_json;
    assert_draft_changed(&docs)?;
    Ok(docs)
}

fn catalog_review(live: &Live) -> Result<(String, Value, Value, Value, Vec<u8>), Stop> {
    let catalog_bytes = catalog_request(live)?;
    let catalog = run_cmd(&live.pipeline, &["catalog-pg"], &catalog_bytes, 120)?;
    save_cmd(live, "catalog-pg", &catalog)?;
    require_exit("catalog-pg", &catalog, 0)?;
    refuse_marker("catalog-pg", &catalog.stdout)?;
    let catalog_value = parse_json(&catalog.stdout, "catalog-pg")?;
    if catalog_value["protocol"] != "oo.source.pgsql/v1" {
        return Err(Stop::new("catalog-pg", "protocol"));
    }
    let captured = run_cmd(&live.builder, &["captured-pg-draft"], &catalog.stdout, 60)?;
    save_cmd(live, "captured-pg-draft", &captured)?;
    require_exit("captured-pg-draft", &captured, 0)?;
    refuse_marker("captured-pg-draft", &captured.stdout)?;
    let captured_value = parse_json(&captured.stdout, "captured-pg-draft")?;
    if captured_value["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
        return Err(Stop::new("captured-pg-draft", "protocol"));
    }
    let (reviewed, artifact_stdout) = review(live, &catalog_value, &captured_value)?;
    Ok((sha256_hex(&catalog.stdout), catalog_value, captured_value, reviewed, artifact_stdout))
}

fn review(live: &Live, catalog: &Value, captured: &Value) -> Result<(Value, Vec<u8>), Stop> {
    let reviews = structural_reviews(captured)?;
    let bytes = serde_json::to_vec(&json!({"catalog": catalog, "reviews": reviews}))
        .map_err(|_| Stop::new("review-pg-draft", "encode"))?;
    let reviewed = run_cmd(&live.builder, &["review-pg-draft"], &bytes, 60)?;
    save_cmd(live, "review-pg-draft", &reviewed)?;
    require_exit("review-pg-draft", &reviewed, 0)?;
    if reviewed.stdout.ends_with(b"\n") {
        return Err(Stop::new("review-pg-draft", "trailing newline"));
    }
    refuse_marker("review-pg-draft", &reviewed.stdout)?;
    let value = parse_json(&reviewed.stdout, "review-pg-draft")?;
    if value["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
        return Err(Stop::new("review-pg-draft", "protocol"));
    }
    let pii = column_field(&value, "shop.customer", "customer_id", "pii")?;
    if !pii.is_null() {
        return Err(Stop::new("review-pg-draft", "structural accept set pii"));
    }
    Ok((value, reviewed.stdout))
}

struct Classified {
    stdout: Vec<u8>,
    command: Value,
    effective: Value,
    decisions_json: Vec<u8>,
}

fn reject_aliases(live: &Live, reviewed: &Value) -> Result<(), Stop> {
    let raw = serde_json::to_vec(reviewed).map_err(|_| Stop::new("role-pack-alias", "encode"))?;
    let direct = run_cmd(&live.builder, &["role-pack"], &raw, 60)?;
    save_cmd(live, "role-pack-review-stdout", &direct)?;
    require_reject(
        "role-pack-review-stdout",
        &direct,
        &["field.unknown", "schema.rejected"],
    )?;
    let wrapped = serde_json::to_vec(&json!({
        "protocol": "oo.builder.role-pack-command/v1",
        "effective_oos": reviewed,
        "visibility": visibility("oo_r_clerk", &fixture::CLERK_CUSTOMER, &fixture::CLERK_ORDER)
    }))
    .map_err(|_| Stop::new("role-pack-wrapped", "encode"))?;
    let wrapped_out = run_cmd(&live.builder, &["role-pack"], &wrapped, 60)?;
    save_cmd(live, "role-pack-wrapped-draft", &wrapped_out)?;
    require_reject("role-pack-wrapped-draft", &wrapped_out, &["schema.rejected"])?;
    Ok(())
}

fn classify(live: &Live, reviewed: &Value) -> Result<Classified, Stop> {
    let decision_list = decisions(reviewed)?;
    let decisions_json = serde_json::to_vec(&decision_list).map_err(|_| Stop::new("reviewed-effective-oos", "decisions"))?;
    let bytes = serde_json::to_vec(&json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": reviewed,
        "settings": settings(),
        "decisions": decision_list
    }))
    .map_err(|_| Stop::new("reviewed-effective-oos", "encode"))?;
    let output = run_cmd(&live.reviewed, &[], &bytes, 60)?;
    save_cmd(live, "reviewed-effective-oos", &output)?;
    require_exit("reviewed-effective-oos", &output, 0)?;
    if output.stdout.ends_with(b"\n") {
        return Err(Stop::new("reviewed-effective-oos", "trailing newline"));
    }
    refuse_marker("reviewed-effective-oos", &output.stdout)?;
    let value = parse_json(&output.stdout, "reviewed-effective-oos")?;
    if value["protocol"] != "oo.builder.reviewed-effective-oos-command/v1" {
        return Err(Stop::new("reviewed-effective-oos", "protocol"));
    }
    check_provenance(&value)?;
    let effective = value
        .get("effective_oos")
        .cloned()
        .ok_or_else(|| Stop::new("reviewed-effective-oos", "effective_oos"))?;
    if effective["protocol"] != "oo.builder.effective-oos/v1" || effective["project"] != SOURCE_ID {
        return Err(Stop::new("reviewed-effective-oos", "effective protocol or project"));
    }
    check_applied(&effective)?;
    let private_probe = private_visibility_probe(live, &effective)?;
    require_reject("role-pack-private-note", &private_probe, &["classification.unknown_pii"])?;
    Ok(Classified {
        stdout: output.stdout,
        command: value,
        effective,
        decisions_json,
    })
}

fn private_visibility_probe(live: &Live, effective: &Value) -> Result<CmdOut, Stop> {
    let mut columns = fixture::CLERK_CUSTOMER.to_vec();
    columns.push("private_note");
    let bytes = pack_command(effective, &visibility("oo_r_clerk", &columns, &fixture::CLERK_ORDER))?;
    let output = run_cmd(&live.builder, &["role-pack"], &bytes, 60)?;
    save_cmd(live, "role-pack-private-note", &output)?;
    Ok(output)
}

struct Packs {
    clerk_digest: String,
    peek_digest: String,
    clerk_manifest: String,
    peek_manifest: String,
    clerk_bundle_sha: String,
    peek_bundle_sha: String,
    clerk_pack_sha256: String,
    peek_pack_sha256: String,
    second_exit: i32,
    second_destination: bool,
    clerk_has_fk: bool,
    peek_has_fk: bool,
}

fn seal_packs(live: &Live, effective: &Value) -> Result<Packs, Stop> {
    let clerk_vis = visibility("oo_r_clerk", &fixture::CLERK_CUSTOMER, &fixture::CLERK_ORDER);
    let peek_vis = visibility("oo_r_peek", &fixture::PEEK_CUSTOMER, &fixture::PEEK_ORDER);
    let clerk = one_pack(live, effective, &clerk_vis, "clerk")?;
    let peek = one_pack(live, effective, &peek_vis, "peek")?;
    if !clerk.stdout_has_fk || contains_bytes(&clerk.stdout, "private_note") {
        return Err(Stop::new("role-pack-clerk", "fk text or private_note"));
    }
    if contains_bytes(&peek.stdout, "private_note") {
        return Err(Stop::new("role-pack-peek", "private_note"));
    }
    if contains_bytes(&peek.stdout, "clerk_note") {
        return Err(Stop::new("role-pack-peek", "clerk_note name was emitted"));
    }
    let clerk_bundle = one_bundle(live, &clerk.command, "clerk", &clerk.digest, &clerk.manifest)?;
    let peek_bundle = one_bundle(live, &peek.command, "peek", &peek.digest, &peek.manifest)?;
    let again = second_bundle(live, &clerk.command, &clerk_bundle)?;
    Ok(Packs {
        clerk_digest: clerk.digest,
        peek_digest: peek.digest,
        clerk_manifest: clerk.manifest,
        peek_manifest: peek.manifest,
        clerk_bundle_sha: clerk_bundle.file_sha,
        peek_bundle_sha: peek_bundle.file_sha,
        clerk_pack_sha256: clerk_bundle.pack_sha256,
        peek_pack_sha256: peek_bundle.pack_sha256,
        second_exit: again.0,
        second_destination: again.1,
        clerk_has_fk: clerk.stdout_has_fk,
        peek_has_fk: contains_bytes(&peek.stdout, "order_customer_fk"),
    })
}

struct PackOut {
    digest: String,
    manifest: String,
    command: Vec<u8>,
    stdout: Vec<u8>,
    stdout_has_fk: bool,
}

fn one_pack(live: &Live, effective: &Value, visibility: &Value, name: &str) -> Result<PackOut, Stop> {
    let command = pack_command(effective, visibility)?;
    let output = run_cmd(&live.builder, &["role-pack"], &command, 60)?;
    save_cmd(live, &format!("role-pack-{name}"), &output)?;
    require_exit(&format!("role-pack-{name}"), &output, 0)?;
    if output.stdout.ends_with(b"\n") {
        return Err(Stop::new("role-pack", "trailing newline"));
    }
    refuse_marker("role-pack", &output.stdout)?;
    let value = parse_json(&output.stdout, "role-pack")?;
    if value["protocol"] != "oo.builder.role-pack-command/v1" {
        return Err(Stop::new("role-pack", "protocol"));
    }
    check_manifest(&value)?;
    let digest = text_field(&value, "pack_digest", "role-pack")?;
    let manifest = text_field(&value, "manifest_sha256", "role-pack")?;
    if digest.len() != 64 || manifest.len() != 64 {
        return Err(Stop::new("role-pack", "digest length"));
    }
    Ok(PackOut {
        digest,
        manifest,
        command,
        stdout_has_fk: contains_bytes(&output.stdout, "order_customer_fk")
            && contains_bytes(&output.stdout, "not a grant"),
        stdout: output.stdout,
    })
}

struct BundleOut {
    file_sha: String,
    pack_sha256: String,
}

fn one_bundle(
    live: &Live,
    command: &[u8],
    name: &str,
    pack_digest: &str,
    manifest: &str,
) -> Result<BundleOut, Stop> {
    let request = live.evidence.join(format!("role-pack-{name}.command.json"));
    fs::write(&request, command).map_err(|err| disk("role-pack-bundle", &err))?;
    let dest = live.bundle_dir.join(format!("{name}.json"));
    if dest.exists() {
        return Err(Stop::new("role-pack-bundle", "output already existed"));
    }
    let output = run_cmd(
        &live.bundle,
        &["--request", &request.to_string_lossy(), "--output", &dest.to_string_lossy()],
        b"",
        60,
    )?;
    save_cmd(live, &format!("role-pack-bundle-{name}"), &output)?;
    require_exit(&format!("role-pack-bundle-{name}"), &output, 0)?;
    if !output.stdout.is_empty() || !output.stderr.is_empty() {
        return Err(Stop::new("role-pack-bundle", "success wrote stdout or stderr"));
    }
    let bytes = fs::read(&dest).map_err(|err| disk("role-pack-bundle", &err))?;
    fs::create_dir_all(live.evidence.join("bundles")).map_err(|err| disk("bundles", &err))?;
    fs::write(live.evidence.join(format!("bundles/{name}.json")), &bytes).map_err(|err| disk("bundles", &err))?;
    let value = parse_json(&bytes, "role-pack-bundle")?;
    if value["protocol"] != "oo.builder.role-pack-bundle/v1" {
        return Err(Stop::new("role-pack-bundle", "protocol"));
    }
    if value["pack_digest"] != pack_digest || value["manifest_sha256"] != manifest {
        return Err(Stop::new("role-pack-bundle", "digest does not match the role pack"));
    }
    if value["statuses"]["authentication"] != "not_claimed"
        || value["statuses"]["publication"] != "not_claimed"
        || value["statuses"]["mount"] != "not_performed"
    {
        return Err(Stop::new("role-pack-bundle", "status"));
    }
    Ok(BundleOut {
        file_sha: sha256_hex(&bytes),
        pack_sha256: text_field(&value, "pack_sha256", "role-pack-bundle")?,
    })
}

fn second_bundle(live: &Live, command: &[u8], first: &BundleOut) -> Result<(i32, bool), Stop> {
    let request = live.evidence.join("role-pack-clerk.command.json");
    let dest = live.bundle_dir.join("clerk.json");
    let before = fs::read(&dest).map_err(|err| disk("role-pack-bundle", &err))?;
    if sha256_hex(&before) != first.file_sha {
        return Err(Stop::new("role-pack-bundle", "copied bytes differ"));
    }
    let _ = command;
    if !request.is_file() {
        return Err(Stop::new("role-pack-bundle", "request missing"));
    }
    let output = run_cmd(
        &live.bundle,
        &["--request", &request.to_string_lossy(), "--output", &dest.to_string_lossy()],
        b"",
        60,
    )?;
    save_cmd(live, "role-pack-bundle-clerk-again", &output)?;
    let after = fs::read(&dest).map_err(|err| disk("role-pack-bundle", &err))?;
    if before != after {
        return Err(Stop::new("role-pack-bundle", "second call changed the file"));
    }
    let destination = stderr_text(&output.stderr).contains("io.destination");
    if output.code != 1 || !destination {
        return Err(Stop::new(
            "role-pack-bundle",
            format!("second exit {} stderr={}", output.code, stderr_text(&output.stderr)),
        ));
    }
    Ok((output.code, destination))
}

fn assemble(
    catalog: Value,
    captured: Value,
    reviewed: Value,
    effective: Value,
    effective_oos: Value,
    packs: Packs,
) -> Docs {
    Docs {
        catalog_sha: sha256_hex(serde_json::to_vec(&catalog).unwrap_or_default().as_slice()),
        captured_draft_hash: captured["wrapper"]["draft_hash"].as_str().unwrap_or("").to_string(),
        reviewed_draft_hash: reviewed["wrapper"]["draft_hash"].as_str().unwrap_or("").to_string(),
        schema_hash: effective["provenance"]["schema_hash"].as_str().unwrap_or("").to_string(),
        decisions_sha256: effective["provenance"]["decisions_sha256"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        sealed_source_sha256: effective["provenance"]["sealed_source_sha256"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        effective_oos,
        command_stdout: Vec::new(),
        artifact_stdout: Vec::new(),
        decisions_json: Vec::new(),
        hidden_private: String::new(),
        clerk_digest: packs.clerk_digest,
        peek_digest: packs.peek_digest,
        clerk_manifest: packs.clerk_manifest,
        peek_manifest: packs.peek_manifest,
        clerk_bundle_sha: packs.clerk_bundle_sha,
        peek_bundle_sha: packs.peek_bundle_sha,
        clerk_pack_sha256: packs.clerk_pack_sha256,
        peek_pack_sha256: packs.peek_pack_sha256,
        second_exit: packs.second_exit,
        second_destination: packs.second_destination,
        clerk_has_fk: packs.clerk_has_fk,
        peek_has_fk: packs.peek_has_fk,
    }
}

fn catalog_request(live: &Live) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": source_object(live)
    }))
    .map_err(|_| Stop::new("catalog-pg", "encode"))
}

pub fn import_request(live: &Live) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": source_object(live),
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "runtime_principal": crate::live::RUNTIME,
            "password_file": "/secrets/runtime.pass",
            "ca_file": "/secrets/ca.crt"
        },
        "shard": SHARD,
        "owner": OWNER,
        "source_generation": GENERATION
    }))
    .map_err(|_| Stop::new("import-pg", "encode"))
}

pub fn init_request(live: &Live) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "connect_ms": 30000,
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "administrator": live.admin_user,
            "password_file": "/secrets/admin.pass",
            "ca_file": "/secrets/ca.crt"
        }
    }))
    .map_err(|_| Stop::new("init-pg", "encode"))
}

fn source_object(live: &Live) -> Value {
    json!({
        "source_id": SOURCE_ID,
        "host": live.host,
        "port": live.port,
        "database": SOURCE_DB,
        "username": READER,
        "secret_ref": "secret://pg/a248/reader",
        "password_file": "/secrets/reader.pass",
        "ca_file": "/secrets/ca.crt",
        "tables": [
            {"schema": SCHEMA, "table": "customer"},
            {"schema": SCHEMA, "table": "order_doc"}
        ]
    })
}

fn structural_reviews(captured: &Value) -> Result<Value, Stop> {
    let schema_hash = captured["wrapper"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema_hash"))?;
    if schema_hash.len() != 64 {
        return Err(Stop::new("captured-pg-draft", "schema_hash length"));
    }
    let queue = captured["draft"]["review_queue"]
        .as_array()
        .ok_or_else(|| Stop::new("captured-pg-draft", "review_queue"))?;
    let mut reviews = Vec::new();
    for item in queue {
        if let Some(review) = one_review(item, schema_hash)? {
            reviews.push(review);
        }
    }
    if reviews.is_empty() {
        return Err(Stop::new("captured-pg-draft", "no structural review items"));
    }
    Ok(Value::Array(reviews))
}

fn one_review(item: &Value, schema_hash: &str) -> Result<Option<Value>, Stop> {
    let kind = item["kind"].as_str().unwrap_or("");
    let item_id = item["item_id"].as_str().unwrap_or("");
    if kind != "structural_column" && kind != "structural_primary_key" && kind != "candidate_relationship" {
        return Ok(None);
    }
    if item_id.starts_with("pii/")
        || item_id.starts_with("semantic/")
        || item_id.starts_with("type/")
        || item_id.starts_with("classification/")
    {
        return Ok(None);
    }
    let fingerprint = item["fact_fingerprint"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "fact_fingerprint"))?;
    Ok(Some(json!({
        "item_id": item_id,
        "action": "accept",
        "schema_hash": schema_hash,
        "fact_fingerprint": fingerprint
    })))
}

fn decisions(artifact: &Value) -> Result<Vec<Value>, Stop> {
    let schema = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("review-pg-draft", "schema_hash"))?;
    if schema.len() != 64 {
        return Err(Stop::new("review-pg-draft", "schema_hash length"));
    }
    let mut out = Vec::new();
    for (dataset, column, pii, semantic) in DECISIONS {
        let fingerprint = fingerprint(artifact, dataset, column)?;
        out.push(json!({
            "dataset": dataset,
            "column": column,
            "schema_hash": schema,
            "fact_fingerprint": fingerprint,
            "pii": pii,
            "semantic_type": semantic,
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }));
    }
    Ok(out)
}

fn fingerprint(artifact: &Value, dataset: &str, column: &str) -> Result<String, Stop> {
    let item_id = format!("column/{dataset}/{column}");
    let queue = artifact["draft"]["review_queue"]
        .as_array()
        .ok_or_else(|| Stop::new("review-pg-draft", "review_queue"))?;
    queue
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
        .and_then(|item| item["fact_fingerprint"].as_str())
        .map(str::to_string)
        .ok_or_else(|| Stop::new("review-pg-draft", format!("fingerprint {item_id}")))
}

fn settings() -> Value {
    json!({
        "project": SOURCE_ID,
        "ontology_version": "ont-a248",
        "catalog_version": "cat-a248",
        "policy": {"id": "pol-a248", "version": "1"},
        "knowledge": {"id": "know-a248", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["shop"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn visibility(role: &str, customer: &[&str], order: &[&str]) -> Value {
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": SOURCE_ID,
        "principal": "fixture_operator",
        "role": role,
        "ontology_version": "ont-a248",
        "catalog_version": "cat-a248",
        "policy": {"id": "pol-a248", "version": "1"},
        "knowledge": {"id": "know-a248", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {"dataset": "shop.customer", "row_security": "none", "global_stats": "denied", "columns": customer},
            {"dataset": "shop.order_doc", "row_security": "none", "global_stats": "denied", "columns": order}
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

fn pack_command(effective: &Value, visibility: &Value) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.builder.role-pack-command/v1",
        "effective_oos": effective,
        "visibility": visibility
    }))
    .map_err(|_| Stop::new("role-pack", "encode"))
}

fn check_provenance(value: &Value) -> Result<(), Stop> {
    for key in [
        "authentication",
        "authorization",
        "grants",
        "publication",
        "semantic_publication",
    ] {
        if value["provenance"][key] != "not_claimed" {
            return Err(Stop::new("reviewed-effective-oos", key));
        }
    }
    let reason = hidden_reason(value, "shop.customer", "private_note")?;
    if reason != "classification.absent" {
        return Err(Stop::new("reviewed-effective-oos", reason));
    }
    Ok(())
}

fn check_applied(effective: &Value) -> Result<(), Stop> {
    for (dataset, column, pii, semantic) in DECISIONS {
        let found = crate::bridge::find_column(effective, dataset, column)?;
        if found["pii"] != pii || found["semantic_type"] != semantic || found["classification"]["state"] != "confirmed" {
            return Err(Stop::new("reviewed-effective-oos", format!("applied {dataset}.{column}")));
        }
    }
    let private = crate::bridge::find_column(effective, "shop.customer", "private_note")?;
    if !private["pii"].is_null() || !private["semantic_type"].is_null() {
        return Err(Stop::new("reviewed-effective-oos", "private_note was classified"));
    }
    Ok(())
}

fn check_manifest(value: &Value) -> Result<(), Stop> {
    let manifest = &value["manifest"];
    if manifest["authentication"] != "not_claimed"
        || manifest["authorization"] != "not_claimed"
        || manifest["publication"] != "not_claimed"
        || manifest["mount"] != "not_performed"
        || manifest["samples"] != "omitted"
        || manifest["row_security_reproven"] != false
    {
        return Err(Stop::new("role-pack", "manifest status"));
    }
    Ok(())
}

fn column_field<'a>(artifact: &'a Value, dataset: &str, column: &str, field: &str) -> Result<&'a Value, Stop> {
    let found = draft_column(artifact, dataset, column)?;
    Ok(&found[field])
}

fn draft_column<'a>(artifact: &'a Value, dataset: &str, column: &str) -> Result<&'a Value, Stop> {
    let datasets = artifact["draft"]["effective"]["datasets"]
        .as_array()
        .ok_or_else(|| Stop::new("review-pg-draft", "datasets"))?;
    let table = datasets
        .iter()
        .find(|item| item["dataset"] == dataset)
        .ok_or_else(|| Stop::new("review-pg-draft", dataset))?;
    table["columns"]
        .as_array()
        .ok_or_else(|| Stop::new("review-pg-draft", "columns"))?
        .iter()
        .find(|item| item["name"] == column)
        .ok_or_else(|| Stop::new("review-pg-draft", column))
}

fn text_field(value: &Value, key: &str, step: &str) -> Result<String, Stop> {
    value[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Stop::new(step, key))
}

fn refuse_marker(step: &str, bytes: &[u8]) -> Result<(), Stop> {
    if contains_bytes(bytes, fixture::MARKER) {
        Err(Stop::new(step, "comment body was copied"))
    } else {
        Ok(())
    }
}

pub fn assert_draft_changed(docs: &Docs) -> Result<(), Stop> {
    if docs.captured_draft_hash.len() != 64
        || docs.reviewed_draft_hash.len() != 64
        || docs.captured_draft_hash == docs.reviewed_draft_hash
    {
        return Err(Stop::new("review-pg-draft", "draft_hash did not change"));
    }
    if docs.schema_hash.len() != 64 || docs.decisions_sha256.len() != 64 || docs.sealed_source_sha256.len() != 64 {
        return Err(Stop::new("reviewed-effective-oos", "provenance hash"));
    }
    if !docs.clerk_has_fk {
        return Err(Stop::new("role-pack-clerk", "missing declared fk text"));
    }
    if docs.hidden_private != "classification.absent" {
        return Err(Stop::new("reviewed-effective-oos", "private_note reason"));
    }
    Ok(())
}
