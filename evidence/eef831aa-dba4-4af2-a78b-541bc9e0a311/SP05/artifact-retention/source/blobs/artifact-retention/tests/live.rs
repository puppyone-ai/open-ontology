//! Private FileHost fixtures. No network, no PostgreSQL, no Python.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use artifact_retention::{
    Deployment, Error, Now, OWNER_UUID, PauseAfter, RegisterOpen, RegisterPublished,
    RetentionClass, enforce_download_ttl, required_integration, resolve_now,
};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectKey,
    ObjectManifest, PrincipalId, ProjectId, SessionId, UploadConfig,
};
use sha2::{Digest, Sha256};

#[test]
fn production_clock_matches_the_process_clock() {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let now = resolve_now(Now::Production).unwrap();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(now >= before && now <= after);
    assert!(enforce_download_ttl(300).is_ok());
    assert_eq!(enforce_download_ttl(301), Err(Error::TtlCap));
    assert_eq!(enforce_download_ttl(0), Err(Error::TtlCap));
    let seams = required_integration();
    assert!(seams.contains(&"filehost_completed_object_delete"));
    assert!(seams.contains(&"gateway_authenticated_url_revocation"));
    assert!(seams.contains(&"formula_safe_csv_excel"));
}

#[test]
fn guessed_paths_and_symlink_roots_are_refused() {
    assert!(ObjectKey::parse("../public-user-keep.txt").is_err());
    assert!(ObjectKey::parse("sessions/abc/outputs/chart.csv").is_err());
    assert!(ObjectKey::parse("/tmp/user/file").is_err());
    let (parent, root) = scratch();
    std::fs::write(parent.join("public-user-keep.txt"), b"keep-user").unwrap();
    assert!(Deployment::create(&parent.join("public-user"), HostLimits::standard()).is_err());
    let real = parent.join("real-dir");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &root).unwrap();
    assert_eq!(
        Deployment::create(&root, HostLimits::standard()).unwrap_err(),
        Error::Symlink
    );
    std::fs::remove_file(&root).unwrap();
    assert_eq!(
        std::fs::read(parent.join("public-user-keep.txt")).unwrap(),
        b"keep-user"
    );
}

