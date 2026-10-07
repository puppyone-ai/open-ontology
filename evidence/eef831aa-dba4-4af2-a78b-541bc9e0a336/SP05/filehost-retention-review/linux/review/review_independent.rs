//! Independent review of FILE-D03 / E41. This file is not product source.
//!
//! It drives the public FileHost and artifact-retention APIs. It does not
//! unlink a store object to simulate deletion and it does not edit a quota body.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use artifact_retention::{
    Deployment, Now, PauseAfter, RegisterOpen, RegisterPublished, RegistrationKind, RetentionClass,
    required_integration,
};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectKey,
    ObjectManifest, PrincipalId, ProjectId, SessionId, UploadConfig,
};
use sha2::{Digest, Sha256};

const OPERATOR: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a336";

const OPEN_SEAMS: [&str; 7] = [
    "filehost_part_absence_observation",
    "filehost_dest_staging_reap",
    "filehost_object_version",
    "filehost_admission_role_and_policy",
    "gateway_authenticated_url_revocation",
    "formula_safe_csv_excel",
    "sessions_outputs_prefix",
];

#[tokio::test]
async fn lifecycle_reap_proves_public_absence_and_keeps_parent_open() {
    assert_eq!(required_integration(), OPEN_SEAMS);
    assert!(!required_integration().contains(&"filehost_completed_object_delete"));
    assert!(!required_integration().contains(&"filehost_quota_release_on_completed_delete"));

    let (_parent, root) = scratch("life");
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let deploy = Deployment::create(&root, limits, OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = vec![0x5a; 64];
    let config = upload("notes.txt", &bytes);
    let manifest = publish_halves(&host, &ctx, &config, &bytes).await;
    assert_eq!(manifest.part_count, 2);
    assert_eq!(manifest.contract, "oo.control.local-file-hosting/v1");

    let mut download = host
        .open_download(&ctx, &manifest.object_key, 8192, None)
        .await
        .unwrap();
    let mut got = Vec::new();
    while let Some(chunk) = download.next_chunk().await.unwrap() {
        got.extend(chunk);
    }
    assert!(download.is_complete());
    assert_eq!(got, bytes);

    assert_eq!(
        host.abort(&ctx, &manifest.session_id).await.unwrap_err(),
        FileHostError::Closed
    );
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );

    let row = deploy
        .register_published(
            &host,
            &ctx,
            published(&manifest, RetentionClass::SessionOutput, 60, false, 1_000),
        )
        .await
        .unwrap();
    assert_eq!(row.kind, RegistrationKind::Published);
    assert_eq!(row.part_count, Some(2));
    assert_eq!(row.object_digest_hex, hex(&sha256(&bytes)));

    let decision = deploy
        .permit(
            &host,
            &ctx,
            &manifest.object_key,
            "viewer",
            "policy_a",
            Now::Explicit(1_000),
            None,
        )
        .await
        .unwrap();
    assert!(!decision.allowed);
    assert_eq!(decision.bearer_url, "none");
    assert_eq!(decision.gateway_url_revocation, "not_implemented");
    assert_eq!(
        deploy
            .pull(
                &host,
                &ctx,
                &manifest.object_key,
                "analyst",
                "policy_a",
                Now::Explicit(1_000),
                None,
            )
            .await
            .unwrap(),
        bytes
    );

    let foreign_ctx = admission("project_b", "user_b", "grant-foreign");
    let foreign_bytes = vec![0x22; 32];
    let foreign = publish_halves(
        &host,
        &foreign_ctx,
        &upload("foreign.txt", &foreign_bytes),
        &foreign_bytes,
    )
    .await;
    deploy
        .register_published(
            &host,
            &foreign_ctx,
            published(
                &foreign,
                RetentionClass::UploadedSource,
                180 * 86_400,
                false,
                1_000,
            ),
        )
        .await
        .unwrap();
    let foreign_quota = quota_path(&deploy, "project_b", foreign.session_id.as_str());
    let foreign_quota_body = std::fs::read(&foreign_quota).unwrap();

    let part_dir = deploy
        .filehost_dir()
        .join(format!("staging/{}/part", manifest.session_id.as_str()));
    std::fs::create_dir_all(&part_dir).unwrap();
    let owned_part = part_dir.join("00001");
    let dest = part_dir.join("dest#1");
    let notes = part_dir.join("notes");
    std::fs::write(&owned_part, b"leftover-part").unwrap();
    std::fs::write(&dest, b"dest-leftover").unwrap();
    std::fs::write(&notes, b"not-a-part").unwrap();
    let outside = root.join("operator-note.txt");
    std::fs::write(&outside, b"keep-outside-filehost").unwrap();
    let neighbor = root.parent().unwrap().join("neighbor-keep.txt");
    std::fs::write(&neighbor, b"keep-neighbor").unwrap();

    let idle = deploy
        .reap(&host, Now::Explicit(1_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(idle.idle);
    assert!(!idle.deletion_success);
    assert!(!idle.parent_complete);

    let report = deploy
        .reap(&host, Now::Explicit(1_000 + 60_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(report.deletion_success);
    assert!(!report.parent_complete);
    assert!(!report.idle);
    assert_eq!(report.outcomes.len(), 1);
    assert!(report.outcomes[0].deleted);
    assert!(report.outcomes[0].absence_verified);
    assert_eq!(report.outcomes[0].status, "deleted");
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
    let tomb = std::fs::read_to_string(tomb_path(&deploy, manifest.session_id.as_str())).unwrap();
    assert!(tomb.contains("phase verified\n"));
    assert!(tomb.contains("part_count 2\n"));
    assert!(tomb.contains("release_count 1\n"));
    assert!(
        !owned_part.exists(),
        "five-digit owned part name was not deleted"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"dest-leftover");
    assert_eq!(std::fs::read(&notes).unwrap(), b"not-a-part");
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-outside-filehost");
    assert_eq!(std::fs::read(&neighbor).unwrap(), b"keep-neighbor");
    assert_eq!(std::fs::read(&foreign_quota).unwrap(), foreign_quota_body);
    assert_eq!(
        host.read_object(&foreign_ctx, &foreign.object_key)
            .await
            .unwrap()
            .bytes,
        foreign_bytes
    );

    let replay = host.begin(&ctx, &config).await.unwrap();
    assert_ne!(replay.session_id.as_str(), manifest.session_id.as_str());
    let again = upload("other.txt", &bytes);
    assert_eq!(
        host.begin(&ctx, &again).await.unwrap_err(),
        FileHostError::Quota
    );
    let repeated = deploy
        .reap(&host, Now::Explicit(1_000 + 60_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(repeated.deletion_success);
    assert!(!repeated.parent_complete);
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        host.begin(&ctx, &again).await.unwrap_err(),
        FileHostError::Quota
    );
}

#[tokio::test]
async fn quota_release_before_bytes_admits_one_new_reservation() {
    let (_parent, root) = scratch("window");
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let max = limits.max_project_bytes();
    let deploy = Deployment::create(&root, limits, OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = vec![0x71; 64];
    let config = upload("held.txt", &bytes);
    let manifest = publish_halves(&host, &ctx, &config, &bytes).await;
    let object = object_path(&deploy, manifest.object_key.as_str());
    let old_quota = quota_path(&deploy, "project_a", manifest.session_id.as_str());
    assert_eq!(regular_len(&object), 64);
    assert_eq!(std::fs::read(&old_quota).unwrap(), b"64");

    let stopped = host
        .delete_completed(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
            true,
        )
        .await
        .unwrap();
    assert!(stopped.is_none());
    assert_eq!(regular_len(&object), bytes.len() as u64);
    assert!(!old_quota.exists());
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
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
    let same = host.begin(&ctx, &config).await.unwrap();
    assert_eq!(same.session_id.as_str(), manifest.session_id.as_str());
    assert!(!old_quota.exists());

    let other = upload("concurrent.txt", &bytes);
    let begun = host.begin(&ctx, &other).await.unwrap();
    assert_ne!(begun.session_id.as_str(), manifest.session_id.as_str());
    let new_quota = quota_path(&deploy, "project_a", begun.session_id.as_str());
    let reserved: u64 = std::str::from_utf8(&std::fs::read(&new_quota).unwrap())
        .unwrap()
        .parse()
        .unwrap();
    let old_len = regular_len(&object);
    eprintln!(
        "quota_window old_bytes={old_len} reserved={reserved} max={max} reservation_within_max={} bytes_plus_reservation_exceeds_max={}",
        reserved <= max,
        old_len + reserved > max
    );
    assert!(reserved <= max);
    assert_eq!(reserved, bytes.len() as u64);
    assert!(old_len + reserved > max);
    let third = upload("third.txt", &bytes);
    assert_eq!(
        host.begin(&ctx, &third).await.unwrap_err(),
        FileHostError::Quota
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

    let finished = host
        .delete_completed(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
            false,
        )
        .await
        .unwrap();
    assert!(finished.is_some());
    assert!(!object.exists());
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
    assert_eq!(std::fs::read(&new_quota).unwrap(), b"64");
    assert_eq!(
        host.begin(&ctx, &config).await.unwrap_err(),
        FileHostError::Quota
    );
    host.abort(&ctx, &begun.session_id).await.unwrap();
    assert!(!new_quota.exists());
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
    let replay = host.begin(&ctx, &config).await.unwrap();
    assert_ne!(replay.session_id.as_str(), manifest.session_id.as_str());
}

#[tokio::test]
async fn quota_release_then_symlink_is_not_absence() {
    let (_parent, root) = scratch("stuck");
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let deploy = Deployment::create(&root, limits, OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = vec![0x73; 64];
    let manifest = publish_halves(&host, &ctx, &upload("stuck.txt", &bytes), &bytes).await;
    host.delete_completed(
        &ctx,
        &manifest.object_key,
        manifest.object_digest,
        manifest.size,
        manifest.lineage_digest,
        true,
    )
    .await
    .unwrap();
    let begun = host
        .begin(&ctx, &upload("beside.txt", &bytes))
        .await
        .unwrap();
    let new_quota = quota_path(&deploy, "project_a", begun.session_id.as_str());
    assert_eq!(std::fs::read(&new_quota).unwrap(), b"64");

    let object = object_path(&deploy, manifest.object_key.as_str());
    let aside = object.with_file_name("original.aside");
    std::fs::rename(&object, &aside).unwrap();
    std::os::unix::fs::symlink(&aside, &object).unwrap();
    let failed = host
        .delete_completed(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
            false,
        )
        .await
        .unwrap_err();
    assert_eq!(failed, FileHostError::Corrupt);
    assert!(
        std::fs::symlink_metadata(&object)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&aside).unwrap(), bytes);
    assert_eq!(std::fs::read(&new_quota).unwrap(), b"64");
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
    assert_ne!(
        host.observe_completed_absence(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
        )
        .await,
        Ok(true)
    );
    assert_eq!(
        host.begin(&ctx, &upload("third.txt", &bytes))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
}

#[tokio::test]
async fn sigkill_at_quota_released_restarts_once() {
    let (_parent, root) = scratch("kill");
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let deploy = Deployment::create(&root, limits, OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = vec![0x41; 64];
    let config = upload("kill.txt", &bytes);
    let manifest = publish_halves(&host, &ctx, &config, &bytes).await;
    deploy
        .register_published(
            &host,
            &ctx,
            published(&manifest, RetentionClass::SessionOutput, 1, false, 0),
        )
        .await
        .unwrap();
    drop(host);

    let bin = retention_bin();
    let mut child = Command::new(bin)
        .arg("reap")
        .arg("--root")
        .arg(&root)
        .arg("--pause-after")
        .arg("quota_released")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let checkpoint = root.join("checkpoint/reap.txt");
    let waiting = root.join("control/waiting");
    let started = Instant::now();
    loop {
        let phase = std::fs::read_to_string(&checkpoint).unwrap_or_default();
        if phase.contains("phase quota_released\n") && waiting.is_file() {
            break;
        }
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            panic!("reaper did not pause at quota_released");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(child.try_wait().unwrap().is_none());
    child.kill().unwrap();
    let killed = child.wait_with_output().unwrap();
    assert!(!killed.status.success());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(killed.status.signal(), Some(9));
    }
    let killed_out = String::from_utf8(killed.stdout).unwrap();
    assert!(!killed_out.contains("\"deletion_success\":true"));
    let host = open_host(&deploy);
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
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
    drop(host);

    let resumed = Command::new(bin)
        .arg("reap")
        .arg("--root")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(resumed.status.code(), Some(0));
    let stdout = String::from_utf8(resumed.stdout).unwrap();
    assert!(stdout.contains("\"deletion_success\":true"));
    assert!(stdout.contains("\"restarted_from_partial\":true"));
    assert!(stdout.contains("\"parent_complete\":false"));
    assert!(stdout.contains("\"absence_verified\":true"));
    let checkpoint_text = std::fs::read_to_string(&checkpoint).unwrap();
    assert!(checkpoint_text.contains("absence_verified false\n"));
    assert!(checkpoint_text.contains("deletion_success false\n"));
    let host = open_host(&deploy);
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
    let replay = host.begin(&ctx, &config).await.unwrap();
    assert_ne!(replay.session_id.as_str(), manifest.session_id.as_str());
    assert_eq!(
        host.begin(&ctx, &upload("second.txt", &bytes))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    drop(host);
    let repeated = Command::new(bin)
        .arg("reap")
        .arg("--root")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(repeated.status.code(), Some(0));
    let host = open_host(&deploy);
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn incomplete_pull_does_not_become_complete() {
    let (_parent, root) = scratch("pull");
    let limits = HostLimits::try_new(64, 64, 10, 128).unwrap();
    let deploy = Deployment::create(&root, limits, OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = vec![0x11; 64];
    let manifest = publish_halves(&host, &ctx, &upload("pull.txt", &bytes), &bytes).await;

    let mut finished = host
        .open_download(&ctx, &manifest.object_key, 8192, None)
        .await
        .unwrap();
    while finished.next_chunk().await.unwrap().is_some() {}
    assert!(finished.is_complete());

    let mut partial = host
        .open_download(&ctx, &manifest.object_key, 32, None)
        .await
        .unwrap();
    let first = partial.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.len(), 32);
    assert!(!partial.is_complete());

    host.delete_completed(
        &ctx,
        &manifest.object_key,
        manifest.object_digest,
        manifest.size,
        manifest.lineage_digest,
        false,
    )
    .await
    .unwrap();
    assert!(finished.next_chunk().await.unwrap().is_none());
    assert!(finished.is_complete());
    let failed = partial.next_chunk().await.unwrap_err();
    assert!(!partial.is_complete());
    assert!(matches!(
        failed,
        FileHostError::Store | FileHostError::Corrupt
    ));
    assert_ne!(
        host.observe_completed_absence(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
        )
        .await
        .ok(),
        Some(false)
    );
}

#[tokio::test]
async fn drift_symlink_tuple_and_read_error_do_not_delete() {
    let (_parent, root) = scratch("refuse");
    let deploy = Deployment::create(&root, HostLimits::standard(), OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = b"refuse-body-review".to_vec();
    let manifest = publish_halves(&host, &ctx, &upload("refuse.txt", &bytes), &bytes).await;
    deploy
        .register_published(
            &host,
            &ctx,
            published(&manifest, RetentionClass::UploadedSource, 60, false, 10),
        )
        .await
        .unwrap();
    let object = object_path(&deploy, manifest.object_key.as_str());

    let other = admission("project_b", "user_b", "grant-review");
    assert_eq!(
        host.delete_completed(
            &other,
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
    let wrong_grant = admission("project_a", "user_a", "grant-other");
    assert_eq!(
        host.delete_completed(
            &wrong_grant,
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
    let mut wrong_digest = manifest.object_digest;
    wrong_digest[0] ^= 0xff;
    assert_eq!(
        host.delete_completed(
            &ctx,
            &manifest.object_key,
            wrong_digest,
            manifest.size,
            manifest.lineage_digest,
            false,
        )
        .await
        .unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(
        host.completed_quota_release_count(&ctx, &manifest.object_key)
            .await
            .unwrap(),
        0
    );
    assert_ne!(
        host.observe_completed_absence(
            &ctx,
            &manifest.object_key,
            wrong_digest,
            manifest.size,
            manifest.lineage_digest,
        )
        .await,
        Ok(true)
    );

    let mut flipped = bytes.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    std::fs::write(&object, &flipped).unwrap();
    let drifted = deploy
        .reap(&host, Now::Explicit(70_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!drifted.deletion_success);
    assert!(!drifted.parent_complete);
    assert!(!drifted.outcomes[0].deleted);
    assert!(!drifted.outcomes[0].absence_verified);
    assert_eq!(drifted.outcomes[0].status, "refused_tamper_or_symlink");
    assert_eq!(std::fs::read(&object).unwrap(), flipped);
    std::fs::write(&object, &bytes).unwrap();

    let aside = object.with_file_name("original.aside");
    std::fs::rename(&object, &aside).unwrap();
    std::os::unix::fs::symlink(&aside, &object).unwrap();
    let linked = deploy
        .reap(&host, Now::Explicit(70_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!linked.outcomes[0].deleted);
    assert!(!linked.outcomes[0].absence_verified);
    assert_eq!(linked.outcomes[0].status, "refused_tamper_or_symlink");
    assert!(
        std::fs::symlink_metadata(&object)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&aside).unwrap(), bytes);
    std::fs::remove_file(&object).unwrap();
    std::fs::rename(&aside, &object).unwrap();

    let manifest_path = deploy
        .filehost_dir()
        .join(format!("meta/manifest/{}", manifest.session_id.as_str()));
    let manifest_aside = manifest_path.with_extension("aside");
    std::fs::rename(&manifest_path, &manifest_aside).unwrap();
    std::fs::create_dir(&manifest_path).unwrap();
    let broken = deploy
        .reap(&host, Now::Explicit(70_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!broken.deletion_success);
    assert!(!broken.outcomes[0].deleted);
    assert!(!broken.outcomes[0].absence_verified);
    assert_ne!(broken.outcomes[0].status, "deleted");
    assert_eq!(std::fs::read(&object).unwrap(), bytes);
    assert_ne!(
        host.observe_completed_absence(
            &ctx,
            &manifest.object_key,
            manifest.object_digest,
            manifest.size,
            manifest.lineage_digest,
        )
        .await,
        Ok(true)
    );
}

#[tokio::test]
async fn open_abort_and_dest_staging_stay_unsupported() {
    let (_parent, root) = scratch("open");
    let deploy = Deployment::create(&root, HostLimits::standard(), OPERATOR).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-review");
    let bytes = b"open-session-body".to_vec();
    let config = upload("open.txt", &bytes);
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
    host.put_part(&ctx, &session, 1, &bytes).await.unwrap();
    let part_dir = deploy
        .filehost_dir()
        .join(format!("staging/{}/part", row.session_id));
    let dest = part_dir.join("dest#1");
    std::fs::write(&dest, b"dest-open").unwrap();
    let outside = root.join("open-keep.txt");
    std::fs::write(&outside, b"keep").unwrap();
    let report = deploy
        .reap(&host, Now::Explicit(100 + 30_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!report.deletion_success);
    assert!(!report.parent_complete);
    assert_eq!(report.outcomes[0].status, "aborted_parts_unverified");
    assert_eq!(report.outcomes[0].seam, "filehost_part_absence_observation");
    assert!(!report.outcomes[0].deleted);
    assert!(!report.outcomes[0].absence_verified);
    assert_eq!(std::fs::read(&dest).unwrap(), b"dest-open");
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
    assert!(required_integration().contains(&"filehost_part_absence_observation"));
    assert!(required_integration().contains(&"filehost_dest_staging_reap"));
    let key = ObjectKey::parse(&row.object_key).unwrap();
    assert_ne!(
        host.observe_completed_absence(&ctx, &key, sha256(&bytes), bytes.len() as u64, [0; 32])
            .await,
        Ok(true)
    );
}

fn retention_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_artifact-retention")
        .or(option_env!("CARGO_BIN_EXE_artifact_retention"))
        .expect("cargo did not set the artifact-retention binary path")
}

fn published<'a>(
    manifest: &'a ObjectManifest,
    class: RetentionClass,
    ttl_seconds: u64,
    explicit_workspace: bool,
    created_unix_ms: u64,
) -> RegisterPublished<'a> {
    RegisterPublished {
        object_key: &manifest.object_key,
        class,
        role: "analyst",
        policy_id: "policy_a",
        ttl_seconds,
        explicit_workspace,
        now: Now::Explicit(created_unix_ms),
    }
}

fn scratch(label: &str) -> (PathBuf, PathBuf) {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let parent =
        std::env::temp_dir().join(format!("oo-a336-review-{label}-{n}-{}", std::process::id()));
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

async fn publish_halves(
    host: &FileHost,
    ctx: &AdmissionContext,
    config: &UploadConfig,
    bytes: &[u8],
) -> ObjectManifest {
    let session = host.begin(ctx, config).await.unwrap();
    let mid = bytes.len() / 2;
    assert!(mid > 0 && mid < bytes.len());
    host.put_part(ctx, &session.session_id, 1, &bytes[..mid])
        .await
        .unwrap();
    host.put_part(ctx, &session.session_id, 2, &bytes[mid..])
        .await
        .unwrap();
    host.complete(ctx, &session.session_id).await.unwrap()
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn quota_path(deploy: &Deployment, project: &str, session: &str) -> PathBuf {
    deploy
        .filehost_dir()
        .join(format!("meta/quota/{project}/{session}"))
}

fn object_path(deploy: &Deployment, key: &str) -> PathBuf {
    deploy.filehost_dir().join(key)
}

fn tomb_path(deploy: &Deployment, session: &str) -> PathBuf {
    deploy
        .filehost_dir()
        .join(format!("meta/tombstone/{session}"))
}

fn regular_len(path: &Path) -> u64 {
    let meta = std::fs::symlink_metadata(path).unwrap();
    assert!(meta.file_type().is_file());
    meta.len()
}
