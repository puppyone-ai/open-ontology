//! Local filesystem integration for upload sessions.
//!
//! These tests open a real [`object_store::local::LocalFileSystem`] under a private
//! scratch directory. They do not stand in for an S3 backend.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, MAX_OBJECT_BYTES,
    MAX_PART_COUNT, ObjectKey, ObjectManifest, PrincipalId, ProjectId, SessionId, UploadConfig,
};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn multipart_reopen_reads_manifest_and_lineage() {
    let scratch = Scratch::new("multipart");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let part_a = "季度-α".as_bytes();
    let part_b = b"\x00\xff\xfe";
    let mut object = Vec::new();
    object.extend_from_slice(part_a);
    object.extend_from_slice(part_b);
    let cfg = upload("季度.csv", &object, Some("text/csv"));
    let first = admission("project_a", "user_a", "grant-one");
    let refreshed = admission("project_a", "user_a", "grant-two");

    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    assert!(!format!("{host:?}").contains("oo-a181-"));
    let session = host.begin(&first, &cfg).await.unwrap();
    let session_text = session.session_id.as_str().to_owned();
    let key_text = session.object_key.as_str().to_owned();
    assert_eq!(
        key_text,
        format!("uploads/{session_text}/{}/original", hex(&digest(&object)))
    );
    assert!(!key_text.contains("季度"));
    assert!(!key_text.contains(".."));
    assert_eq!(
        session.idempotency_key.as_str(),
        hex(&idempotency_oracle(
            "project_a",
            "user_a",
            &digest(&object),
            object.len() as u64,
            "季度.csv",
            Some("text/csv"),
        ))
    );
    assert_eq!(
        oo_control::idempotency_key(&first, &cfg).unwrap().as_str(),
        session.idempotency_key.as_str()
    );
    assert_eq!(
        oo_control::idempotency_key(&refreshed, &cfg)
            .unwrap()
            .as_str(),
        session.idempotency_key.as_str()
    );

    host.put_part(&first, &session.session_id, 2, part_b)
        .await
        .unwrap();
    assert_eq!(
        host.complete(&first, &session.session_id)
            .await
            .unwrap_err(),
        FileHostError::Incomplete
    );
    assert!(fetch(&root, &key_text).await.is_none());
    assert_eq!(
        fetch(&root, &format!("staging/{session_text}/part/00002"))
            .await
            .as_deref(),
        Some(part_b.as_slice())
    );

    drop(host);
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let parsed = SessionId::parse(&session_text).unwrap();
    let receipt = host.put_part(&first, &parsed, 1, part_a).await.unwrap();
    assert_eq!(receipt.part_number, 1);
    assert_eq!(receipt.size, part_a.len() as u64);
    assert_eq!(receipt.part_sha256, digest(part_a));
    let again = host.put_part(&first, &parsed, 1, part_a).await.unwrap();
    assert_eq!(again, receipt);

    let resumed = host.begin(&refreshed, &cfg).await.unwrap();
    assert_eq!(resumed.session_id.as_str(), session_text);
    assert_eq!(resumed.object_key.as_str(), key_text);
    let manifest = host.complete(&refreshed, &parsed).await.unwrap();
    assert_eq!(manifest.contract, "oo.control.local-file-hosting/v1");
    assert_eq!(manifest.grant_evidence.as_str(), "grant-one");
    assert_eq!(manifest.display_name, "季度.csv");
    assert_eq!(manifest.content_type.as_deref(), Some("text/csv"));
    assert_eq!(manifest.object_digest, digest(&object));
    assert_eq!(manifest.size, object.len() as u64);
    assert_eq!(manifest.part_count, 2);
    assert_eq!(manifest.project_id.as_str(), "project_a");
    assert_eq!(manifest.principal_id.as_str(), "user_a");
    assert_eq!(
        hex(&manifest.lineage_digest),
        hex(&lineage_oracle(&manifest))
    );
    assert_ne!(manifest.lineage_digest, manifest.object_digest);
    assert!(
        fetch(&root, &format!("staging/{session_text}/part/00001"))
            .await
            .is_none()
    );
    assert!(
        fetch(&root, &format!("staging/{session_text}/part/00002"))
            .await
            .is_none()
    );
    assert_eq!(
        fetch(&root, &key_text).await.as_deref(),
        Some(object.as_slice())
    );

    drop(host);
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let second = host
        .complete(&first, &SessionId::parse(&session_text).unwrap())
        .await
        .unwrap();
    assert_eq!(second, manifest);
    let key = ObjectKey::parse(&key_text).unwrap();
    let read_manifest = host.read_manifest(&first, &key).await.unwrap();
    assert_eq!(read_manifest, manifest);
    let stored = host.read_object(&refreshed, &key).await.unwrap();
    assert_eq!(stored.bytes, object);
    assert_eq!(stored.manifest, manifest);
    assert_eq!(
        host.put_part(&first, &parsed, 1, part_a).await.unwrap_err(),
        FileHostError::Closed
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");

    write_store(&root, &key_text, b"mutated").await;
    assert_eq!(
        host.read_object(&first, &key).await.unwrap_err(),
        FileHostError::Corrupt
    );
}

#[tokio::test]
async fn conflicting_part_and_config_do_not_overwrite() {
    let scratch = Scratch::new("conflict");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a");
    let bytes = b"aaaa";
    let cfg = upload("notes.txt", bytes, None);
    let session = host.begin(&ctx, &cfg).await.unwrap();
    host.put_part(&ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    let part_key = format!("staging/{}/part/00001", session.session_id.as_str());
    assert_eq!(
        host.put_part(&ctx, &session.session_id, 1, b"bbbb")
            .await
            .unwrap_err(),
        FileHostError::Conflict
    );
    assert_eq!(
        fetch(&root, &part_key).await.as_deref(),
        Some(bytes.as_slice())
    );
    let same = host
        .put_part(&ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    assert_eq!(same.part_sha256, digest(bytes));

    let other = UploadConfig {
        display_name: "other.txt".to_owned(),
        expected_sha256: digest(bytes),
        expected_size: 4,
        content_type: None,
    };
    let other_key = oo_control::idempotency_key(&ctx, &other).unwrap();
    assert_ne!(other_key.as_str(), session.idempotency_key.as_str());
    let pointer = format!(
        "meta/idempotency/{}/{}/{}",
        ctx.project_id.as_str(),
        ctx.principal_id.as_str(),
        other_key.as_str()
    );
    write_store(&root, &pointer, session.session_id.as_str().as_bytes()).await;
    let before = object_keys(&root).await;
    assert_eq!(
        host.begin(&ctx, &other).await.unwrap_err(),
        FileHostError::Conflict
    );
    assert_eq!(object_keys(&root).await, before);
    assert_eq!(
        fetch(&root, &part_key).await.as_deref(),
        Some(bytes.as_slice())
    );
    let resumed = host.begin(&ctx, &cfg).await.unwrap();
    assert_eq!(resumed.session_id.as_str(), session.session_id.as_str());
    let manifest = host.complete(&ctx, &session.session_id).await.unwrap();
    assert_eq!(manifest.object_digest, digest(bytes));
    assert_eq!(
        fetch(&root, manifest.object_key.as_str()).await.as_deref(),
        Some(bytes.as_slice())
    );
}

#[tokio::test]
async fn two_contexts_deny_a_known_key() {
    let scratch = Scratch::new("contexts");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let bytes = b"same-bytes";
    let owner = admission("project_a", "user_a", "grant-a");
    let other = admission("project_b", "user_b", "grant-b");
    let cfg = upload("same.bin", bytes, None);
    let owned = host.begin(&owner, &cfg).await.unwrap();
    host.put_part(&owner, &owned.session_id, 1, bytes)
        .await
        .unwrap();
    let manifest = host.complete(&owner, &owned.session_id).await.unwrap();
    let second = host.begin(&other, &cfg).await.unwrap();
    assert_ne!(second.session_id.as_str(), owned.session_id.as_str());
    assert_ne!(second.object_key.as_str(), owned.object_key.as_str());
    host.put_part(&other, &second.session_id, 1, bytes)
        .await
        .unwrap();
    let other_manifest = host.complete(&other, &second.session_id).await.unwrap();
    assert_eq!(other_manifest.object_digest, manifest.object_digest);
    assert_ne!(
        other_manifest.object_key.as_str(),
        manifest.object_key.as_str()
    );
    assert!(!format!("{other_manifest:?}").contains("project_a"));
    assert!(!format!("{other_manifest:?}").contains(owned.session_id.as_str()));

    let denied = host
        .read_manifest(&other, &owned.object_key)
        .await
        .unwrap_err();
    assert_eq!(denied, FileHostError::Denied);
    assert_eq!(
        denied.to_string(),
        "admission context does not match this upload"
    );
    assert!(!denied.to_string().contains("project_a"));
    assert!(!denied.to_string().contains(owned.session_id.as_str()));
    assert_eq!(
        host.read_object(&other, &owned.object_key)
            .await
            .unwrap_err(),
        FileHostError::Denied
    );
    assert_eq!(
        host.put_part(&other, &owned.session_id, 1, bytes)
            .await
            .unwrap_err(),
        FileHostError::Denied
    );
    assert_eq!(
        host.abort(&other, &owned.session_id).await.unwrap_err(),
        FileHostError::Denied
    );
    let still = host.read_object(&owner, &owned.object_key).await.unwrap();
    assert_eq!(still.bytes, bytes);

    let wrong = ObjectKey::parse(format!(
        "uploads/{}/{}/original",
        manifest.session_id.as_str(),
        "00".repeat(32)
    ))
    .unwrap();
    assert_eq!(
        host.read_manifest(&owner, &wrong).await.unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        host.read_manifest(&other, &wrong).await.unwrap_err(),
        FileHostError::NotFound
    );
    let absent = ObjectKey::parse(format!(
        "uploads/{}/{}/original",
        "ab".repeat(16),
        "cd".repeat(32)
    ))
    .unwrap();
    assert_eq!(
        host.read_manifest(&owner, &absent).await.unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        ObjectKey::parse("uploads/../etc/passwd").unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        SessionId::parse("not-a-session").unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        host.abort(&owner, &SessionId::parse("ab".repeat(16)).unwrap())
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );
}

#[tokio::test]
async fn checksum_and_size_failure_leave_no_visible_object() {
    let scratch = Scratch::new("failure");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let limits = HostLimits::try_new(64, 64, 8, 16).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a");
    let uploaded = b"\x00\xffnot-the-hash!";
    let expected_bytes = b"\x00\xffyes-the-hash!";
    assert_eq!(uploaded.len(), expected_bytes.len());
    let expected = digest(expected_bytes);
    assert_ne!(digest(uploaded), expected);
    let cfg = UploadConfig {
        display_name: "bad.bin".to_owned(),
        expected_sha256: expected,
        expected_size: u64::try_from(uploaded.len()).unwrap(),
        content_type: None,
    };
    let session = host.begin(&ctx, &cfg).await.unwrap();
    let other = upload("other.bin", uploaded, None);
    assert_eq!(
        host.begin(&ctx, &other).await.unwrap_err(),
        FileHostError::Quota
    );
    host.put_part(&ctx, &session.session_id, 1, uploaded)
        .await
        .unwrap();
    let notes = format!("staging/{}/part/notes", session.session_id.as_str());
    write_store(&root, &notes, b"foreign").await;
    let object_key = session.object_key.as_str().to_owned();
    let part_key = format!("staging/{}/part/00001", session.session_id.as_str());
    assert_eq!(
        host.complete(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::Checksum
    );
    assert!(fetch(&root, &object_key).await.is_none());
    assert!(fetch(&root, &part_key).await.is_none());
    assert_eq!(
        fetch(&root, &notes).await.as_deref(),
        Some(b"foreign".as_slice())
    );
    assert!(
        fetch(
            &root,
            &format!("meta/session/{}", session.session_id.as_str())
        )
        .await
        .is_none()
    );
    assert_eq!(
        host.read_manifest(&ctx, &session.object_key)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );
    let retried = host.begin(&ctx, &cfg).await.unwrap();
    assert_ne!(retried.session_id.as_str(), session.session_id.as_str());
    host.abort(&ctx, &retried.session_id).await.unwrap();

    let payload = b"abcd";
    let size_cfg = upload("size.bin", payload, None);
    let size_session = host.begin(&ctx, &size_cfg).await.unwrap();
    host.put_part(&ctx, &size_session.session_id, 1, payload)
        .await
        .unwrap();
    let size_part = format!("staging/{}/part/00001", size_session.session_id.as_str());
    let size_notes = format!("staging/{}/part/notes", size_session.session_id.as_str());
    write_store(&root, &size_part, b"abcde").await;
    write_store(&root, &size_notes, b"foreign-size").await;
    let size_object = size_session.object_key.as_str().to_owned();
    assert_eq!(
        host.complete(&ctx, &size_session.session_id)
            .await
            .unwrap_err(),
        FileHostError::Size
    );
    assert!(fetch(&root, &size_object).await.is_none());
    assert!(fetch(&root, &size_part).await.is_none());
    assert_eq!(
        fetch(&root, &size_notes).await.as_deref(),
        Some(b"foreign-size".as_slice())
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");
    assert_eq!(
        host.read_manifest(&ctx, &size_session.object_key)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );
}

#[tokio::test]
async fn abort_releases_quota_and_leaves_foreign_bytes() {
    let scratch = Scratch::new("abort");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::try_new(32, 32, 4, 10).unwrap()).unwrap();
    let ctx = admission("project_q", "user_q", "grant-q");
    let first_bytes = [1u8; 8];
    let second_bytes = [2u8; 8];
    let first_cfg = upload("a.bin", &first_bytes, None);
    let second_cfg = upload("b.bin", &second_bytes, None);
    let session = host.begin(&ctx, &first_cfg).await.unwrap();
    write_store(
        &root,
        &format!("meta/quota/{}/notes", ctx.project_id.as_str()),
        b"999999",
    )
    .await;
    let resumed = host.begin(&ctx, &first_cfg).await.unwrap();
    assert_eq!(resumed.session_id.as_str(), session.session_id.as_str());
    // 8 reserved plus 1 is still under the project ceiling. Counting the foreign
    // `notes` body would refuse this begin.
    let tiny = upload("c.bin", b"z", None);
    let tiny_session = host.begin(&ctx, &tiny).await.unwrap();
    host.abort(&ctx, &tiny_session.session_id).await.unwrap();
    assert_eq!(
        host.begin(&ctx, &second_cfg).await.unwrap_err(),
        FileHostError::Quota
    );
    host.put_part(&ctx, &session.session_id, 1, &first_bytes)
        .await
        .unwrap();
    let notes = format!("staging/{}/part/notes", session.session_id.as_str());
    let part_key = format!("staging/{}/part/00001", session.session_id.as_str());
    write_store(&root, &notes, b"foreign").await;
    host.abort(&ctx, &session.session_id).await.unwrap();
    assert_eq!(
        host.abort(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::NotFound
    );
    assert!(fetch(&root, &part_key).await.is_none());
    assert_eq!(
        fetch(&root, &notes).await.as_deref(),
        Some(b"foreign".as_slice())
    );
    assert_eq!(
        host.read_manifest(&ctx, &session.object_key)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );

    let finished = host.begin(&ctx, &second_cfg).await.unwrap();
    host.put_part(&ctx, &finished.session_id, 1, &second_bytes)
        .await
        .unwrap();
    let manifest = host.complete(&ctx, &finished.session_id).await.unwrap();
    assert_eq!(
        host.abort(&ctx, &finished.session_id).await.unwrap_err(),
        FileHostError::Closed
    );
    let stored = host.read_object(&ctx, &manifest.object_key).await.unwrap();
    assert_eq!(stored.bytes, second_bytes);
    assert_eq!(
        host.begin(&ctx, &first_cfg).await.unwrap_err(),
        FileHostError::Quota
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");
    assert_eq!(
        fetch(
            &root,
            &format!("meta/quota/{}/notes", ctx.project_id.as_str())
        )
        .await
        .as_deref(),
        Some(b"999999".as_slice())
    );
}

#[tokio::test]
async fn limits_and_names_are_refused_before_write() {
    assert_eq!(
        HostLimits::try_new(MAX_OBJECT_BYTES + 1, 1, 1, 1).unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        HostLimits::try_new(0, 1, 1, 1).unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        HostLimits::try_new(4, 5, 1, 1).unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        HostLimits::try_new(4, 4, 0, 1).unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        HostLimits::try_new(4, 4, MAX_PART_COUNT + 1, 1).unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        HostLimits::try_new(4, 4, 1, 0).unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(HostLimits::standard().max_object_bytes(), MAX_OBJECT_BYTES);
    assert_eq!(HostLimits::standard().max_part_count(), MAX_PART_COUNT);

    let scratch = Scratch::new("limits");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::try_new(8, 4, 4, 100).unwrap()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a");
    for name in ["../secret", "a/b", "a\\b", ""] {
        let cfg = UploadConfig {
            display_name: name.to_owned(),
            expected_sha256: digest(b"12345678"),
            expected_size: 8,
            content_type: None,
        };
        assert_eq!(
            host.begin(&ctx, &cfg).await.unwrap_err(),
            FileHostError::Admission
        );
    }
    let bad_type = UploadConfig {
        display_name: "ok.bin".to_owned(),
        expected_sha256: digest(b"12345678"),
        expected_size: 8,
        content_type: Some("text/..".to_owned()),
    };
    assert_eq!(
        host.begin(&ctx, &bad_type).await.unwrap_err(),
        FileHostError::Admission
    );
    let empty = UploadConfig {
        display_name: "ok.bin".to_owned(),
        expected_sha256: digest(b"12345678"),
        expected_size: 0,
        content_type: None,
    };
    assert_eq!(
        host.begin(&ctx, &empty).await.unwrap_err(),
        FileHostError::Limit
    );
    assert!(object_keys(&root).await.is_empty());
    assert_eq!(ProjectId::new("Bad").unwrap_err(), FileHostError::Admission);
    assert_eq!(
        PrincipalId::new("user/a").unwrap_err(),
        FileHostError::Admission
    );
    assert_eq!(
        GrantEvidence::new("").unwrap_err(),
        FileHostError::Admission
    );

    let cfg = upload("ok.bin", b"12345678", Some("text/plain"));
    let session = host.begin(&ctx, &cfg).await.unwrap();
    assert_eq!(
        host.put_part(&ctx, &session.session_id, 1, b"12345")
            .await
            .unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        host.put_part(&ctx, &session.session_id, 0, b"1234")
            .await
            .unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        host.put_part(&ctx, &session.session_id, 5, b"1234")
            .await
            .unwrap_err(),
        FileHostError::Limit
    );
    assert!(
        fetch(
            &root,
            &format!("staging/{}/part/00001", session.session_id.as_str())
        )
        .await
        .is_none()
    );
    host.put_part(&ctx, &session.session_id, 1, b"1234")
        .await
        .unwrap();
    assert_eq!(
        host.complete(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::Incomplete
    );
    assert!(fetch(&root, session.object_key.as_str()).await.is_none());
    assert_eq!(
        fetch(
            &root,
            &format!("staging/{}/part/00001", session.session_id.as_str())
        )
        .await
        .as_deref(),
        Some(b"1234".as_slice())
    );
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oo-a181-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn store(&self) -> PathBuf {
        self.root.join("store")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn admission(project: &str, principal: &str, grant: &str) -> AdmissionContext {
    AdmissionContext {
        project_id: ProjectId::new(project).unwrap(),
        principal_id: PrincipalId::new(principal).unwrap(),
        grant_evidence: GrantEvidence::new(grant).unwrap(),
    }
}

fn upload(name: &str, bytes: &[u8], content_type: Option<&str>) -> UploadConfig {
    UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: digest(bytes),
        expected_size: u64::try_from(bytes.len()).unwrap(),
        content_type: content_type.map(str::to_owned),
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let hash = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
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

fn feed(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update([0]);
}

fn idempotency_oracle(
    project: &str,
    principal: &str,
    sha: &[u8; 32],
    size: u64,
    display: &str,
    content_type: Option<&str>,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"oo.file-hosting/v1 idempotency\0");
    feed(&mut hasher, project);
    feed(&mut hasher, principal);
    feed(&mut hasher, &hex(sha));
    feed(&mut hasher, &size.to_string());
    feed(&mut hasher, display);
    feed(&mut hasher, content_type.unwrap_or(""));
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn lineage_oracle(manifest: &ObjectManifest) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"oo.file-hosting/v1 lineage\0");
    feed(&mut hasher, manifest.session_id.as_str());
    feed(&mut hasher, manifest.project_id.as_str());
    feed(&mut hasher, manifest.principal_id.as_str());
    feed(&mut hasher, manifest.grant_evidence.as_str());
    feed(&mut hasher, manifest.object_key.as_str());
    feed(&mut hasher, &hex(&manifest.object_digest));
    feed(&mut hasher, &manifest.size.to_string());
    feed(&mut hasher, &manifest.part_count.to_string());
    feed(&mut hasher, manifest.idempotency_key.as_str());
    feed(&mut hasher, &manifest.display_name);
    feed(&mut hasher, manifest.content_type.as_deref().unwrap_or(""));
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn open_raw(root: &Path) -> LocalFileSystem {
    LocalFileSystem::new_with_prefix(root).expect("raw store")
}

async fn fetch(root: &Path, key: &str) -> Option<Vec<u8>> {
    let store = open_raw(root);
    let path = ObjectPath::parse(key).expect("key");
    match store.get(&path).await {
        Ok(got) => Some(got.bytes().await.expect("bytes").to_vec()),
        Err(object_store::Error::NotFound { .. }) => None,
        Err(error) => panic!("store get failed: {error}"),
    }
}

async fn write_store(root: &Path, key: &str, bytes: &[u8]) {
    let store = open_raw(root);
    let path = ObjectPath::parse(key).expect("key");
    store
        .put(&path, bytes.to_vec().into())
        .await
        .expect("store put");
}

async fn object_keys(root: &Path) -> Vec<String> {
    let store = open_raw(root);
    let mut stream = store.list(None);
    let mut keys = Vec::new();
    while let Some(item) = stream.next().await {
        keys.push(item.expect("list").location.to_string());
    }
    keys.sort();
    keys
}
