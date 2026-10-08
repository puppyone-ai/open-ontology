//! Real shop fixture through the authenticated API. No stub identity and no publication.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use selfbuild_control_api::identity::fetch_current;
use selfbuild_control_api::tlsutil;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shop_fixture_workflow() {
    if std::env::var("OO_SELFBUILD_WORKFLOW").ok().as_deref() != Some("1") {
        return;
    }
    let phase = std::env::var("OO_SELFBUILD_PHASE").expect("phase");
    let config_path = std::env::var("OO_SELFBUILD_CONFIG").expect("config");
    let config: Value = serde_json::from_slice(&std::fs::read(&config_path).expect("config bytes")).expect("config json");
    let mut summary = json!({
        "phase": phase,
        "identity": "unaccepted_test_issuer",
        "customer_idp": false,
        "published": false,
        "compiled_pack": "not_published"
    });
    let result = if phase == "failure" {
        failure_phase(&config, &config_path, &mut summary).await
    } else {
        success_phase(&config, &config_path, &mut summary).await
    };
    let path = format!("/out/workflow-{phase}.json");
    let _ = std::fs::write(&path, serde_json::to_vec_pretty(&summary).unwrap_or_default());
    if let Err(message) = result {
        panic!("{phase} {message} summary={path}");
    }
}

async fn failure_phase(config: &Value, config_path: &str, summary: &mut Value) -> Result<(), String> {
    let server = start_server(config_path)?;
    let origin = identity_origin(config)?;
    let ca = std::fs::read(config["identity"]["ca_file"].as_str().unwrap_or("")).map_err(|err| err.to_string())?;
    let api_ca = api_trust(config)?;
    let bind = config["listen"]["bind"].as_str().unwrap_or("");
    let anon = https(bind, &api_ca, "GET", "/control/v1/builds/b_00000000000000000000000000000000", "", &[]).await?;
    summary["anon_status"] = json!(anon.0);
    summary["anon_code"] = json!(error_code(&anon.1));
    if anon.0 != 401 || error_code(&anon.1) != "unauthenticated" {
        return Err(format!("anon {}", anon.0));
    }
    let admin = session("admin")?;
    let reviewer = session("reviewer")?;
    let auditor = session("auditor")?;
    let admin_id = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let body = create_body(config, &admin_id.revision, "shopfailkey0000002");
    let reviewer_id = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let denied = post(bind, &api_ca, "/control/v1/builds", &reviewer, &reviewer_id.csrf_token, &body).await?;
    summary["reviewer_create"] = json!(denied.0);
    if denied.0 != 403 {
        return Err(format!("reviewer create {}", denied.0));
    }
    let mut cross = body.clone();
    cross["project_id"] = json!("beta");
    let admin_cross = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let crossed = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_cross.csrf_token, &cross).await?;
    summary["cross_project"] = json!(crossed.0);
    if crossed.0 != 403 {
        return Err(format!("cross project {}", crossed.0));
    }
    let mut wrong = body.clone();
    wrong["identity_revision"] = json!(admin_cross.revision + 1);
    let admin_rev = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    wrong["identity_revision"] = json!(admin_rev.revision + 1);
    let conflict = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_rev.csrf_token, &wrong).await?;
    summary["revision_conflict"] = json!(conflict.0);
    summary["revision_code"] = json!(error_code(&conflict.1));
    if conflict.0 != 409 || error_code(&conflict.1) != "revision.conflict" {
        return Err(format!("revision {}", conflict.0));
    }
    let admin_ok = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let mut created_body = create_body(config, &admin_ok.revision, "shopfailkey0000002");
    created_body["identity_revision"] = json!(admin_ok.revision);
    let created = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_ok.csrf_token, &created_body).await?;
    summary["create_status"] = json!(created.0);
    if created.0 != 202 {
        return Err(format!("create {} {}", created.0, error_code(&created.1)));
    }
    let build_id = created.1["build_id"].as_str().unwrap_or("").to_owned();
    summary["build_id"] = json!(build_id);
    let admin_replay = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let mut replay_body = created_body.clone();
    replay_body["identity_revision"] = json!(admin_replay.revision);
    let replay = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_replay.csrf_token, &replay_body).await?;
    summary["idempotency_replay"] = json!(replay.0);
    if replay.0 != 202 || replay.1["build_id"].as_str() != Some(build_id.as_str()) {
        return Err(format!("replay {}", replay.0));
    }
    let admin_other = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let mut other = replay_body.clone();
    other["identity_revision"] = json!(admin_other.revision);
    other["allowance_id"] = json!("shop_other");
    let idem = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_other.csrf_token, &other).await?;
    summary["allowance_binding"] = json!(idem.0);
    summary["allowance_code"] = json!(error_code(&idem.1));
    if idem.0 != 403 || error_code(&idem.1) != "forbidden" {
        return Err(format!("allowance binding {}", idem.0));
    }
    let auditor_id = fetch_current(&origin, &ca, &auditor).await.map_err(|_| "auditor current".to_owned())?;
    let decide = json!({
        "build_id": build_id,
        "choice": "defer",
        "decision_kind": "label_description",
        "candidate_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "identity_revision": auditor_id.revision,
        "expected_revision": 1,
        "idempotency_key": "auditordefer00001"
    });
    let auditor_write = post(
        bind,
        &api_ca,
        "/control/v1/reviews/shop.party.display_name/decision",
        &auditor,
        &auditor_id.csrf_token,
        &decide,
    )
    .await?;
    summary["auditor_write"] = json!(auditor_write.0);
    if auditor_write.0 != 403 {
        return Err(format!("auditor write {}", auditor_write.0));
    }
    let ctx = ClientCtx { bind, api_ca: &api_ca, cookie: &admin, origin: &origin, ca: &ca };
    let row = wait_build(&ctx, &build_id, "failed", 900).await?;
    summary["final_status"] = json!(row["status"]);
    summary["failure_code"] = json!(row["failure_code"]);
    summary["stages"] = row["stages"].clone();
    let s5 = stage_of(&row, "S5");
    summary["s5"] = json!(s5);
    if row["status"] != "failed" || s5 != "failed" {
        return Err(format!("expected S5 failure, status {}", row["status"]));
    }
    if row["published"] != false || row["compiled_pack"] != "not_published" {
        return Err("failure published".to_owned());
    }
    let admin_pack = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let packed = post(
        bind,
        &api_ca,
        &format!("/control/v1/builds/{build_id}/pack"),
        &admin,
        &admin_pack.csrf_token,
        &json!({"identity_revision": admin_pack.revision, "idempotency_key": "failpackkey000001"}),
    )
    .await?;
    summary["pack_after_failure"] = json!(packed.0);
    summary["pack_after_failure_code"] = json!(error_code(&packed.1));
    if packed.0 == 200 {
        return Err("failure compiled a pack".to_owned());
    }
    drop(server);
    Ok(())
}

