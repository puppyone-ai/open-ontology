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
        "identity": "a6f92da8a0bf872f8a30efb116c2c7b14463505b",
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
    let created_body = create_body(config, &admin_id.revision, "shopconvkey0000001");
    let created = post(bind, &api_ca, "/control/v1/builds", &admin, &admin_id.csrf_token, &created_body).await?;
    summary["create_status"] = json!(created.0);
    if created.0 != 202 {
        return Err(format!("create {} {}", created.0, error_code(&created.1)));
    }
    let build_id = created.1["build_id"].as_str().unwrap_or("").to_owned();
    summary["build_id"] = json!(build_id);
    let ctx = ClientCtx { bind, api_ca: &api_ca, cookie: &admin, origin: &origin, ca: &ca };
    // S9 and S10 are inserted awaiting, so completion is S8 succeeded.
    let mut row = wait_stage(&ctx, &build_id, "S8", "succeeded", 900).await?;
    summary["status_before_review"] = json!(row["status"]);
    summary["stages"] = row["stages"].clone();
    if stage_of(&row, "S5") != "succeeded" || stage_of(&row, "S8") != "succeeded" {
        return Err("S5 or S8 did not succeed".to_owned());
    }
    let s9 = stage_of(&row, "S9");
    let s10 = stage_of(&row, "S10");
    summary["s9_before_review"] = json!(s9);
    summary["s10_before_review"] = json!(s10);
    // A finished job keeps its decisions. Replaying them must not demand the awaiting window again.
    let already = row["status"] == "succeeded" && s9 == "succeeded" && s10 == "succeeded";
    summary["resumed_completed_job"] = json!(already);
    if !already {
        if s9 != "awaiting" || s10 != "awaiting" {
            return Err(format!("S9 or S10 was not left awaiting ({s9}/{s10})"));
        }
        for _ in 0..30 {
            if row["stage"] == "S9" {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            row = get_text(bind, &api_ca, &admin, &origin, &ca, &format!("/control/v1/builds/{build_id}")).await?.1;
        }
        summary["stage_before_review"] = json!(row["stage"]);
        if row["stage"] != "S9" || row["status"] != "running" {
            return Err(format!("job stayed {} {}", row["status"], row["stage"]));
        }
    }
    note_generation(config, &build_id, summary)?;
    note_prior_generation(summary)?;
    replay_cache(config, &build_id, summary)?;
    require_model_evidence(summary)?;
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
    let phone = candidate_of(&reviews.1, "shop.party.phone")?;
    let reviewer_hidden = fetch_current(&origin, &ca, &reviewer).await.map_err(|_| "reviewer current".to_owned())?;
    let hidden_approve = json!({
        "build_id": build_id,
        "choice": "approve",
        "decision_kind": "label_description",
        "candidate_sha256": phone,
        "identity_revision": reviewer_hidden.revision,
        "expected_revision": 1,
        "idempotency_key": "shopcvphoneapprove",
        "fields": {"label": {"overwrite": true}, "description": {"overwrite": true}}
    });
    let hidden_denied_label = post(
        bind,
        &api_ca,
        "/control/v1/reviews/shop.party.phone/decision",
        &reviewer,
        &reviewer_hidden.csrf_token,
        &hidden_approve,
    )
    .await?;
    summary["hidden_approve"] = json!(hidden_denied_label.0);
    summary["hidden_approve_code"] = json!(error_code(&hidden_denied_label.1));
    if hidden_denied_label.0 != 409 || error_code(&hidden_denied_label.1) != "membership.refused" {
        return Err(format!("hidden approve {}", hidden_denied_label.0));
    }
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
        "idempotency_key": "shopcvdisplayappr",
        "fields": {
            "label": {"overwrite": true},
            "description": {"overwrite": true}
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
        "idempotency_key": "shopcvmemodefer01"
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
    let env = HttpEnv { bind, api_ca: &api_ca, origin: &origin, ca: &ca };
    decide_remaining(&env, &reviewer, &build_id, &reviews.1).await?;
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
    let confirmed_body = class_body(config, &build_id, owner_ok.revision, "shopcvclass000001");
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
    let reviewed = get_text(bind, &api_ca, &admin, &origin, &ca, &format!("/control/v1/builds/{build_id}")).await?;
    summary["status_after_confirm"] = json!(reviewed.1["status"]);
    summary["s9_after_confirm"] = json!(stage_of(&reviewed.1, "S9"));
    summary["s10_after_confirm"] = json!(stage_of(&reviewed.1, "S10"));
    let reviewed_status = reviewed.1["status"].as_str().unwrap_or("");
    if (reviewed_status != "running" && reviewed_status != "succeeded")
        || stage_of(&reviewed.1, "S9") != "succeeded"
        || stage_of(&reviewed.1, "S10") != "succeeded"
    {
        return Err(format!(
            "review did not finish {} {} {}",
            reviewed.1["status"],
            stage_of(&reviewed.1, "S9"),
            stage_of(&reviewed.1, "S10")
        ));
    }
    let admin_pack = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let packed = post(
        bind,
        &api_ca,
        &format!("/control/v1/builds/{build_id}/pack"),
        &admin,
        &admin_pack.csrf_token,
        &json!({"identity_revision": admin_pack.revision, "idempotency_key": "shopcvpack0000001"}),
    )
    .await?;
    summary["pack_status"] = json!(packed.0);
    if packed.0 != 200 {
        return Err(format!("pack {} {}", packed.0, error_code(&packed.1)));
    }
    note_pack(config, &build_id, &packed.1, summary)?;
    let finished = get_text(bind, &api_ca, &admin, &origin, &ca, &format!("/control/v1/builds/{build_id}")).await?;
    summary["terminal_status"] = json!(finished.1["status"]);
    summary["terminal_stage"] = json!(finished.1["stage"]);
    if finished.1["status"] != "succeeded" || finished.1["stage"] != "S10" || finished.1["published"] != false || finished.1["compiled_pack"] != "not_published" {
        return Err(format!("terminal {} {}", finished.1["status"], finished.1["stage"]));
    }
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
    let env = HttpEnv { bind, api_ca: &api_ca, origin: &origin, ca: &ca };
    prove_cached_rebuild(config, &env, &admin, &reviewer, &owner, summary).await?;
    let kept = get_text(
        bind,
        &api_ca,
        &admin,
        &origin,
        &ca,
        "/control/v1/builds/b_f36a05605491fe53429435921539158c",
    )
    .await?;
    summary["prior_job_status"] = json!(kept.1["status"]);
    summary["prior_job_stage"] = json!(kept.1["stage"]);
    summary["prior_s9"] = json!(stage_of(&kept.1, "S9"));
    if kept.1["status"] != "running" || kept.1["stage"] != "S5" || stage_of(&kept.1, "S9") != "awaiting" {
        return Err(format!("prior job changed {} {}", kept.1["status"], kept.1["stage"]));
    }
    let admin_cancel_id = fetch_current(&origin, &ca, &admin).await.map_err(|_| "admin current".to_owned())?;
    let cancel_body = create_body(config, &admin_cancel_id.revision, "shopcancelkey000003");
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

fn note_pack(config: &Value, build_id: &str, document: &Value, summary: &mut Value) -> Result<(), String> {
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
    if producer.pointer("/statements/field_candidates_applied") != Some(&Value::Bool(true)) {
        return Err("field candidates were not applied".to_owned());
    }
    if producer.get("reseal_count").and_then(Value::as_i64).unwrap_or(0) < 1 {
        return Err("reseal count stayed zero".to_owned());
    }
    if !text.contains("not a grant") {
        return Err("pack dropped the source-declared not-a-grant mark".to_owned());
    }
    let work = config["work_dir"].as_str().unwrap_or("");
    let operator = std::fs::read_to_string(format!("{work}/builds/{build_id}/operator.json")).map_err(|err| err.to_string())?;
    summary["operator_has_review_state"] = json!(operator.contains("review.state"));
    summary["operator_has_column_decisions"] = json!(operator.contains("column_decisions"));
    if operator.contains("review.state") || !operator.contains("column_decisions") {
        return Err("operator document is missing column decisions or forged review state".to_owned());
    }
    let generation = read_generation(config, build_id)?;
    let approved_column = candidate_prose(&generation, "shop.party", Some("display_name"), "description")?;
    let approved_table = candidate_prose(&generation, "shop.party", None, "description")?;
    let rejected_label = candidate_prose(&generation, "shop.account", Some("label"), "description")?;
    let deferred_memo = candidate_prose(&generation, "shop.entry", Some("memo"), "description")?;
    let role_text = role_artifact_text(producer);
    let in_both = role_text.iter().all(|text| text.contains(&approved_column) && text.contains(&approved_table));
    let rejected_absent = role_text.iter().all(|text| !text.contains(&rejected_label) && !text.contains(&deferred_memo));
    summary["approved_column_len"] = json!(approved_column.len());
    summary["approved_table_len"] = json!(approved_table.len());
    summary["approved_in_both_roles"] = json!(in_both);
    summary["rejected_absent"] = json!(rejected_absent);
    if approved_column.len() < 12 || approved_table.len() < 12 || !in_both || !rejected_absent {
        return Err("approved text did not land in both packs".to_owned());
    }
    Ok(())
}

fn role_artifact_text(producer: &Value) -> Vec<String> {
    producer
        .pointer("/product/packs")
        .and_then(Value::as_array)
        .map(|packs| {
            packs
                .iter()
                .map(|pack| {
                    pack.pointer("/artifact/files")
                        .and_then(Value::as_array)
                        .map(|files| {
                            files
                                .iter()
                                .filter_map(|file| file.get("text").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn candidate_prose(generation: &Value, dataset: &str, column: Option<&str>, field: &str) -> Result<String, String> {
    let calls = generation["calls"].as_array().ok_or("generation has no calls")?;
    for call in calls {
        if call.get("dataset").and_then(Value::as_str) != Some(dataset) {
            continue;
        }
        let rows = match column {
            Some(_) => call.pointer("/candidates/field").and_then(Value::as_array),
            None => call.pointer("/candidates/table").and_then(Value::as_array),
        };
        let Some(rows) = rows else {
            continue;
        };
        for row in rows {
            if column.is_some() && row.get("column").and_then(Value::as_str) != column {
                continue;
            }
            if let Some(text) = row.get(field).and_then(Value::as_str)
                && !text.is_empty()
            {
                return Ok(text.to_owned());
            }
        }
    }
    Err(format!("missing candidate prose {dataset}"))
}

struct ModelCounts {
    misses: u32,
    hits: u32,
    prompt: i64,
    completion: i64,
    cost: Vec<String>,
    sockets_opened: u32,
    sockets_closed: u32,
}

fn note_generation(config: &Value, build_id: &str, summary: &mut Value) -> Result<(), String> {
    let value = read_generation(config, build_id)?;
    let counts = model_counts(&value);
    summary["generation_outcome"] = json!(value.get("outcome"));
    summary["generation_cache"] = json!(value.get("cache"));
    write_counts(summary, "", &counts);
    if value["outcome"] != "validated" {
        return Err(format!("generation {}", value["outcome"]));
    }
    Ok(())
}

fn note_prior_generation(summary: &mut Value) -> Result<(), String> {
    let path = "/out/work/builds/b_f36a05605491fe53429435921539158c/generation.json";
    let Ok(bytes) = std::fs::read(path) else {
        summary["prior_generation_present"] = json!(false);
        return Ok(());
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    let counts = model_counts(&value);
    summary["prior_generation_present"] = json!(true);
    summary["prior_generation_outcome"] = json!(value.get("outcome"));
    summary["prior_generation_cache"] = json!(value.get("cache"));
    write_counts(summary, "prior_", &counts);
    Ok(())
}

fn write_counts(summary: &mut Value, prefix: &str, counts: &ModelCounts) {
    let Some(object) = summary.as_object_mut() else {
        return;
    };
    object.insert(format!("{prefix}model_misses"), json!(counts.misses));
    object.insert(format!("{prefix}model_hits"), json!(counts.hits));
    object.insert(format!("{prefix}prompt_tokens"), json!(counts.prompt));
    object.insert(format!("{prefix}completion_tokens"), json!(counts.completion));
    object.insert(format!("{prefix}declared_cost_status"), json!(counts.cost));
    object.insert(format!("{prefix}sockets_opened"), json!(counts.sockets_opened));
    object.insert(format!("{prefix}sockets_closed"), json!(counts.sockets_closed));
}

fn require_model_evidence(summary: &Value) -> Result<(), String> {
    let misses = summary["model_misses"].as_u64().unwrap_or(0);
    let hits = summary["model_hits"].as_u64().unwrap_or(0);
    let replay_hits = summary["cache_replay_hits"].as_u64().unwrap_or(0);
    let replay_misses = summary["cache_replay_misses"].as_u64().unwrap_or(0);
    let prior_misses = summary["prior_model_misses"].as_u64().unwrap_or(0);
    let prior_hits = summary["prior_model_hits"].as_u64().unwrap_or(0);
    let fresh = misses >= 1 && replay_misses == 0 && replay_hits >= 1;
    let cached = prior_misses >= 1 && prior_hits >= 1 && misses == 0 && hits >= 1 && replay_misses == 0;
    if !fresh && !cached {
        return Err(format!("model evidence misses {misses} hits {hits} prior {prior_misses}/{prior_hits}"));
    }
    let mut costs = Vec::new();
    if let Some(rows) = summary["declared_cost_status"].as_array() {
        costs.extend(rows.iter().filter_map(|item| item.as_str()));
    }
    if let Some(rows) = summary["prior_declared_cost_status"].as_array() {
        costs.extend(rows.iter().filter_map(|item| item.as_str()));
    }
    if costs.is_empty() || costs.iter().any(|status| *status != "unknown") {
        return Err("model cost was not recorded as unknown".to_owned());
    }
    Ok(())
}

fn read_generation(config: &Value, build_id: &str) -> Result<Value, String> {
    let work = config["work_dir"].as_str().unwrap_or("");
    let bytes = std::fs::read(format!("{work}/builds/{build_id}/generation.json")).map_err(|err| err.to_string())?;
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
}

fn model_counts(value: &Value) -> ModelCounts {
    let mut counts = ModelCounts {
        misses: 0,
        hits: 0,
        prompt: 0,
        completion: 0,
        cost: Vec::new(),
        sockets_opened: 0,
        sockets_closed: 0,
    };
    walk_generation(value, &mut counts);
    counts
}

fn walk_generation(value: &Value, counts: &mut ModelCounts) {
    if value.get("dataset").is_some() {
        if let Some(cache) = value.get("cache").and_then(Value::as_str) {
            if cache == "miss" {
                counts.misses += 1;
            }
            if cache == "hit" {
                counts.hits += 1;
            }
        }
        if let Some(status) = value.pointer("/transport/declared_cost/status").and_then(Value::as_str) {
            counts.cost.push(status.to_owned());
        }
        if let Some(tokens) = value.pointer("/transport/usage/prompt_tokens").and_then(Value::as_i64) {
            counts.prompt += tokens;
        }
        if let Some(tokens) = value.pointer("/transport/usage/completion_tokens").and_then(Value::as_i64) {
            counts.completion += tokens;
        }
        if let Some(socket) = value.pointer("/transport/socket").and_then(Value::as_str) {
            if socket == "opened" {
                counts.sockets_opened += 1;
            }
            if socket == "not_opened" {
                counts.sockets_closed += 1;
            }
        }
    }
    if let Some(calls) = value.get("calls").and_then(Value::as_array) {
        for call in calls {
            walk_generation(call, counts);
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
    let counts = model_counts(&value);
    summary["cache_replay_hits"] = json!(counts.hits);
    summary["cache_replay_misses"] = json!(counts.misses);
    if counts.hits == 0 || counts.misses != 0 {
        return Err(format!("cache replay hits {} misses {}", counts.hits, counts.misses));
    }
    Ok(())
}

struct HttpEnv<'a> {
    bind: &'a str,
    api_ca: &'a [u8],
    origin: &'a str,
    ca: &'a [u8],
}

async fn decide_remaining(env: &HttpEnv<'_>, reviewer: &str, build_id: &str, reviews: &Value) -> Result<(), String> {
    post_scope(env, reviewer, build_id, reviews, "").await
}

async fn post_scope(env: &HttpEnv<'_>, reviewer: &str, build_id: &str, reviews: &Value, suffix: &str) -> Result<(), String> {
    let rows = [
        ("shop.party", "approve", "shopcvtableapprove", true),
        ("shop.account", "reject", "shopcvacctreject1", true),
        ("shop.entry", "reject", "shopcventryreject", true),
        ("shop.party.party_key", "reject", "shopcvpartykeyrej", false),
        ("shop.party.display_name", "approve", "shopcvdisplayappr", false),
        ("shop.party.active", "reject", "shopcvactiveject1", false),
        ("shop.party.phone", "reject", "shopcvphonereject", false),
        ("shop.party.national_id", "reject", "shopcvnatidreject", false),
        ("shop.account.account_id", "reject", "shopcvacctidrejec", false),
        ("shop.account.party_key", "reject", "shopcvacctpkrejec", false),
        ("shop.account.opened", "reject", "shopcvopenedrejec", false),
        ("shop.account.label", "reject", "shopcvlabelreject", false),
        ("shop.entry.entry_id", "reject", "shopcventryidreje", false),
        ("shop.entry.account_id", "reject", "shopcventryacreje", false),
        ("shop.entry.qty", "reject", "shopcvqtyreject01", false),
        ("shop.entry.amount", "reject", "shopcvamountrejec", false),
        ("shop.entry.memo", "defer", "shopcvmemodefer01", false),
    ];
    for (object_id, choice, key, dataset) in rows {
        if suffix.is_empty() && (object_id == "shop.party.display_name" || object_id == "shop.entry.memo") {
            continue;
        }
        let idempotency_key = format!("{key}{suffix}");
        if idempotency_key.len() < 16 {
            return Err(format!("short key {idempotency_key}"));
        }
        let candidate = candidate_of(reviews, object_id)?;
        let identity = fetch_current(env.origin, env.ca, reviewer).await.map_err(|_| format!("reviewer {object_id}"))?;
        let mut body = json!({
            "build_id": build_id,
            "choice": choice,
            "decision_kind": "label_description",
            "candidate_sha256": candidate,
            "identity_revision": identity.revision,
            "expected_revision": 1,
            "idempotency_key": idempotency_key
        });
        if choice == "approve" {
            body["fields"] = if dataset {
                json!({"description": {"overwrite": true}, "grain": {"overwrite": true}, "label": {"overwrite": true}})
            } else {
                json!({"description": {"overwrite": true}, "label": {"overwrite": true}})
            };
        }
        let response = post(
            env.bind,
            env.api_ca,
            &format!("/control/v1/reviews/{object_id}/decision"),
            reviewer,
            &identity.csrf_token,
            &body,
        )
        .await?;
        if response.0 != 200 || response.1["ledger_authenticated"] != false {
            return Err(format!("{object_id} {choice} {} {}", response.0, error_code(&response.1)));
        }
    }
    Ok(())
}

async fn prove_cached_rebuild(
    config: &Value,
    env: &HttpEnv<'_>,
    admin: &str,
    reviewer: &str,
    owner: &str,
    summary: &mut Value,
) -> Result<(), String> {
    let admin_id = fetch_current(env.origin, env.ca, admin).await.map_err(|_| "second admin".to_owned())?;
    let created_body = create_body(config, &admin_id.revision, "shopconvkey0000003");
    let created = post(env.bind, env.api_ca, "/control/v1/builds", admin, &admin_id.csrf_token, &created_body).await?;
    if created.0 != 202 {
        return Err(format!("second create {}", created.0));
    }
    let build_id = created.1["build_id"].as_str().unwrap_or("").to_owned();
    summary["second_build_id"] = json!(build_id);
    let ctx = ClientCtx { bind: env.bind, api_ca: env.api_ca, cookie: admin, origin: env.origin, ca: env.ca };
    let row = wait_stage(&ctx, &build_id, "S8", "succeeded", 900).await?;
    if stage_of(&row, "S5") != "succeeded" {
        return Err("second S5 failed".to_owned());
    }
    let generation = read_generation(config, &build_id)?;
    let counts = model_counts(&generation);
    write_counts(summary, "second_", &counts);
    if counts.misses != 0 || counts.hits == 0 || counts.sockets_opened != 0 {
        return Err(format!("second model misses {} hits {} opened {}", counts.misses, counts.hits, counts.sockets_opened));
    }
    let admin_replay = fetch_current(env.origin, env.ca, admin).await.map_err(|_| "replay admin".to_owned())?;
    let mut replay_body = create_body(config, &admin_replay.revision, "shopconvkey0000001");
    replay_body["identity_revision"] = json!(admin_replay.revision);
    let replay = post(env.bind, env.api_ca, "/control/v1/builds", admin, &admin_replay.csrf_token, &replay_body).await?;
    summary["create_replay"] = json!(replay.0);
    if replay.0 != 202 || replay.1["build_id"].as_str() == Some(build_id.as_str()) || replay.1["build_id"].as_str().unwrap_or("").is_empty() {
        return Err(format!("create replay {}", replay.0));
    }
    let reviews = get_text(env.bind, env.api_ca, admin, env.origin, env.ca, &format!("/control/v1/reviews?project_id=shop&build_id={build_id}")).await?;
    post_scope(env, reviewer, &build_id, &reviews.1, "b2").await?;
    let owner_id = fetch_current(env.origin, env.ca, owner).await.map_err(|_| "second owner".to_owned())?;
    let confirmed = class_body(config, &build_id, owner_id.revision, "shopb2class000001");
    let confirm = post(env.bind, env.api_ca, "/control/v1/classifications/confirm", owner, &owner_id.csrf_token, &confirmed).await?;
    if confirm.0 != 200 || confirm.1["grants"] != "not_authorized" {
        return Err(format!("second confirm {}", confirm.0));
    }
    let admin_pack = fetch_current(env.origin, env.ca, admin).await.map_err(|_| "second pack".to_owned())?;
    let packed = post(
        env.bind,
        env.api_ca,
        &format!("/control/v1/builds/{build_id}/pack"),
        admin,
        &admin_pack.csrf_token,
        &json!({"identity_revision": admin_pack.revision, "idempotency_key": "shopb2pack0000001"}),
    )
    .await?;
    summary["second_pack_status"] = json!(packed.0);
    if packed.0 != 200 {
        return Err(format!("second pack {} {}", packed.0, error_code(&packed.1)));
    }
    if packed.1.pointer("/producer/statements/field_candidates_applied") != Some(&Value::Bool(true)) {
        return Err("second pack did not apply candidates".to_owned());
    }
    if packed.1.pointer("/producer/reseal_count").and_then(Value::as_i64).unwrap_or(0) < 1 {
        return Err("second reseal stayed zero".to_owned());
    }
    let finished = get_text(env.bind, env.api_ca, admin, env.origin, env.ca, &format!("/control/v1/builds/{build_id}")).await?;
    summary["second_terminal_status"] = json!(finished.1["status"]);
    summary["second_terminal_stage"] = json!(finished.1["stage"]);
    if finished.1["status"] != "succeeded" || finished.1["stage"] != "S10" || finished.1["published"] != false {
        return Err(format!("second terminal {} {}", finished.1["status"], finished.1["stage"]));
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
