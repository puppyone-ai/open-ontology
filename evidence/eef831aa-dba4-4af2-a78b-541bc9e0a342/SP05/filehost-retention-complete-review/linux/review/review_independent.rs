//! Independent lifecycle checks for the frozen retention completion.
//! The harness does not unlink platform staging. The host does.

#![forbid(unsafe_code)]

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use artifact_retention::{
    Deployment, Now, RegisterOpen, RegisterPublished, RetentionClass, required_integration,
};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectManifest,
    PrincipalId, ProjectId, SessionId, UploadConfig, UploadSession,
};
use sha2::{Digest, Sha256};

const OPERATOR: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a342";

#[tokio::test]
async fn completed_kills_hold_quota_until_one_release_and_reuse_the_pointer() {
    let (parent, root) = scratch("kills");
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let deploy = Deployment::create(&root, limits, OPERATOR).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a342");
    let bytes = vec![0x41u8; 64];
    let config = upload("held.txt", &bytes);
    let host = open_host(&deploy);
    let manifest = publish_two_parts(&host, &ctx, &config, &bytes).await;
    assert_eq!(manifest.part_count, 2);
    assert!(manifest.object_key.as_str().starts_with("uploads/"));
    assert!(manifest.object_key.as_str().ends_with("/original"));
    assert!(!manifest.object_key.as_str().contains("/outputs/"));
    assert_eq!(manifest.contract, "oo.control.local-file-hosting/v1");
    let filehost = deploy.filehost_dir();
    let quota = quota_path(&filehost, "project_a", manifest.session_id.as_str());
    let pointer = pointer_path(
        &filehost,
        "project_a",
        "user_a",
        manifest.idempotency_key.as_str(),
    );
    let canonical = filehost.join(format!("{}#1", manifest.object_key.as_str()));
    let odd = filehost.join(format!("{}#01", manifest.object_key.as_str()));
    let foreign = filehost.join(format!("uploads/{}/dest#1", manifest.session_id.as_str()));
    let part_staging = filehost.join(format!(
        "staging/{}/part/00001#1",
        manifest.session_id.as_str()
    ));
    plant_file(&canonical, b"platform-original-1");
    plant_file(&odd, b"leading-zero");
    plant_file(&foreign, b"foreign-dest");
    plant_file(&part_staging, b"platform-part-1");
    let neighbor = parent.join("neighbor.txt");
    plant_file(&neighbor, b"outside-root");
    assert_eq!(std::fs::read(&quota).unwrap(), b"64");
    assert_eq!(
        std::fs::read(&pointer).unwrap(),
        manifest.session_id.as_str().as_bytes()
    );
    deploy
        .register_published(
            &host,
            &ctx,
            RegisterPublished {
                object_key: &manifest.object_key,
                class: RetentionClass::SessionOutput,
                role: "analyst",
                policy_id: "policy_a",
                ttl_seconds: 1,
                explicit_workspace: false,
                now: Now::Explicit(0),
            },
        )
        .await
        .unwrap();
    let permit = deploy
        .permit(
            &host,
            &ctx,
            &manifest.object_key,
            "other-role",
            "other-policy",
            Now::Explicit(500),
            None,
        )
        .await
        .unwrap();
    assert!(!permit.allowed);
    assert_eq!(permit.reason, "role_or_policy_mismatch");
    assert_eq!(permit.bearer_url, "none");
    assert_eq!(permit.gateway_url_revocation, "not_implemented");
    drop(host);

    let killed = kill_at_checkpoint(&root, "data_deleting");
    assert_eq!(killed.status.signal(), Some(9));
    let killed_out = String::from_utf8(killed.stdout).unwrap();
    assert!(!killed_out.contains("\"deletion_success\":true"));
    assert!(!killed_out.contains("\"absence_verified\":true"));
    let host = open_host(&deploy);
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(std::fs::read(&quota).unwrap(), b"64");
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        0
    );
    assert!(
        !host
            .observe_completed_absence(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
            )
            .await
            .unwrap()
    );
    assert_eq!(std::fs::read(&canonical).unwrap(), b"platform-original-1");
    assert_eq!(std::fs::read(&part_staging).unwrap(), b"platform-part-1");
    assert_eq!(
        host.begin(&ctx, &upload("other-deleting.txt", &bytes))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    drop(host);

    let resumed = reap_cli(&root);
    assert_eq!(resumed.status.code(), Some(0));
    let stdout = String::from_utf8(resumed.stdout).unwrap();
    assert!(stdout.contains("\"deletion_success\":true"));
    assert!(stdout.contains("\"restarted_from_partial\":true"));
    assert!(stdout.contains("\"parent_complete\":false"));
    assert!(stdout.contains("\"absence_verified\":true"));
    let host = open_host(&deploy);
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );
    assert!(
        host.observe_completed_absence(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
        )
        .await
        .unwrap()
    );
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
    assert!(!quota.exists());
    assert!(!canonical.exists());
    assert!(!part_staging.exists());
    assert_eq!(std::fs::read(&odd).unwrap(), b"leading-zero");
    assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign-dest");
    assert_eq!(std::fs::read(&neighbor).unwrap(), b"outside-root");
    drop(host);
    let repeated = reap_cli(&root);
    assert_eq!(repeated.status.code(), Some(0));
    let host = open_host(&deploy);
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
    let reused = host.begin(&ctx, &config).await.unwrap();
    assert_ne!(reused.session_id.as_str(), manifest.session_id.as_str());
    assert_eq!(
        std::fs::read(&pointer).unwrap(),
        reused.session_id.as_str().as_bytes()
    );
    assert_eq!(
        host.begin(&ctx, &upload("other-after.txt", &bytes))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    assert_eq!(
        quota_file_names(&filehost, "project_a"),
        vec![reused.session_id.as_str().to_owned()]
    );
    host.abort(&ctx, &reused.session_id).await.unwrap();
    assert!(quota_file_names(&filehost, "project_a").is_empty());

    let again = vec![0x42u8; 64];
    let again_config = upload("after.txt", &again);
    let second = publish_two_parts(&host, &ctx, &again_config, &again).await;
    let second_quota = quota_path(&filehost, "project_a", second.session_id.as_str());
    deploy
        .register_published(
            &host,
            &ctx,
            RegisterPublished {
                object_key: &second.object_key,
                class: RetentionClass::SessionOutput,
                role: "analyst",
                policy_id: "policy_a",
                ttl_seconds: 1,
                explicit_workspace: false,
                now: Now::Explicit(0),
            },
        )
        .await
        .unwrap();
    drop(host);
    let killed_absent = kill_at_checkpoint(&root, "data_absent");
    assert_eq!(killed_absent.status.signal(), Some(9));
    let absent_out = String::from_utf8(killed_absent.stdout).unwrap();
    assert!(!absent_out.contains("\"deletion_success\":true"));
    assert!(!absent_out.contains("\"absence_verified\":true"));
    let host = open_host(&deploy);
    assert_eq!(
        host.read_object(&ctx, &second.object_key)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(std::fs::read(&second_quota).unwrap(), b"64");
    assert_eq!(
        host.completed_quota_release_count(&ctx, &second.object_key)
            .await
            .unwrap(),
        0
    );
    assert!(
        !host
            .observe_completed_absence(
                &ctx,
                &second.object_key,
                second.object_digest,
                second.size,
                second.lineage_digest,
            )
            .await
            .unwrap()
    );
    let tomb = std::fs::read_to_string(
        filehost.join(format!("meta/tombstone/{}", second.session_id.as_str())),
    )
    .unwrap();
    assert!(tomb.contains("phase data_absent\n"));
    assert!(tomb.contains("release_count 0\n"));
    assert!(tomb.contains("order bytes_then_quota\n"));
    assert_eq!(
        host.begin(&ctx, &upload("blocked-absent.txt", &again))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    drop(host);
    let resumed_absent = reap_cli(&root);
    assert_eq!(resumed_absent.status.code(), Some(0));
    let absent_stdout = String::from_utf8(resumed_absent.stdout).unwrap();
    assert!(absent_stdout.contains("\"deletion_success\":true"));
    assert!(absent_stdout.contains("\"restarted_from_partial\":true"));
    assert!(absent_stdout.contains("\"parent_complete\":false"));
    let host = open_host(&deploy);
    assert!(
        host.observe_completed_absence(
            &ctx,
            &second.object_key,
            second.object_digest,
            second.size,
            second.lineage_digest,
        )
        .await
        .unwrap()
    );
    assert_eq!(
        host.completed_quota_release_count(&ctx, &second.object_key)
            .await
            .unwrap(),
        1
    );
    assert!(!second_quota.exists());
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
    let reused_again = host.begin(&ctx, &again_config).await.unwrap();
    assert_ne!(reused_again.session_id.as_str(), second.session_id.as_str());
    assert_eq!(
        host.begin(&ctx, &upload("one-slot.txt", &again))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    drop(host);
    let repeated_absent = reap_cli(&root);
    assert_eq!(repeated_absent.status.code(), Some(0));
    let host = open_host(&deploy);
    assert_eq!(
        host.completed_quota_release_count(&ctx, &second.object_key)
            .await
            .unwrap(),
        1
    );
    drop(host);
    std::fs::remove_dir_all(&parent).unwrap();
}

#[tokio::test]
async fn open_reap_drops_canonical_staging_and_checks_the_receipt_grant() {
    let seams = required_integration();
    assert_eq!(
        seams,
        [
            "filehost_object_version",
            "filehost_admission_role_and_policy",
            "gateway_authenticated_url_revocation",
            "formula_safe_csv_excel",
            "sessions_outputs_prefix",
        ]
        .as_slice()
    );
    let (parent, root) = scratch("open");
    let deploy = Deployment::create(&root, HostLimits::standard(), OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a342");
    let bytes = b"open-platform-body";
    let config = upload("open.txt", bytes);
    let row = deploy
        .register_open(
            &host,
            &ctx,
            RegisterOpen {
                config: &config,
                class: RetentionClass::UploadedSource,
                role: "analyst",
                policy_id: "policy_a",
                ttl_seconds: 30,
                explicit_workspace: false,
                now: Now::Explicit(100),
            },
        )
        .await
        .unwrap();
    let session = SessionId::parse(&row.session_id).unwrap();
    host.put_part(&ctx, &session, 1, bytes).await.unwrap();
    let filehost = deploy.filehost_dir();
    let canonical = filehost.join(format!("{}#1", row.object_key));
    let odd = filehost.join(format!("{}#01", row.object_key));
    let part = filehost.join(format!("staging/{}/part/00001", row.session_id));
    let part_staging = filehost.join(format!("staging/{}/part/00001#1", row.session_id));
    let dest = filehost.join(format!("staging/{}/part/dest#1", row.session_id));
    let notes = filehost.join(format!("staging/{}/part/notes", row.session_id));
    plant_file(&canonical, b"platform-original-1");
    plant_file(&odd, b"leading-zero");
    plant_file(&part_staging, b"platform-part-1");
    plant_file(&dest, b"foreign-dest");
    plant_file(&notes, b"keep-notes");
    let neighbor = parent.join("neighbor.txt");
    plant_file(&neighbor, b"outside-root");
    assert!(part.is_file());
    let report = deploy
        .reap(
            &host,
            Now::Explicit(100 + 30_000),
            artifact_retention::PauseAfter::None,
        )
        .await
        .unwrap();
    assert!(report.deletion_success);
    assert!(!report.parent_complete);
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, "aborted");
    assert!(report.outcomes[0].deleted);
    assert!(report.outcomes[0].absence_verified);
    assert_eq!(report.outcomes[0].seam, "none");
    assert!(!canonical.exists());
    assert!(!part_staging.exists());
    assert!(!part.exists());
    assert_eq!(std::fs::read(&odd).unwrap(), b"leading-zero");
    assert_eq!(std::fs::read(&dest).unwrap(), b"foreign-dest");
    assert_eq!(std::fs::read(&notes).unwrap(), b"keep-notes");
    assert_eq!(std::fs::read(&neighbor).unwrap(), b"outside-root");
    let grant_hex = hex_encode(ctx.grant_evidence.as_str().as_bytes());
    assert_eq!(row.grant_hex, grant_hex);
    let receipt_path = filehost.join(format!("meta/open-absence/{}", row.session_id));
    let receipt = std::fs::read_to_string(&receipt_path).unwrap();
    assert!(receipt.contains(&format!("session {}\n", row.session_id)));
    assert!(receipt.contains(&format!("project {}\n", row.project)));
    assert!(receipt.contains(&format!("principal {}\n", row.principal)));
    assert!(receipt.contains(&format!("grant_hex {grant_hex}\n")));
    assert!(receipt.contains(&format!("object_key {}\n", row.object_key)));
    assert!(host.observe_open_absence(&ctx, &session).await.unwrap());
    let wrong_grant = admission("project_a", "user_a", "grant-forged");
    assert_eq!(
        host.observe_open_absence(&wrong_grant, &session)
            .await
            .unwrap_err(),
        FileHostError::Denied
    );
    let wrong_project = admission("project_b", "user_b", "grant-a342");
    assert_eq!(
        host.observe_open_absence(&wrong_project, &session)
            .await
            .unwrap_err(),
        FileHostError::Denied
    );
    assert_eq!(
        host.abort(&ctx, &session).await.unwrap_err(),
        FileHostError::NotFound
    );
    assert!(host.observe_open_absence(&ctx, &session).await.unwrap());
    let again = deploy
        .reap(
            &host,
            Now::Explicit(100 + 30_000),
            artifact_retention::PauseAfter::None,
        )
        .await
        .unwrap();
    assert!(again.deletion_success);
    assert!(!again.parent_complete);
    assert_eq!(again.outcomes[0].status, "aborted");
    assert!(again.outcomes[0].absence_verified);
    assert_eq!(std::fs::read_to_string(&receipt_path).unwrap(), receipt);
    assert_eq!(std::fs::read(&dest).unwrap(), b"foreign-dest");
    drop(host);
    std::fs::remove_dir_all(&parent).unwrap();
}

#[tokio::test]
async fn open_receipt_window_restarts_once_and_rejects_a_forged_grant() {
    let (parent, root) = scratch("window");
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a342");
    let bytes = vec![0x51u8; 64];
    let config = upload("window.txt", &bytes);
    let session = host.begin(&ctx, &config).await.unwrap();
    host.put_part(&ctx, &session.session_id, 1, &bytes)
        .await
        .unwrap();
    let canonical = root.join(format!("{}#1", session.object_key.as_str()));
    let odd = root.join(format!("{}#01", session.object_key.as_str()));
    let dest = root.join(format!(
        "staging/{}/part/dest#1",
        session.session_id.as_str()
    ));
    plant_file(&canonical, b"platform-original-1");
    plant_file(&odd, b"leading-zero");
    plant_file(&dest, b"foreign-dest");
    let quota = quota_path(&root, "project_a", session.session_id.as_str());
    let session_path = root.join(format!("meta/session/{}", session.session_id.as_str()));
    let pointer = pointer_path(
        &root,
        "project_a",
        "user_a",
        session.idempotency_key.as_str(),
    );
    let saved_quota = std::fs::read(&quota).unwrap();
    let saved_session = std::fs::read(&session_path).unwrap();
    let saved_pointer = std::fs::read(&pointer).unwrap();
    assert_eq!(saved_quota, b"64");
    host.abort(&ctx, &session.session_id).await.unwrap();
    assert!(!canonical.exists());
    assert_eq!(std::fs::read(&odd).unwrap(), b"leading-zero");
    assert_eq!(std::fs::read(&dest).unwrap(), b"foreign-dest");
    assert!(!quota.exists());
    assert!(!session_path.exists());
    assert!(!pointer.exists());
    let grant_hex = hex_encode(ctx.grant_evidence.as_str().as_bytes());
    let receipt_path = root.join(format!("meta/open-absence/{}", session.session_id.as_str()));
    let receipt = std::fs::read_to_string(&receipt_path).unwrap();
    assert!(receipt.contains(&format!("grant_hex {grant_hex}\n")));
    assert!(receipt.contains(&format!("session {}\n", session.session_id.as_str())));
    assert!(
        host.observe_open_absence(&ctx, &session.session_id)
            .await
            .unwrap()
    );

    plant_file(&quota, &saved_quota);
    plant_file(&session_path, &saved_session);
    plant_file(&pointer, &saved_pointer);
    let observed_while_held = host
        .observe_open_absence(&ctx, &session.session_id)
        .await
        .unwrap();
    eprintln!("receipt_window observe_while_quota_held={observed_while_held}");
    assert_eq!(std::fs::read(&quota).unwrap(), b"64");
    assert_eq!(
        host.begin(&ctx, &upload("other-window.txt", &bytes))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    let resumed = host.begin(&ctx, &config).await.unwrap();
    assert_eq!(resumed.session_id.as_str(), session.session_id.as_str());
    assert_eq!(
        quota_file_names(&root, "project_a"),
        vec![session.session_id.as_str().to_owned()]
    );
    host.abort(&ctx, &session.session_id).await.unwrap();
    assert!(!quota.exists());
    assert!(!session_path.exists());
    assert!(
        host.observe_open_absence(&ctx, &session.session_id)
            .await
            .unwrap()
    );
    assert_eq!(
        host.abort(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(std::fs::read_to_string(&receipt_path).unwrap(), receipt);
    assert_eq!(
        host.observe_open_absence(
            &admission("project_a", "user_a", "grant-forged"),
            &session.session_id
        )
        .await
        .unwrap_err(),
        FileHostError::Denied
    );

    plant_file(&quota, &saved_quota);
    assert_eq!(
        host.abort(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::NotFound
    );
    let replanted = host
        .observe_open_absence(&ctx, &session.session_id)
        .await
        .unwrap();
    let quota_still_present = quota.exists();
    eprintln!("notfound_replant observe={replanted} quota_still_present={quota_still_present}");
    assert_eq!(std::fs::read(&quota).unwrap(), saved_quota);
    assert_eq!(
        host.observe_open_absence(
            &admission("project_a", "user_a", "grant-forged"),
            &session.session_id
        )
        .await
        .unwrap_err(),
        FileHostError::Denied
    );
    drop(host);

    let forged_deploy_parent = parent.join("forged-deploy");
    std::fs::create_dir_all(&forged_deploy_parent).unwrap();
    let forged_operator = forged_deploy_parent.join(OPERATOR);
    let forged_deploy = Deployment::create(&forged_operator, limits, OPERATOR).unwrap();
    let forged_live = open_host(&forged_deploy);
    let forged_bytes = vec![0x52u8; 64];
    let forged_config = upload("forged.txt", &forged_bytes);
    let forged_row = forged_deploy
        .register_open(
            &forged_live,
            &ctx,
            RegisterOpen {
                config: &forged_config,
                class: RetentionClass::UploadedSource,
                role: "analyst",
                policy_id: "policy_a",
                ttl_seconds: 30,
                explicit_workspace: false,
                now: Now::Explicit(100),
            },
        )
        .await
        .unwrap();
    let forged_session = SessionId::parse(&forged_row.session_id).unwrap();
    forged_live
        .put_part(&ctx, &forged_session, 1, &forged_bytes)
        .await
        .unwrap();
    let forged_store = forged_deploy.filehost_dir();
    let forged_canonical = forged_store.join(format!("{}#1", forged_row.object_key));
    let forged_dest = forged_store.join(format!("staging/{}/part/dest#1", forged_row.session_id));
    let forged_part = forged_store.join(format!("staging/{}/part/00001", forged_row.session_id));
    plant_file(&forged_canonical, b"platform-original-1");
    plant_file(&forged_dest, b"foreign-dest");
    let forged_quota = quota_path(&forged_store, "project_a", &forged_row.session_id);
    assert_eq!(std::fs::read(&forged_quota).unwrap(), b"64");
    let forged_grant_hex = hex_encode(b"grant-forged");
    let forged_receipt = format!(
        "oo.control.local-file-hosting/v1 open-absence\nsession {}\nproject {}\nprincipal {}\ngrant_hex {forged_grant_hex}\nobject_key {}\nexpected_digest {}\nexpected_size {}\n",
        forged_row.session_id,
        forged_row.project,
        forged_row.principal,
        forged_row.object_key,
        forged_row.object_digest_hex,
        forged_row.size
    );
    plant_file(
        &forged_store.join(format!("meta/open-absence/{}", forged_row.session_id)),
        forged_receipt.as_bytes(),
    );
    let forged_report = forged_deploy
        .reap(
            &forged_live,
            Now::Explicit(100 + 30_000),
            artifact_retention::PauseAfter::None,
        )
        .await
        .unwrap();
    assert!(!forged_report.deletion_success);
    assert!(!forged_report.parent_complete);
    assert!(!forged_report.outcomes[0].deleted);
    assert!(!forged_report.outcomes[0].absence_verified);
    assert_eq!(std::fs::read(&forged_quota).unwrap(), b"64");
    assert_eq!(std::fs::read(&forged_dest).unwrap(), b"foreign-dest");
    assert!(!forged_canonical.exists());
    assert!(!forged_part.exists());
    assert_eq!(
        forged_live
            .observe_open_absence(&ctx, &forged_session)
            .await
            .unwrap_err(),
        FileHostError::Denied
    );
    let attacker = admission("project_a", "user_a", "grant-forged");
    assert_eq!(
        forged_live
            .observe_open_absence(&attacker, &forged_session)
            .await
            .unwrap_err(),
        FileHostError::Conflict
    );
    assert_eq!(
        forged_live
            .begin(&ctx, &upload("still-held.txt", &forged_bytes))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    drop(forged_live);
    std::fs::remove_dir_all(&parent).unwrap();
}

#[tokio::test]
async fn symlink_store_error_and_wrong_tuple_keep_the_reservation() {
    let (parent, root) = scratch("refuse");
    let deploy = Deployment::create(&root, HostLimits::standard(), OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a342");
    let bytes = b"refuse-target-body";
    let manifest = publish_one(&host, &ctx, bytes, "refuse.txt").await;
    deploy
        .register_published(
            &host,
            &ctx,
            RegisterPublished {
                object_key: &manifest.object_key,
                class: RetentionClass::UploadedSource,
                role: "analyst",
                policy_id: "policy_a",
                ttl_seconds: 60,
                explicit_workspace: false,
                now: Now::Explicit(10),
            },
        )
        .await
        .unwrap();
    let filehost = deploy.filehost_dir();
    let quota = quota_path(&filehost, "project_a", manifest.session_id.as_str());
    let quota_body = std::fs::read(&quota).unwrap();
    let object_path = filehost.join(manifest.object_key.as_str());
    let notes = object_path.with_file_name("notes");
    plant_file(&notes, b"keep-notes");
    let mut flipped = manifest.object_digest;
    flipped[0] ^= 0xff;
    assert_eq!(
        host.delete_completed(
            &ctx,
            &manifest.object_key,
            flipped,
            manifest.size,
            manifest.lineage_digest,
            false,
        )
        .await
        .unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        host.delete_completed(
            &admission("project_a", "user_a", "grant-forged"),
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
            false,
        )
        .await
        .unwrap_err(),
        FileHostError::Denied
    );
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(std::fs::read(&quota).unwrap(), quota_body);
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        0
    );
    let aside = object_path.with_file_name("original.aside");
    std::fs::rename(&object_path, &aside).unwrap();
    std::os::unix::fs::symlink(&aside, &object_path).unwrap();
    let linked = deploy
        .reap(
            &host,
            Now::Explicit(70_000),
            artifact_retention::PauseAfter::None,
        )
        .await
        .unwrap();
    assert_eq!(linked.outcomes[0].status, "refused_tamper_or_symlink");
    assert!(!linked.outcomes[0].deleted);
    assert!(!linked.outcomes[0].absence_verified);
    assert!(!linked.deletion_success);
    assert!(!linked.parent_complete);
    assert!(
        std::fs::symlink_metadata(&object_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&aside).unwrap(), bytes);
    assert_eq!(std::fs::read(&quota).unwrap(), quota_body);
    assert_eq!(std::fs::read(&notes).unwrap(), b"keep-notes");
    std::fs::remove_file(&object_path).unwrap();
    std::fs::rename(&aside, &object_path).unwrap();
    let manifest_path = filehost.join(format!("meta/manifest/{}", manifest.session_id.as_str()));
    let manifest_aside = manifest_path.with_extension("aside");
    std::fs::rename(&manifest_path, &manifest_aside).unwrap();
    std::fs::create_dir(&manifest_path).unwrap();
    let broken = deploy
        .reap(
            &host,
            Now::Explicit(70_000),
            artifact_retention::PauseAfter::None,
        )
        .await
        .unwrap();
    assert!(!broken.outcomes[0].absence_verified);
    assert!(!broken.outcomes[0].deleted);
    assert!(!broken.deletion_success);
    assert!(!broken.parent_complete);
    assert_ne!(broken.outcomes[0].filehost_after, "found");
    assert_eq!(std::fs::read(&object_path).unwrap(), bytes);
    assert_eq!(std::fs::read(&quota).unwrap(), quota_body);
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        0
    );
    assert_eq!(std::fs::read(&notes).unwrap(), b"keep-notes");
    drop(host);
    std::fs::remove_dir_all(&parent).unwrap();
}

fn scratch(label: &str) -> (PathBuf, PathBuf) {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let parent = std::env::temp_dir().join(format!("oo-a342-{label}-{n}-{}", std::process::id()));
    std::fs::create_dir_all(&parent).unwrap();
    let root = parent.join(OPERATOR);
    (parent, root)
}

fn open_host(deploy: &Deployment) -> FileHost {
    FileHost::open_local(&deploy.filehost_dir(), deploy.limits()).unwrap()
}

fn admission(project: &str, principal: &str, grant: &str) -> AdmissionContext {
    AdmissionContext {
        project_id: ProjectId::new(project).unwrap(),
        principal_id: PrincipalId::new(principal).unwrap(),
        grant_evidence: GrantEvidence::new(grant).unwrap(),
    }
}

fn upload(name: &str, bytes: &[u8]) -> UploadConfig {
    UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: sha256(bytes),
        expected_size: bytes.len() as u64,
        content_type: Some("text/plain".to_owned()),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn plant_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
    assert!(path.symlink_metadata().unwrap().is_file());
}

fn quota_path(root: &Path, project: &str, session: &str) -> PathBuf {
    root.join(format!("meta/quota/{project}/{session}"))
}

fn pointer_path(root: &Path, project: &str, principal: &str, idem: &str) -> PathBuf {
    root.join(format!("meta/idempotency/{project}/{principal}/{idem}"))
}

fn quota_file_names(root: &Path, project: &str) -> Vec<String> {
    let dir = root.join(format!("meta/quota/{project}"));
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return names;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}

async fn publish_one(
    host: &FileHost,
    ctx: &AdmissionContext,
    bytes: &[u8],
    name: &str,
) -> ObjectManifest {
    let config = upload(name, bytes);
    publish_two_parts(host, ctx, &config, bytes).await
}

async fn publish_two_parts(
    host: &FileHost,
    ctx: &AdmissionContext,
    config: &UploadConfig,
    bytes: &[u8],
) -> ObjectManifest {
    let session: UploadSession = host.begin(ctx, config).await.unwrap();
    let (left, right) = bytes.split_at(bytes.len() / 2);
    host.put_part(ctx, &session.session_id, 1, left)
        .await
        .unwrap();
    host.put_part(ctx, &session.session_id, 2, right)
        .await
        .unwrap();
    host.complete(ctx, &session.session_id).await.unwrap()
}

fn retention_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_artifact-retention")
        .or(option_env!("CARGO_BIN_EXE_artifact_retention"))
        .expect("cargo did not set the artifact-retention binary path")
}

fn kill_at_checkpoint(root: &Path, phase: &str) -> std::process::Output {
    let mut child = Command::new(retention_bin())
        .arg("reap")
        .arg("--root")
        .arg(root)
        .arg("--pause-after")
        .arg(phase)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let checkpoint = root.join("checkpoint/reap.txt");
    let needle = format!("phase {phase}\n");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&checkpoint)
            && text.contains(&needle)
        {
            break;
        }
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            panic!("reaper did not reach {phase}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    child.wait_with_output().unwrap()
}

fn reap_cli(root: &Path) -> std::process::Output {
    Command::new(retention_bin())
        .arg("reap")
        .arg("--root")
        .arg(root)
        .output()
        .unwrap()
}