async fn success_phase(config: &Value, config_path: &str, summary: &mut Value) -> Result<(), String> {
    requeue_timed_out_generate(config_path, summary).await?;
    let server = start_server(config_path)?;
    let origin = identity_origin(config)?;
    let ca = std::fs::read(config["identity"]["ca_file"].as_str().unwrap_or("")).map_err(|err| err.to_string())?;
    let api_ca = api_trust(config)?;
    let bind = config["listen"]["bind"].as_str().unwrap_or("");
    let admin = session("admin")?;
    let reviewer = session("reviewer")?;
    let owner = session("owner")?;
    let admin_id = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let created_body = create_body(config, &admin_id.revision, "shopsuccesskey0001");
    let created = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_id.csrf_token, &created_body).await?;
    summary["create_status"] = json!(created.0);
    if created.0 != 202 {
        return Err(format!("create {} {}", created.0, error_code(&created.1)));
    }
    let build_id = created.1["build_id"].as_str().unwrap_or("").to_owned();
    summary["build_id"] = json!(build_id);
    let ctx = ClientCtx { bind, api_ca: &api_ca, cookie: &admin, origin: &origin, ca: &ca };
    // S9 and S10 are inserted awaiting, so completion is S8 succeeded.
    let row = wait_stage(&ctx, &build_id, "S8", "succeeded", 900).await?;
    summary["status_before_review"] = json!(row["status"]);
    summary["stages"] = row["stages"].clone();
    if stage_of(&row, "S5") != "succeeded" || stage_of(&row, "S8") != "succeeded" {
        return Err("S5 or S8 did not succeed".to_owned());
    }
    let s9 = stage_of(&row, "S9");
    let s10 = stage_of(&row, "S10");
    summary["s9_before_review"] = json!(s9);
    summary["s10_before_review"] = json!(s10);
    let fresh = s9 == "awaiting" && s10 == "awaiting";
    let resumed = (s9 == "awaiting" || s9 == "succeeded") && s10 == "succeeded";
    if !fresh && !resumed {
        return Err(format!("S9 or S10 was not left awaiting ({s9}/{s10})"));
    }
    note_generation(config, &build_id, summary)?;
    replay_cache(config, &build_id, summary)?;
    let reviews = get_text(
        bind,
        &api_ca,
        &admin,
        &origin,
        &ca,
        &format!("/control/v1/reviews?project_id=shop&build_id={build_id}"),
    )
    .await?;
    let suggestions = get_text(
        bind,
        &api_ca,
        &admin,
        &origin,
        &ca,
        &format!("/control/v1/builds/{build_id}/candidates"),
    )
    .await?;
    let suggestion_text = suggestions.1.to_string();
    let effects = &suggestions.1["suggestions"]["effects"];
    summary["suggestions_certified_true"] = json!(suggestion_text.contains("\"certified\":true"));
    summary["suggestions_auto_merge"] = json!(effects.get("auto_merge"));
    summary["suggestions_published"] = json!(effects.get("published"));
    summary["suggestions_metrics_certified"] = json!(effects.get("metrics_certified"));
    if suggestion_text.contains("\"certified\":true")
        || effects["auto_merge"] != false
        || effects["published"] != false
        || effects["metrics_certified"] != false
    {
        return Err("suggestions were merged or certified".to_owned());
    }
    let display = candidate_of(&reviews.1, "shop.party.display_name")?;
    let memo = candidate_of(&reviews.1, "shop.entry.memo")?;
    let reviewer_id = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let reviewer_projects: Vec<Value> = reviewer_id
        .projects
        .iter()
        .map(|project| {
            json!({
                "project_id": project.project_id,
                "role": project.role,
                "capabilities": project.capabilities
            })
        })
        .collect();
    summary["reviewer_projects"] = json!(reviewer_projects);
    let approve = json!({
        "build_id": build_id,
        "choice": "approve",
        "decision_kind": "label_description",
        "candidate_sha256": display,
        "identity_revision": reviewer_id.revision,
        "expected_revision": 1,
        "idempotency_key": "shopapprovedisplay1",
        "fields": {
            "label": {"overwrite": false},
            "description": {"overwrite": false}
        }
    });
    let approved = post(
        bind,
        &api_ca,
        "/control/v1/reviews/shop.party.display_name/decision",
        &reviewer,
        &reviewer_id.csrf_token,
        &approve,
    )
    .await?;
    summary["approve_status"] = json!(approved.0);
    summary["approve_code"] = json!(approved.1.get("code").cloned().unwrap_or(Value::Null));
    summary["approve_ledger_authenticated"] = json!(approved.1.get("ledger_authenticated"));
    if approved.0 != 200 || approved.1["ledger_authenticated"] != false || approved.1["grants"] != "not_authorized" {
        return Err(format!("approve {} {}", approved.0, error_code(&approved.1)));
    }
    let reviewer_same = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let mut approve_again = approve.clone();
    approve_again["identity_revision"] = json!(reviewer_same.revision);
    let decision_replay = post(
        bind,
        &api_ca,
        "/control/v1/reviews/shop.party.display_name/decision",
        &reviewer,
        &reviewer_same.csrf_token,
        &approve_again,
    )
    .await?;
    summary["decision_replay"] = json!(decision_replay.0);
    if decision_replay.0 != 200 {
        return Err(format!("decision replay {}", decision_replay.0));
    }
    let reviewer_conflict = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let mut changed = approve_again.clone();
    changed["identity_revision"] = json!(reviewer_conflict.revision);
    changed["choice"] = json!("defer");
    if let Some(object) = changed.as_object_mut() {
        let _ = object.remove("fields");
    }
    let decision_conflict = post(
        bind,
        &api_ca,
        "/control/v1/reviews/shop.party.display_name/decision",
        &reviewer,
        &reviewer_conflict.csrf_token,
        &changed,
    )
    .await?;
    summary["idempotency_conflict"] = json!(decision_conflict.0);
    summary["idempotency_code"] = json!(error_code(&decision_conflict.1));
    if decision_conflict.0 != 409 || error_code(&decision_conflict.1) != "idempotency.conflict" {
        return Err(format!("idempotency {}", decision_conflict.0));
    }
    let reviewer_defer = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let defer = json!({
        "build_id": build_id,
        "choice": "defer",
        "decision_kind": "label_description",
        "candidate_sha256": memo,
        "identity_revision": reviewer_defer.revision,
        "expected_revision": 1,
        "idempotency_key": "shopdefermemo00001"
    });
    let deferred = post(
        bind,
        &api_ca,
        "/control/v1/reviews/shop.entry.memo/decision",
        &reviewer,
        &reviewer_defer.csrf_token,
        &defer,
    )
    .await?;
    summary["defer_status"] = json!(deferred.0);
    if deferred.0 != 200 || deferred.1["ledger_authenticated"] != false {
        return Err(format!("defer {} {}", deferred.0, error_code(&deferred.1)));
    }
    let reviewer_grant = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let grant = class_body(config, &build_id, reviewer_grant.revision, "reviewgrantkey0001");
    let grant_denied = post(
        bind,
        &api_ca,
        "/control/v1/classifications/confirm",
        &reviewer,
        &reviewer_grant.csrf_token,
        &grant,
    )
    .await?;
    summary["reviewer_grant"] = json!(grant_denied.0);
    if grant_denied.0 != 403 {
        return Err(format!("reviewer grant {}", grant_denied.0));
    }
    let reviewer_pack = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let pack_denied = post(
        bind,
        &api_ca,
        &format!("/control/v1/builds/{build_id}/pack"),
        &reviewer,
        &reviewer_pack.csrf_token,
        &json!({"identity_revision": reviewer_pack.revision, "idempotency_key": "reviewerpack000001"}),
    )
    .await?;
    summary["reviewer_pack"] = json!(pack_denied.0);
    if pack_denied.0 != 403 {
        return Err(format!("reviewer pack {}", pack_denied.0));
    }
    let owner_bad = fetch_current(&origin, &ca, &owner).await.map_err(|_| "owner current".to_owned())?;
    let mut hidden = class_body(config, &build_id, owner_bad.revision, "hiddenpiikey000001");
    hidden["items"] = json!([{"object_id": "shop.party.phone", "pii": "sensitive", "semantic_type": "phone"}]);
    let hidden_denied = post(
        bind,
        &api_ca,
        "/control/v1/classifications/confirm",
        &owner,
        &owner_bad.csrf_token,
        &hidden,
    )
    .await?;
    summary["hidden_pii"] = json!(hidden_denied.0);
    summary["hidden_pii_code"] = json!(error_code(&hidden_denied.1));
    if hidden_denied.0 != 409 || error_code(&hidden_denied.1) != "membership.refused" {
        return Err(format!("hidden pii {}", hidden_denied.0));
    }
    let owner_unknown = fetch_current(&origin, &ca, &owner).await.map_err(|_| "owner current".to_owned())?;
    let mut unknown = class_body(config, &build_id, owner_unknown.revision, "unknownpiikey00001");
    if let Some(items) = unknown["items"].as_array_mut()
        && let Some(first) = items.first_mut()
    {
        first["pii"] = json!("unknown");
    }
    let unknown_denied = post(
        bind,
        &api_ca,
        "/control/v1/classifications/confirm",
        &owner,
        &owner_unknown.csrf_token,
        &unknown,
    )
    .await?;
    summary["unknown_pii"] = json!(unknown_denied.0);
    if unknown_denied.0 != 409 || error_code(&unknown_denied.1) != "pii.unknown" {
        return Err(format!("unknown pii {} {}", unknown_denied.0, error_code(&unknown_denied.1)));
    }
    let owner_all = fetch_current(&origin, &ca, &owner).await.map_err(|_| "owner current".to_owned())?;
    let mut accept_all = class_body(config, &build_id, owner_all.revision, "acceptallkey000001");
    accept_all["accept_all"] = json!(true);
    let accept_denied = post(
        bind,
        &api_ca,
        "/control/v1/classifications/preview",
        &owner,
        &owner_all.csrf_token,
        &accept_all,
    )
    .await?;
    summary["accept_all"] = json!(accept_denied.0);
    if accept_denied.0 != 409 || error_code(&accept_denied.1) != "batch.accept_all" {
        return Err(format!("accept all {}", accept_denied.0));
    }
    let owner_ok = fetch_current(&origin, &ca, &owner).await.map_err(|_| "owner current".to_owned())?;
    let confirmed_body = class_body(config, &build_id, owner_ok.revision, "shopclasskey000001");
    let preview = post(
        bind,
        &api_ca,
        "/control/v1/classifications/preview",
        &owner,
        &owner_ok.csrf_token,
        &confirmed_body,
    )
    .await?;
    summary["preview_wrote"] = json!(preview.1.get("wrote"));
    if preview.0 != 200 || preview.1["wrote"] != false {
        return Err(format!("preview {}", preview.0));
    }
    let owner_confirm = fetch_current(&origin, &ca, &owner).await.map_err(|_| "owner current".to_owned())?;
    let mut confirm = confirmed_body.clone();
    confirm["identity_revision"] = json!(owner_confirm.revision);
    let confirmed = post(
        bind,
        &api_ca,
        "/control/v1/classifications/confirm",
        &owner,
        &owner_confirm.csrf_token,
        &confirm,
    )
    .await?;
    summary["confirm_status"] = json!(confirmed.0);
    summary["confirm_grants"] = json!(confirmed.1.get("grants"));
    summary["confirm_ledger_authenticated"] = json!(confirmed.1.get("ledger_authenticated"));
    if confirmed.0 != 200 || confirmed.1["ledger_authenticated"] != false || confirmed.1["grants"] != "not_authorized" {
        return Err(format!("confirm {} {}", confirmed.0, error_code(&confirmed.1)));
    }
    let admin_pack = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let packed = post(
        bind,
        &api_ca,
        &format!("/control/v1/builds/{build_id}/pack"),
        &admin,
        &admin_pack.csrf_token,
        &json!({"identity_revision": admin_pack.revision, "idempotency_key": "shoppackkey0000001"}),
    )
    .await?;
    summary["pack_status"] = json!(packed.0);
    if packed.0 != 200 {
        return Err(format!("pack {} {}", packed.0, error_code(&packed.1)));
    }
    note_pack(&packed.1, summary)?;
    drop(server);
    let restarted = start_server(config_path)?;
    let again = get_text(
        bind,
        &api_ca,
        &admin,
        &origin,
        &ca,
        &format!("/control/v1/builds/{build_id}/pack"),
    )
    .await?;
    summary["restart_pack_status"] = json!(again.0);
    if again.0 != 200 || again.1["published"] != false || again.1["output_sha256"] != packed.1["output_sha256"] {
        return Err(format!("restart pack {}", again.0));
    }
    let admin_cancel_id = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let cancel_body = create_body(config, &admin_cancel_id.revision, "shopcancelkey000001");
    let cancel_created = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_cancel_id.csrf_token, &cancel_body).await?;
    if cancel_created.0 != 202 {
        return Err(format!("cancel create {}", cancel_created.0));
    }
    let cancel_id = cancel_created.1["build_id"].as_str().unwrap_or("").to_owned();
    let running = wait_build(&ctx, &cancel_id, "running", 90).await?;
    summary["cancel_saw_running"] = json!(running["status"] == "running");
    let admin_stop = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let stopped = post(
        bind,
        &api_ca,
        &format!("/control/v1/builds/{cancel_id}/cancel"),
        &admin,
        &admin_stop.csrf_token,
        &json!({"identity_revision": admin_stop.revision}),
    )
    .await?;
    summary["cancel_http"] = json!(stopped.0);
    if stopped.0 != 202 {
        return Err(format!("cancel http {}", stopped.0));
    }
    let canceled = wait_build(&ctx, &cancel_id, "canceled", 120).await?;
    summary["cancel_status"] = json!(canceled["status"]);
    if canceled["status"] != "canceled" || canceled["published"] != false {
        return Err(format!("cancel status {}", canceled["status"]));
    }
    let cancel_pack = get_text(
        bind,
        &api_ca,
        &admin,
        &origin,
        &ca,
        &format!("/control/v1/builds/{cancel_id}/pack"),
    )
    .await?;
    summary["cancel_pack_status"] = json!(cancel_pack.0);
    if cancel_pack.0 == 200 {
        return Err("canceled build published a pack".to_owned());
    }
    drop(restarted);
    Ok(())
}

