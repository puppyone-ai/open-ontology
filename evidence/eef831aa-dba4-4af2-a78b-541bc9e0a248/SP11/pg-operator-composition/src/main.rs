//! One private PostgreSQL composition. This binary records what the fixed
//! compilers emitted. It does not accept M1-I01, M1-I02, publication,
//! authentication, a WAL, natural language, or a harness mount.

#![forbid(unsafe_code)]

mod bridge;
mod docs;
mod fixture;
mod live;
mod pg;
mod policies;
mod proc;
mod query;
mod read;
mod sql;
mod stop;

use std::env;
use std::fs;
use std::process::ExitCode;

use serde_json::json;

use crate::docs::Docs;
use crate::fixture::DECISIONS;
use crate::live::{ensure_dir, Live, FIXED};
use crate::pg::Imported;
use crate::proc::write_json;
use crate::read::Reads;
use crate::stop::Stop;

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(stop) => {
            write_stop(&stop);
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<(), Stop> {
    let live = Live::load()?;
    let stale = live.evidence.join("stop.json");
    if stale.exists() {
        fs::remove_file(&stale).map_err(|err| crate::stop::disk("stop", &err))?;
    }
    ensure_dir(&live.evidence.join("steps"))?;
    ensure_dir(&live.evidence.join("bundles"))?;
    ensure_dir(&live.bundle_dir)?;
    pg::prepare(&live).await?;
    let imported = pg::load_target(&live).await?;
    let docs = docs::compile(&live)?;
    let emitted = policies::emit(&live, &docs, &imported)?;
    let reads = read::guard_and_read(&live, &imported, &emitted).await?;
    write_outcome(&live, &docs, &imported, &reads)?;
    fs::write(live.evidence.join("outcome.txt"), "reviewed_pg_operator_composition_recorded\n")
        .map_err(|err| crate::stop::disk("outcome", &err))?;
    Ok(())
}

fn write_outcome(live: &Live, docs: &Docs, imported: &Imported, reads: &Reads) -> Result<(), Stop> {
    let mut value = outcome_scope();
    extend_object(&mut value, outcome_compiler(docs, reads));
    extend_object(&mut value, outcome_packs(docs));
    extend_object(&mut value, outcome_import(imported));
    extend_object(&mut value, outcome_reads(reads));
    write_json(live, "outcome.json", &value)
}

fn extend_object(into: &mut serde_json::Value, extra: serde_json::Value) {
    let serde_json::Value::Object(src) = extra else {
        return;
    };
    let Some(dst) = into.as_object_mut() else {
        return;
    };
    dst.extend(src);
}

fn outcome_scope() -> serde_json::Value {
    json!({
        "fixed_commit": FIXED,
        "parent_acceptance": false,
        "development_unit": false,
        "component_approval": false,
        "m1_i01": "not_accepted",
        "m1_i02": "not_accepted",
        "semantic_publication": "missing",
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "wal": "missing",
        "natural_language": "missing",
        "harness_mount": "not_performed",
        "compiled_install_column_acl": "pasted_reviewed_policy_dto",
        "hand_grant_business_columns": false,
        "hand_classified_policies": false,
        "policy_labels": "reviewed_policy_dto",
        "role_login_binding": "configuration_not_auth",
        "login_mapping_grant": "provision_mapping_reader",
        "install_row_filter": "null_preserved",
        "query_row_filter": "null_key_omitted",
        "component_reviews_not_substituted": true
    })
}

fn outcome_compiler(docs: &Docs, reads: &Reads) -> serde_json::Value {
    json!({
        "mapping_privileges_before": reads.mapping_before,
        "mapping_privileges_after": reads.mapping_after,
        "decision_count": DECISIONS.len(),
        "schema_hash": docs.schema_hash,
        "catalog_sha256": docs.catalog_sha,
        "captured_draft_hash": docs.captured_draft_hash,
        "reviewed_draft_hash": docs.reviewed_draft_hash,
        "decisions_sha256": docs.decisions_sha256,
        "sealed_source_sha256": docs.sealed_source_sha256,
        "hidden_private_note": docs.hidden_private,
        "clerk_pack_digest": docs.clerk_digest,
        "peek_pack_digest": docs.peek_digest,
        "clerk_manifest_sha256": docs.clerk_manifest,
        "peek_manifest_sha256": docs.peek_manifest
    })
}

fn outcome_packs(docs: &Docs) -> serde_json::Value {
    json!({
        "clerk_bundle_sha256": docs.clerk_bundle_sha,
        "peek_bundle_sha256": docs.peek_bundle_sha,
        "clerk_pack_sha256": docs.clerk_pack_sha256,
        "peek_pack_sha256": docs.peek_pack_sha256,
        "second_bundle_exit": docs.second_exit,
        "second_bundle_io_destination": docs.second_destination,
        "clerk_has_declared_fk": docs.clerk_has_fk,
        "peek_has_declared_fk": docs.peek_has_fk
    })
}

fn outcome_import(imported: &Imported) -> serde_json::Value {
    json!({
        "init_install": imported.init_install,
        "init_full_import": imported.init_full_import,
        "historical_v2": imported.historical_v2,
        "import_state": "awaiting_publish",
        "import_run_id": imported.run_id,
        "source_generation": imported.source_generation,
        "target_generation": imported.target_generation,
        "publication_count": imported.publication,
        "runtime_owns_target": imported.owner_member
    })
}

fn outcome_reads(reads: &Reads) -> serde_json::Value {
    json!({
        "reltuples_before": reads.reltuples_before,
        "reltuples_after": reads.reltuples_after,
        "stats_published": reads.maintenance.published,
        "stats_estimate_is_exact": reads.maintenance.exact,
        "estimated_customer": reads.estimated_customer,
        "estimated_order": reads.estimated_order,
        "grantor_clerk": reads.grantor_clerk,
        "grantor_peek": reads.grantor_peek,
        "provision_clerk": reads.provision_clerk,
        "provision_peek": reads.provision_peek,
        "query_before_analyze_exit": reads.before_exit,
        "guard_hidden_exit": reads.guard_hidden,
        "guard_insert_exit": reads.guard_insert,
        "guard_order_by_pos_exit": reads.guard_pos,
        "catalog_pending_customer": reads.pending_customer,
        "catalog_private_note_absent": reads.private_absent,
        "direct_acl": reads.direct_states,
        "peek_display_matches_gold": reads.peek_display_ok
    })
}

fn write_stop(stop: &Stop) {
    let root = env::var("OO_A248_EVIDENCE").unwrap_or_else(|_| "/evidence/live".to_string());
    let body = format!(
        "{}\n",
        json!({"step": stop.step, "detail": stop.detail})
    );
    let _ = fs::write(format!("{root}/stop.json"), body);
}