#[tokio::test]
async fn ttl_classes_stay_distinct_and_workspace_defaults_off() {
    let (_parent, root) = scratch();
    let deploy = Deployment::create(&root, HostLimits::standard()).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a311");
    let source = b"uploaded-source-body";
    let output = b"session-output-body";
    let workspace = b"workspace-snapshot-body";
    let source_manifest = publish(&host, &ctx, source, "source.txt").await;
    let output_manifest = publish(&host, &ctx, output, "output.txt").await;
    let workspace_manifest = publish(&host, &ctx, workspace, "workspace.txt").await;

    let denied = deploy
        .register_published(
            &host,
            &ctx,
            published(
                &workspace_manifest,
                RetentionClass::WorkspaceSnapshot,
                86_400,
                false,
                1,
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(denied, Error::WorkspaceDisabled);

    let over_day = deploy
        .register_published(
            &host,
            &ctx,
            published(
                &workspace_manifest,
                RetentionClass::WorkspaceSnapshot,
                86_401,
                true,
                1,
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(over_day, Error::TtlCap);

    let workspace_row = deploy
        .register_published(
            &host,
            &ctx,
            published(
                &workspace_manifest,
                RetentionClass::WorkspaceSnapshot,
                86_400,
                true,
                1,
            ),
        )
        .await
        .unwrap();
    assert!(workspace_row.explicit_workspace);
    assert_eq!(
        workspace_row.manifest_contract,
        "oo.control.local-file-hosting/v1"
    );

    assert_eq!(
        deploy
            .register_published(
                &host,
                &ctx,
                published(
                    &source_manifest,
                    RetentionClass::UploadedSource,
                    180 * 86_400 + 1,
                    false,
                    1
                ),
            )
            .await
            .unwrap_err(),
        Error::TtlCap
    );
    let source_row = deploy
        .register_published(
            &host,
            &ctx,
            published(
                &source_manifest,
                RetentionClass::UploadedSource,
                180 * 86_400,
                false,
                1,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        deploy
            .register_published(
                &host,
                &ctx,
                published(
                    &output_manifest,
                    RetentionClass::SessionOutput,
                    30 * 86_400 + 1,
                    false,
                    1
                ),
            )
            .await
            .unwrap_err(),
        Error::TtlCap
    );
    let output_row = deploy
        .register_published(
            &host,
            &ctx,
            published(
                &output_manifest,
                RetentionClass::SessionOutput,
                30 * 86_400,
                false,
                1,
            ),
        )
        .await
        .unwrap();
    assert_ne!(source_row.registration_id, output_row.registration_id);
    assert_ne!(source_row.class, output_row.class);
    assert_eq!(source_row.class, RetentionClass::UploadedSource);
    assert_eq!(output_row.class, RetentionClass::SessionOutput);
    let text = std::fs::read_to_string(root.join("inventory/registrations.txt")).unwrap();
    assert!(text.contains("version absent"));
    assert!(text.contains("version_reason filehost_manifest_has_no_version"));
    assert!(!text.contains("version v1"));
}

#[tokio::test]
async fn published_reap_does_not_fake_absence_and_keeps_quota() {
    let (_parent, root) = scratch();
    let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
    let deploy = Deployment::create(&root, limits).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a311");
    let bytes = vec![0x5a; 64];
    let config = upload("notes.txt", &bytes);
    let manifest = publish_config(&host, &ctx, &config, &bytes).await;
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

    let row = deploy
        .register_published(
            &host,
            &ctx,
            published(&manifest, RetentionClass::SessionOutput, 60, false, 1_000),
        )
        .await
        .unwrap();
    assert_eq!(row.object_digest_hex, hex(&sha256(&bytes)));
    let early = deploy
        .reap(&host, Now::Explicit(1_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(early.idle);
    assert!(!early.deletion_success);
    assert!(!early.parent_complete);

    let report = deploy
        .reap(&host, Now::Explicit(1_000 + 60_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!report.deletion_success);
    assert!(!report.parent_complete);
    assert!(!report.idle);
    assert_eq!(report.outcomes.len(), 1);
    assert!(!report.outcomes[0].deleted);
    assert!(!report.outcomes[0].absence_verified);
    assert_eq!(
        report.outcomes[0].status,
        "unsupported_completed_object_delete"
    );
    assert_eq!(report.outcomes[0].seam, "filehost_completed_object_delete");
    assert_eq!(report.outcomes[0].filehost_after, "found");
    let stored = host.read_object(&ctx, &manifest.object_key).await.unwrap();
    assert_eq!(stored.bytes, bytes);
    assert!(host.read_object(&ctx, &manifest.object_key).await.is_ok());

    let replay = host.begin(&ctx, &config).await.unwrap();
    assert_eq!(replay.session_id.as_str(), manifest.session_id.as_str());
    let other_bytes = [0x11; 64];
    let other = upload("other.txt", &other_bytes);
    assert_eq!(
        host.begin(&ctx, &other).await.unwrap_err(),
        FileHostError::Quota
    );

    let foreign_ctx = admission("project_b", "user_b", "grant-b");
    let foreign_bytes = vec![0x22; 64];
    let foreign = publish(&host, &foreign_ctx, &foreign_bytes, "foreign.txt").await;
    let again = deploy
        .reap(&host, Now::Explicit(1_000 + 60_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!again.deletion_success);
    assert_eq!(
        host.read_object(&foreign_ctx, &foreign.object_key)
            .await
            .unwrap()
            .bytes,
        foreign_bytes
    );
}

#[tokio::test]
async fn current_tuple_refuses_the_pull_without_a_url() {
    let (_parent, root) = scratch();
    let deploy = Deployment::create(&root, HostLimits::standard()).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a311");
    let bytes = b"tuple-body-a311";
    let manifest = publish(&host, &ctx, bytes, "tuple.txt").await;
    deploy
        .register_published(
            &host,
            &ctx,
            published(
                &manifest,
                RetentionClass::SessionOutput,
                3_600,
                false,
                5_000,
            ),
        )
        .await
        .unwrap();
    let now = Now::Explicit(5_000);
    let pulled = deploy
        .pull(
            &host,
            &ctx,
            &manifest.object_key,
            "analyst",
            "policy_a",
            now,
            None,
        )
        .await
        .unwrap();
    assert_eq!(pulled, bytes);
    let refused = deploy
        .permit(
            &host,
            &ctx,
            &manifest.object_key,
            "viewer",
            "policy_a",
            now,
            None,
        )
        .await
        .unwrap();
    assert!(!refused.allowed);
    assert_eq!(refused.reason, "role_or_policy_mismatch");
    assert_eq!(refused.filehost_admission, "project_principal_matched");
    assert_eq!(refused.gateway_url_revocation, "not_implemented");
    assert_eq!(refused.bearer_url, "none");
    assert_eq!(
        deploy
            .pull(
                &host,
                &ctx,
                &manifest.object_key,
                "viewer",
                "policy_a",
                now,
                None
            )
            .await
            .unwrap_err(),
        Error::TupleMismatch
    );
    let other = admission("project_b", "user_a", "grant-a311");
    assert_eq!(
        deploy
            .pull(
                &host,
                &other,
                &manifest.object_key,
                "analyst",
                "policy_a",
                now,
                None
            )
            .await
            .unwrap_err(),
        Error::Denied
    );
    assert_eq!(
        deploy
            .pull(
                &host,
                &ctx,
                &manifest.object_key,
                "analyst",
                "policy_a",
                now,
                Some(301)
            )
            .await
            .unwrap_err(),
        Error::TtlCap
    );
    let again = deploy
        .pull(
            &host,
            &ctx,
            &manifest.object_key,
            "analyst",
            "policy_a",
            now,
            Some(300),
        )
        .await
        .unwrap();
    assert_eq!(again, bytes);
    assert_eq!(
        deploy
            .pull(
                &host,
                &ctx,
                &manifest.object_key,
                "analyst",
                "policy_a",
                Now::Explicit(5_000 + 3_600_000),
                None,
            )
            .await
            .unwrap_err(),
        Error::Expired
    );
    let expired_permit = deploy
        .permit(
            &host,
            &ctx,
            &manifest.object_key,
            "analyst",
            "policy_a",
            Now::Explicit(5_000 + 3_600_000),
            None,
        )
        .await
        .unwrap();
    assert!(!expired_permit.allowed);
    assert_eq!(expired_permit.reason, "expired");
    assert_eq!(expired_permit.filehost_admission, "not_consulted");
    assert_eq!(expired_permit.gateway_url_revocation, "not_implemented");
    assert_eq!(expired_permit.bearer_url, "none");
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );
}

#[tokio::test]
async fn tamper_drift_symlink_and_read_error_do_not_delete() {
    let (_parent, root) = scratch();
    let deploy = Deployment::create(&root, HostLimits::standard()).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a311");
    let bytes = b"tamper-target-body";
    let manifest = publish(&host, &ctx, bytes, "tamper.txt").await;
    deploy
        .register_published(
            &host,
            &ctx,
            published(&manifest, RetentionClass::UploadedSource, 60, false, 10),
        )
        .await
        .unwrap();
    let object_path = deploy.filehost_dir().join(manifest.object_key.as_str());
    let mut flipped = bytes.to_vec();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    std::fs::write(&object_path, &flipped).unwrap();
    let drifted = deploy
        .reap(&host, Now::Explicit(70_000), PauseAfter::None)
        .await
        .unwrap();
    assert_eq!(drifted.outcomes[0].status, "refused_tamper_or_symlink");
    assert!(!drifted.outcomes[0].deleted);
    assert!(!drifted.deletion_success);
    assert_eq!(std::fs::read(&object_path).unwrap(), flipped);

    std::fs::write(&object_path, bytes).unwrap();
    let inventory = root.join("inventory/registrations.txt");
    let original = std::fs::read(&inventory).unwrap();
    let mut tampered = original.clone();
    let marker = b"object_digest ";
    let start = find_subslice(&tampered, marker).unwrap() + marker.len();
    tampered[start] = if tampered[start] == b'0' { b'1' } else { b'0' };
    write_hashed(&inventory, &tampered);
    let hashed = deploy
        .reap(&host, Now::Explicit(70_000), PauseAfter::None)
        .await
        .unwrap();
    assert_eq!(hashed.outcomes[0].status, "refused_drift");
    assert!(!hashed.outcomes[0].deleted);
    assert_eq!(std::fs::read(&object_path).unwrap(), bytes);
    write_hashed(&inventory, &original);

    let aside = object_path.with_file_name("original.aside");
    std::fs::rename(&object_path, &aside).unwrap();
    std::os::unix::fs::symlink(&aside, &object_path).unwrap();
    let linked = deploy
        .reap(&host, Now::Explicit(70_000), PauseAfter::None)
        .await
        .unwrap();
    assert_eq!(linked.outcomes[0].status, "refused_tamper_or_symlink");
    assert!(!linked.outcomes[0].deleted);
    assert!(
        std::fs::symlink_metadata(&object_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&aside).unwrap(), bytes);
    std::fs::remove_file(&object_path).unwrap();
    std::fs::rename(&aside, &object_path).unwrap();

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
    assert!(!broken.outcomes[0].absence_verified);
    assert!(!broken.outcomes[0].deleted);
    assert!(!broken.deletion_success);
    assert_ne!(broken.outcomes[0].filehost_after, "found");
    assert_eq!(std::fs::read(&object_path).unwrap(), bytes);
}

#[tokio::test]
async fn open_abort_does_not_claim_part_absence() {
    let (_parent, root) = scratch();
    let deploy = Deployment::create(&root, HostLimits::standard()).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a311");
    let bytes = b"open-session-body";
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
    let part = deploy
        .filehost_dir()
        .join(format!("staging/{}/part/00001", row.session_id));
    assert!(part.is_file());
    let keep = deploy.root().join("user-keep.txt");
    std::fs::write(&keep, b"keep").unwrap();
    let foreign_name = deploy.filehost_dir().join("notes-foreign.txt");
    std::fs::write(&foreign_name, b"foreign").unwrap();
    let report = deploy
        .reap(&host, Now::Explicit(100 + 30_000), PauseAfter::None)
        .await
        .unwrap();
    assert!(!report.deletion_success);
    assert!(!report.parent_complete);
    assert!(!report.outcomes[0].deleted);
    assert!(!report.outcomes[0].absence_verified);
    assert_eq!(report.outcomes[0].status, "aborted_parts_unverified");
    assert_eq!(report.outcomes[0].seam, "filehost_part_absence_observation");
    assert!(!part.exists());
    let replay = host.begin(&ctx, &config).await.unwrap();
    assert_ne!(replay.session_id.as_str(), row.session_id);
    assert_eq!(std::fs::read(&keep).unwrap(), b"keep");
    assert_eq!(std::fs::read(&foreign_name).unwrap(), b"foreign");
}

#[tokio::test]
async fn kill_between_metadata_and_object_step_stays_partial() {
    let (_parent, root) = scratch();
    let deploy = Deployment::create(&root, HostLimits::standard()).unwrap();
    let host = open_host(&deploy);
    let ctx = admission("project_a", "user_a", "grant-a311");
    let bytes = b"OOA311-retention-kill-v1";
    let manifest = publish(&host, &ctx, bytes, "kill.txt").await;
    deploy
        .register_published(
            &host,
            &ctx,
            published(&manifest, RetentionClass::SessionOutput, 1, false, 0),
        )
        .await
        .unwrap();
    drop(host);
    let bin = option_env!("CARGO_BIN_EXE_artifact-retention")
        .or(option_env!("CARGO_BIN_EXE_artifact_retention"))
        .expect("cargo did not set the artifact-retention binary path");
    let mut child = Command::new(bin)
        .arg("reap")
        .arg("--root")
        .arg(&root)
        .arg("--pause-after")
        .arg("metadata_api")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let checkpoint = root.join("checkpoint/reap.txt");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&checkpoint)
            && text.contains("phase metadata_api_done\n")
        {
            break;
        }
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            panic!("reaper did not reach metadata_api_done");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    let host = open_host(&deploy);
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );

    let resumed = Command::new(bin)
        .arg("reap")
        .arg("--root")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(resumed.status.code(), Some(2));
    let stdout = String::from_utf8(resumed.stdout).unwrap();
    assert!(stdout.contains("\"deletion_success\":false"));
    assert!(stdout.contains("\"restarted_from_partial\":true"));
    assert!(stdout.contains("\"parent_complete\":false"));
    assert!(stdout.contains("\"absence_verified\":false"));
    assert!(stdout.contains("\"deleted\":false"));
    assert!(stdout.contains("\"filehost_after\":\"found\""));
    assert!(stdout.contains("unsupported_completed_object_delete"));
    assert_eq!(
        host.read_object(&ctx, &manifest.object_key)
            .await
            .unwrap()
            .bytes,
        bytes
    );
    let rejected = Command::new(bin)
        .arg("reap")
        .arg("--root")
        .arg(&root)
        .arg("--now")
        .arg("1")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
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

fn scratch() -> (PathBuf, PathBuf) {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let parent = std::env::temp_dir().join(format!("oo-a311-ret-{n}-{}", std::process::id()));
    std::fs::create_dir_all(&parent).unwrap();
    let root = parent.join(OWNER_UUID);
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

async fn publish(
    host: &FileHost,
    ctx: &AdmissionContext,
    bytes: &[u8],
    name: &str,
) -> ObjectManifest {
    let config = upload(name, bytes);
    publish_config(host, ctx, &config, bytes).await
}

async fn publish_config(
    host: &FileHost,
    ctx: &AdmissionContext,
    config: &UploadConfig,
    bytes: &[u8],
) -> ObjectManifest {
    let session = host.begin(ctx, config).await.unwrap();
    let mid = bytes.len() / 2;
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

fn write_hashed(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    let digest = format!("{}\n", hex(&sha256(bytes)));
    let side = path.with_file_name("registrations.txt.sha256");
    std::fs::write(side, digest).unwrap();
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