async fn requeue_timed_out_generate(config_path: &str, summary: &mut Value) -> Result<(), String> {
    let app = selfbuild_control_api::config::load_config_file(config_path).map_err(|err| err.code.to_owned())?;
    let mut client = selfbuild_control_api::store::connect(&app.postgres).await.map_err(|err| err.code.to_owned())?;
    let tx = client.transaction().await.map_err(|err| err.to_string())?;
    let stage = tx
        .execute(
            "UPDATE oo_selfbuild_api.stage_checkpoint AS s
             SET status = 'pending', detail_code = NULL
             FROM oo_selfbuild_api.build_job AS j
             WHERE s.build_id = j.build_id
               AND j.project_id = 'shop'
               AND j.idempotency_key = 'shopsuccesskey0001'
               AND j.status = 'failed'
               AND j.failure_code = 'stage.failed'
               AND s.stage = 'S5'
               AND s.status = 'failed'
               AND s.detail_code = 'generate.failed'",
            &[],
        )
        .await
        .map_err(|err| err.to_string())?;
    let job = tx
        .execute(
            "UPDATE oo_selfbuild_api.build_job
             SET status = 'queued', stage = 'S5', failure_code = NULL,
                 lease_owner = NULL, lease_until = NULL, child_pid = NULL, child_exited = false,
                 updated_at = now()
             WHERE project_id = 'shop'
               AND idempotency_key = 'shopsuccesskey0001'
               AND status = 'failed'
               AND failure_code = 'stage.failed'
               AND stage = 'S5'",
            &[],
        )
        .await
        .map_err(|err| err.to_string())?;
    tx.commit().await.map_err(|err| err.to_string())?;
    summary["requeued_stage"] = json!(stage);
    summary["requeued_job"] = json!(job);
    Ok(())
}

fn note_pack(document: &Value, summary: &mut Value) -> Result<(), String> {
    let producer = &document["producer"];
    let packs = producer.pointer("/product/packs").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let text = producer.to_string();
    summary["pack_count"] = json!(packs);
    summary["producer_authenticated"] = json!(producer.get("authenticated"));
    summary["model_publication"] = json!(producer.pointer("/statements/model_publication"));
    summary["envelope_published"] = json!(document.get("published"));
    summary["field_candidates_applied"] = json!(producer.pointer("/statements/field_candidates_applied"));
    summary["reseal_count"] = json!(producer.get("reseal_count"));
    summary["unreviewed_prefix"] = json!(text.contains("UNREVIEWED"));
    summary["source_declared_not_grant"] = json!(text.contains("not a grant"));
    let roles: Vec<&str> = producer
        .pointer("/product/packs")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|row| row.get("role").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    summary["pack_roles"] = json!(roles);
    if document["published"] != false || document["compiled_pack"] != "not_published" || document["producer_authenticated"] != false {
        return Err("pack envelope claims publication".to_owned());
    }
    if packs != 2 || producer["authenticated"] != false {
        return Err(format!("pack shape {packs}"));
    }
    if roles != ["oo_r_analyst", "oo_r_clerk"] && roles != ["oo_r_clerk", "oo_r_analyst"] {
        return Err(format!("pack roles {roles:?}"));
    }
    // Declared foreign keys stay source-declared. Empty descriptions do not get a forged banner.
    // Stored label decisions stay unapplied unless every compiler column has a real decision.
    if producer.pointer("/statements/field_candidates_applied") != Some(&Value::Bool(false)) {
        return Err("pack applied undecided field candidates".to_owned());
    }
    if !text.contains("not a grant") {
        return Err("pack dropped the source-declared not-a-grant mark".to_owned());
    }
    if text.contains("\"state\":\"approved\"") || text.contains("\"state\": \"approved\"") {
        return Err("pack forged review.state approved".to_owned());
    }
    Ok(())
}

fn note_generation(config: &Value, build_id: &str, summary: &mut Value) -> Result<(), String> {
    let work = config["work_dir"].as_str().unwrap_or("");
    let bytes = std::fs::read(format!("{work}/builds/{build_id}/generation.json")).map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    summary["generation_outcome"] = json!(value.get("outcome"));
    summary["generation_cache"] = json!(value.get("cache"));
    let mut misses = 0u32;
    let mut hits = 0u32;
    let mut prompt = 0i64;
    let mut completion = 0i64;
    let mut cost_status = Vec::new();
    walk_generation(&value, &mut misses, &mut hits, &mut prompt, &mut completion, &mut cost_status);
    summary["model_misses"] = json!(misses);
    summary["model_hits"] = json!(hits);
    summary["prompt_tokens"] = json!(prompt);
    summary["completion_tokens"] = json!(completion);
    summary["declared_cost_status"] = json!(cost_status);
    if value["outcome"] != "validated" {
        return Err(format!("generation {}", value["outcome"]));
    }
    Ok(())
}

fn walk_generation(value: &Value, misses: &mut u32, hits: &mut u32, prompt: &mut i64, completion: &mut i64, cost: &mut Vec<String>) {
    if let Some(cache) = value.get("cache").and_then(Value::as_str) {
        if cache == "miss" {
            *misses += 1;
        }
        if cache == "hit" {
            *hits += 1;
        }
    }
    if let Some(status) = value.pointer("/transport/declared_cost/status").and_then(Value::as_str) {
        cost.push(status.to_owned());
    }
    if let Some(tokens) = value.pointer("/transport/usage/prompt_tokens").and_then(Value::as_i64) {
        *prompt += tokens;
    }
    if let Some(tokens) = value.pointer("/transport/usage/completion_tokens").and_then(Value::as_i64) {
        *completion += tokens;
    }
    if let Some(calls) = value.get("calls").and_then(Value::as_array) {
        for call in calls {
            walk_generation(call, misses, hits, prompt, completion, cost);
        }
    }
}

fn replay_cache(config: &Value, build_id: &str, summary: &mut Value) -> Result<(), String> {
    let work = config["work_dir"].as_str().unwrap_or("");
    let dir = format!("{work}/builds/{build_id}");
    let binary = config["binaries"]["pg_builder"].as_str().unwrap_or("");
    let output = Command::new(binary)
        .args([
            "generate",
            "--config",
            &format!("{dir}/pg-builder.config.json"),
            "--seal",
            &format!("{dir}/seal.json"),
            "--cards",
            &format!("{dir}/cards.json"),
            "--approval",
            &format!("{dir}/card-approval.json"),
            "--cache",
            &format!("{dir}/cache"),
            "--out",
            &format!("{dir}/generation-replay.json"),
        ])
        .env_clear()
        .output()
        .map_err(|err| err.to_string())?;
    summary["cache_replay_exit"] = json!(output.status.code());
    if !output.status.success() {
        return Err(format!("cache replay {}", output.status));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(format!("{dir}/generation-replay.json")).map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())?;
    let mut misses = 0u32;
    let mut hits = 0u32;
    let mut prompt = 0i64;
    let mut completion = 0i64;
    let mut cost = Vec::new();
    walk_generation(&value, &mut misses, &mut hits, &mut prompt, &mut completion, &mut cost);
    summary["cache_replay_hits"] = json!(hits);
    summary["cache_replay_misses"] = json!(misses);
    if hits == 0 || misses != 0 {
        return Err(format!("cache replay hits {hits} misses {misses}"));
    }
    Ok(())
}

fn class_body(config: &Value, build_id: &str, revision: i64, key: &str) -> Value {
    let columns = config["projects"][0]["visible_columns"].as_array().cloned().unwrap_or_default();
    let items: Vec<Value> = columns
        .iter()
        .filter_map(|item| item.as_str())
        .map(|object_id| {
            let (pii, semantic) = class_of(object_id);
            json!({"object_id": object_id, "pii": pii, "semantic_type": semantic})
        })
        .collect();
    json!({
        "project_id": "shop",
        "build_id": build_id,
        "identity_revision": revision,
        "idempotency_key": key,
        "items": items
    })
}

fn class_of(object_id: &str) -> (&'static str, &'static str) {
    match object_id {
        "shop.party.display_name" => ("personal", "name"),
        "shop.party.active" | "shop.account.opened" => ("none", "flag"),
        "shop.account.label" => ("none", "label"),
        "shop.entry.qty" => ("none", "quantity"),
        "shop.entry.amount" => ("none", "amount"),
        "shop.entry.memo" => ("none", "text"),
        _ => ("none", "identifier"),
    }
}

fn candidate_of(document: &Value, object_id: &str) -> Result<String, String> {
    let items = document["items"].as_array().ok_or("reviews have no items")?;
    let item = items
        .iter()
        .find(|item| item["item_id"] == object_id)
        .ok_or_else(|| format!("missing {object_id}"))?;
    item["candidate_sha256"].as_str().map(str::to_owned).ok_or_else(|| "candidate".to_owned())
}

fn create_body(config: &Value, revision: &i64, key: &str) -> Value {
    let project = &config["projects"][0];
    json!({
        "project_id": project["project_id"],
        "identity_revision": revision,
        "config_commit": project["config_commit"],
        "source_ref": project["source_ref"],
        "allowance_id": project["allowance_id"],
        "llm_secret_ref": project["llm_secret_ref"],
        "idempotency_key": key
    })
}

fn stage_of(document: &Value, stage: &str) -> String {
    document["stages"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["stage"] == stage))
        .and_then(|row| row["status"].as_str())
        .unwrap_or("")
        .to_owned()
}

struct ClientCtx<'a> {
    bind: &'a str,
    api_ca: &'a [u8],
    cookie: &'a str,
    origin: &'a str,
    ca: &'a [u8],
}

async fn wait_build(ctx: &ClientCtx<'_>, build_id: &str, status: &str, seconds: u64) -> Result<Value, String> {
    let mut last = Value::Null;
    for _ in 0..seconds {
        let row = get_text(
            ctx.bind,
            ctx.api_ca,
            ctx.cookie,
            ctx.origin,
            ctx.ca,
            &format!("/control/v1/builds/{build_id}"),
        )
        .await?;
        last = row.1;
        if last["status"] == status || last["status"] == "failed" || last["status"] == "canceled" {
            return Ok(last);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(format!("timeout waiting for {status}: {}", last["status"]))
}

async fn wait_stage(
    ctx: &ClientCtx<'_>,
    build_id: &str,
    stage: &str,
    status: &str,
    seconds: u64,
) -> Result<Value, String> {
    let mut last = Value::Null;
    for _ in 0..seconds {
        let row = get_text(
            ctx.bind,
            ctx.api_ca,
            ctx.cookie,
            ctx.origin,
            ctx.ca,
            &format!("/control/v1/builds/{build_id}"),
        )
        .await?;
        last = row.1;
        if stage_of(&last, stage) == status {
            return Ok(last);
        }
        if last["status"] == "failed" || last["status"] == "canceled" {
            return Err(format!("{stage} stopped at {} {}", last["status"], last["failure_code"]));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(format!("timeout waiting for {stage} {status}: {}", last["status"]))
}

async fn get_text(
    bind: &str,
    api_ca: &[u8],
    cookie: &str,
    origin: &str,
    ca: &[u8],
    path: &str,
) -> Result<(u16, Value), String> {
    let _ = (origin, ca);
    let cookie_header = format!("__Host-oo-session={cookie}");
    https(bind, api_ca, "GET", path, &cookie_header, &[]).await
}

async fn post(bind: &str, api_ca: &[u8], path: &str, cookie: &str, csrf: &str, body: &Value) -> Result<(u16, Value), String> {
    let bytes = serde_json::to_vec(body).map_err(|err| err.to_string())?;
    let cookie_header = format!("__Host-oo-session={cookie}");
    let mut request_headers = format!(
        "content-type: application/json\r\nx-oo-csrf: {csrf}\r\norigin: {}\r\n",
        std::env::var("OO_SELFBUILD_IDENTITY_ORIGIN").map_err(|err| err.to_string())?
    );
    request_headers.push_str(&format!("cookie: {cookie_header}\r\n"));
    https_raw(bind, api_ca, "POST", path, &request_headers, &bytes).await
}

async fn https(bind: &str, ca: &[u8], method: &str, path: &str, cookie_header: &str, body: &[u8]) -> Result<(u16, Value), String> {
    let headers = if cookie_header.is_empty() {
        String::new()
    } else {
        format!("cookie: {cookie_header}\r\n")
    };
    https_raw(bind, ca, method, path, &headers, body).await
}

async fn https_raw(bind: &str, ca: &[u8], method: &str, path: &str, headers: &str, body: &[u8]) -> Result<(u16, Value), String> {
    let config = tlsutil::client_config(ca).map_err(|err| err.to_string())?;
    let connector = TlsConnector::from(Arc::new(config));
    let stream = TcpStream::connect(bind).await.map_err(|err| err.to_string())?;
    let name = ServerName::try_from("127.0.0.1".to_owned()).map_err(|_| "server name".to_owned())?;
    let mut tls = connector.connect(name, stream).await.map_err(|err| err.to_string())?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nhost: {bind}\r\nconnection: close\r\ncontent-length: {}\r\n{headers}\r\n",
        body.len()
    );
    tls.write_all(request.as_bytes()).await.map_err(|err| err.to_string())?;
    if !body.is_empty() {
        tls.write_all(body).await.map_err(|err| err.to_string())?;
    }
    let mut bytes = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(120), tls.read_to_end(&mut bytes)).await;
    parse_http(&bytes)
}

fn parse_http(bytes: &[u8]) -> Result<(u16, Value), String> {
    let text = String::from_utf8_lossy(bytes);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("short http")?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or("status")?;
    let value = serde_json::from_str(body).unwrap_or(Value::Null);
    Ok((status, value))
}

fn clip_text(value: &str) -> String {
    let mut text = value.replace('\n', " ");
    if text.len() > 240 {
        text.truncate(240);
    }
    text
}

fn error_code(value: &Value) -> String {
    value.pointer("/error/code").and_then(Value::as_str).unwrap_or("").to_owned()
}

fn api_trust(config: &Value) -> Result<Vec<u8>, String> {
    let path = std::env::var("OO_SELFBUILD_API_CA").unwrap_or_else(|_| {
        config["listen"]["cert_file"].as_str().unwrap_or("").to_owned()
    });
    std::fs::read(path).map_err(|err| err.to_string())
}

fn identity_origin(config: &Value) -> Result<String, String> {
    config["identity"]["origin"].as_str().map(str::to_owned).ok_or_else(|| "origin".to_owned())
}

fn session(name: &str) -> Result<String, String> {
    let path = std::env::var(format!("OO_SELFBUILD_SESSION_{}", name.to_ascii_uppercase())).map_err(|err| err.to_string())?;
    let mut text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    while text.ends_with(['\n', '\r']) {
        text.pop();
    }
    if text.len() != 43 {
        return Err(format!("{name} session"));
    }
    Ok(text)
}

struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server(config_path: &str) -> Result<Server, String> {
    let bin = env!("CARGO_BIN_EXE_selfbuild-control-api");
    let install = Command::new(bin)
        .args(["install", "--config", config_path])
        .env_clear()
        .output()
        .map_err(|err| err.to_string())?;
    if !install.status.success() {
        let stdout = String::from_utf8_lossy(&install.stdout);
        let stderr = String::from_utf8_lossy(&install.stderr);
        return Err(format!(
            "install {} {} {}",
            install.status,
            clip_text(&stdout),
            clip_text(&stderr)
        ));
    }
    let mut child = Command::new(bin)
        .args(["serve", "--config", config_path])
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    let mut stdout = child.stdout.take().ok_or("stdout")?;
    let ready = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while stdout.read(&mut byte).ok() == Some(1) {
            buf.push(byte[0]);
            if buf.windows(5).any(|window| window == b"ready") || buf.len() > 4096 {
                break;
            }
        }
    });
    let _ = ready.join();
    std::thread::sleep(Duration::from_millis(200));
    let _ = child.try_wait();
    Ok(Server(child))
}
