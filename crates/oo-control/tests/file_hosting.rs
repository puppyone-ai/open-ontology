//! Local filesystem integration for upload sessions and the bounded download pull.
//!
//! These tests open a real [`object_store::local::LocalFileSystem`] under a private
//! scratch directory. They do not stand in for an S3 backend.

#![forbid(unsafe_code)]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::path::Path as StorePath;
use object_store::{
    Attributes, CopyOptions, GetOptions, GetRange, GetResult, GetResultPayload, ListResult,
    MultipartUpload, ObjectMeta, ObjectStore, ObjectStoreExt, PutMultipartOptions, PutOptions,
    PutPayload, PutResult,
};
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

#[tokio::test]
async fn generic_store_refuses_before_any_write() {
    let store = Arc::new(object_store::memory::InMemory::new());
    let host_store: Arc<dyn ObjectStore> = store.clone();
    let host = FileHost::new(host_store, HostLimits::try_new(16, 16, 4, 10).unwrap());
    let ctx = admission("project_a", "user_a", "grant-a");
    let cfg = upload("a.bin", b"12345678", None);
    let missing = SessionId::parse("ab".repeat(16)).unwrap();
    assert_eq!(
        host.begin(&ctx, &cfg).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        host.put_part(&ctx, &missing, 1, b"12345678")
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        host.complete(&ctx, &missing).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        host.abort(&ctx, &missing).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert!(store.list(None).next().await.is_none());
    let text = FileHostError::Unsupported.to_string();
    assert!(!text.contains('/'));
    assert!(!text.contains("memory"));
}

#[tokio::test]
async fn concurrent_quota_two_hosts_and_child() {
    if child_for("concurrent_quota_two_hosts_and_child") {
        quota_child().await;
        return;
    }
    let scratch = Scratch::new("quota-race");
    let root = scratch.store();
    let limits = HostLimits::try_new(64, 64, 4, 10).unwrap();
    let ctx = admission("project_q", "user_q", "grant-q");

    for round in 0..4 {
        let barrier = scratch.root.join(format!("barrier-round-{round}"));
        std::fs::create_dir_all(&barrier).unwrap();
        let payload_a = [1u8; 8];
        let payload_b = [2u8; 8];
        let name_a = format!("r{round}a.bin");
        let name_b = format!("r{round}b.bin");
        let root_a = root.clone();
        let root_b = root.clone();
        let barrier_a = barrier.clone();
        let barrier_b = barrier.clone();
        let name_a_task = name_a.clone();
        let task_a = tokio::spawn(async move {
            barrier_arrive(&barrier_a, "a").await;
            let host = FileHost::open_local(&root_a, limits).unwrap();
            host.begin(&ctx_q(), &upload(&name_a_task, &payload_a, None))
                .await
        });
        let task_b = tokio::spawn(async move {
            barrier_arrive(&barrier_b, "b").await;
            let host = FileHost::open_local(&root_b, limits).unwrap();
            host.begin(&ctx_q(), &upload(&name_b, &payload_b, None))
                .await
        });
        tokio::task::spawn_blocking(move || release_when_ready(&barrier, &["a", "b"]))
            .await
            .unwrap();
        let left = task_a.await.unwrap();
        let right = task_b.await.unwrap();
        let oks = [&left, &right].iter().filter(|item| item.is_ok()).count();
        assert_eq!(oks, 1, "round {round} left={left:?} right={right:?}");
        assert!(
            [&left, &right]
                .iter()
                .filter(|item| item.is_err())
                .all(|item| item.as_ref().unwrap_err() == &FileHostError::Quota),
            "round {round} left={left:?} right={right:?}"
        );
        assert_eq!(reserved_bytes(&root, "project_q").await, 8);
        let winner = left.ok().or(right.ok()).unwrap();
        let host = FileHost::open_local(&root, limits).unwrap();
        host.abort(&ctx, &winner.session_id).await.unwrap();
        assert_eq!(reserved_bytes(&root, "project_q").await, 0);
    }

    let barrier = scratch.root.join("barrier-child");
    std::fs::create_dir_all(&barrier).unwrap();
    let root_a = root.clone();
    let root_b = root.clone();
    let barrier_a = barrier.clone();
    let barrier_b = barrier.clone();
    let task_a = tokio::spawn(async move {
        barrier_arrive(&barrier_a, "a").await;
        let host = FileHost::open_local(&root_a, limits).unwrap();
        host.begin(&ctx_q(), &upload("pa.bin", &[4u8; 8], None))
            .await
    });
    let task_b = tokio::spawn(async move {
        barrier_arrive(&barrier_b, "b").await;
        let host = FileHost::open_local(&root_b, limits).unwrap();
        host.begin(&ctx_q(), &upload("pb.bin", &[5u8; 8], None))
            .await
    });
    let child = spawn_child(
        "concurrent_quota_two_hosts_and_child",
        &[
            ("OO_FH_KIND", "distinct"),
            ("OO_FH_ROOT", root.to_str().unwrap()),
            ("OO_FH_BARRIER", barrier.to_str().unwrap()),
            (
                "OO_FH_RESULT",
                barrier.join("result-child").to_str().unwrap(),
            ),
        ],
    );
    let barrier_wait = barrier.clone();
    tokio::task::spawn_blocking(move || release_when_ready(&barrier_wait, &["a", "b", "child"]))
        .await
        .unwrap();
    let left = task_a.await.unwrap();
    let right = task_b.await.unwrap();
    wait_child(child);
    let child_line = std::fs::read_to_string(barrier.join("result-child")).unwrap();
    let child_ok = child_line.starts_with("ok ");
    let oks = usize::from(left.is_ok()) + usize::from(right.is_ok()) + usize::from(child_ok);
    assert_eq!(oks, 1, "left={left:?} right={right:?} child={child_line}");
    for result in [&left, &right] {
        if let Err(error) = result {
            assert_eq!(*error, FileHostError::Quota);
        }
    }
    if !child_ok {
        assert_eq!(child_line, "err Quota");
    }
    assert!(reserved_bytes(&root, "project_q").await <= 10);
    assert_eq!(reserved_bytes(&root, "project_q").await, 8);
    assert!(scratch.root.join("store.oo-file-hosting.lock").is_file());
    assert!(
        object_keys(&root)
            .await
            .iter()
            .all(|key| !key.contains("oo-file-hosting"))
    );

    let winner = if let Ok(session) = &left {
        session.session_id.clone()
    } else if let Ok(session) = &right {
        session.session_id.clone()
    } else {
        let id = child_line.trim_start_matches("ok ").trim();
        SessionId::parse(id).unwrap()
    };
    let reopened = FileHost::open_local(&root, limits).unwrap();
    assert_eq!(
        reopened
            .begin(&ctx, &upload("later.bin", &[6u8; 8], None))
            .await
            .unwrap_err(),
        FileHostError::Quota
    );
    reopened.abort(&ctx, &winner).await.unwrap();
    assert_eq!(reserved_bytes(&root, "project_q").await, 0);
    let released = reopened
        .begin(&ctx, &upload("later.bin", &[6u8; 8], None))
        .await
        .unwrap();
    reopened.abort(&ctx, &released.session_id).await.unwrap();
    assert_eq!(reserved_bytes(&root, "project_q").await, 0);

    let barrier = scratch.root.join("barrier-same");
    std::fs::create_dir_all(&barrier).unwrap();
    let same = [7u8; 8];
    let root_a = root.clone();
    let root_b = root.clone();
    let barrier_a = barrier.clone();
    let barrier_b = barrier.clone();
    let task_a = tokio::spawn(async move {
        barrier_arrive(&barrier_a, "a").await;
        let host = FileHost::open_local(&root_a, limits).unwrap();
        host.begin(&ctx_q(), &upload("same.bin", &same, None)).await
    });
    let task_b = tokio::spawn(async move {
        barrier_arrive(&barrier_b, "b").await;
        let host = FileHost::open_local(&root_b, limits).unwrap();
        host.begin(&ctx_q(), &upload("same.bin", &same, None)).await
    });
    let child = spawn_child(
        "concurrent_quota_two_hosts_and_child",
        &[
            ("OO_FH_KIND", "same"),
            ("OO_FH_ROOT", root.to_str().unwrap()),
            ("OO_FH_BARRIER", barrier.to_str().unwrap()),
            (
                "OO_FH_RESULT",
                barrier.join("result-child").to_str().unwrap(),
            ),
        ],
    );
    let barrier_wait = barrier.clone();
    tokio::task::spawn_blocking(move || release_when_ready(&barrier_wait, &["a", "b", "child"]))
        .await
        .unwrap();
    let left = task_a.await.unwrap().unwrap();
    let right = task_b.await.unwrap().unwrap();
    wait_child(child);
    let child_line = std::fs::read_to_string(barrier.join("result-child")).unwrap();
    assert!(child_line.starts_with("ok "), "{child_line}");
    let child_id = child_line.trim_start_matches("ok ").trim();
    assert_eq!(left.session_id.as_str(), right.session_id.as_str());
    assert_eq!(left.session_id.as_str(), child_id);
    assert_eq!(reserved_bytes(&root, "project_q").await, 8);
    let host = FileHost::open_local(&root, limits).unwrap();
    host.abort(&ctx, &left.session_id).await.unwrap();
    assert_eq!(reserved_bytes(&root, "project_q").await, 0);
    let retried = host
        .begin(&ctx, &upload("same.bin", &same, None))
        .await
        .unwrap();
    assert_ne!(retried.session_id.as_str(), left.session_id.as_str());
    assert_eq!(reserved_bytes(&root, "project_q").await, 8);
    host.abort(&ctx, &retried.session_id).await.unwrap();
    assert_eq!(
        host.abort(&ctx, &retried.session_id).await.unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(reserved_bytes(&root, "project_q").await, 0);
}

#[tokio::test]
async fn concurrent_complete_and_part_keep_one_object() {
    if child_for("concurrent_complete_and_part_keep_one_object") {
        conflict_child().await;
        return;
    }
    let scratch = Scratch::new("race-complete");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a");
    let bytes = b"abcdefgh";
    let session = host
        .begin(&ctx, &upload("race.bin", bytes, None))
        .await
        .unwrap();
    host.put_part(&ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    let notes = format!("staging/{}/part/notes", session.session_id.as_str());
    write_store(&root, &notes, b"foreign").await;
    write_store(
        &root,
        &format!("meta/quota/{}/notes", ctx.project_id.as_str()),
        b"999999",
    )
    .await;
    let session_text = session.session_id.as_str().to_owned();
    let key_text = session.object_key.as_str().to_owned();
    drop(host);

    let barrier = scratch.root.join("barrier-complete");
    std::fs::create_dir_all(&barrier).unwrap();
    let root_a = root.clone();
    let root_b = root.clone();
    let barrier_a = barrier.clone();
    let barrier_b = barrier.clone();
    let session_a = session_text.clone();
    let session_b = session_text.clone();
    let task_a = tokio::spawn(async move {
        barrier_arrive(&barrier_a, "a").await;
        let host = FileHost::open_local(&root_a, HostLimits::standard()).unwrap();
        host.complete(&ctx_a(), &SessionId::parse(&session_a).unwrap())
            .await
    });
    let task_b = tokio::spawn(async move {
        barrier_arrive(&barrier_b, "b").await;
        let host = FileHost::open_local(&root_b, HostLimits::standard()).unwrap();
        host.complete(&ctx_a(), &SessionId::parse(&session_b).unwrap())
            .await
    });
    let child = spawn_child(
        "concurrent_complete_and_part_keep_one_object",
        &[
            ("OO_FH_KIND", "conflict"),
            ("OO_FH_ROOT", root.to_str().unwrap()),
            ("OO_FH_BARRIER", barrier.to_str().unwrap()),
            (
                "OO_FH_RESULT",
                barrier.join("result-child").to_str().unwrap(),
            ),
            ("OO_FH_SESSION", session_text.as_str()),
        ],
    );
    let barrier_wait = barrier.clone();
    tokio::task::spawn_blocking(move || release_when_ready(&barrier_wait, &["a", "b", "child"]))
        .await
        .unwrap();
    let left = task_a.await.unwrap().unwrap();
    let right = task_b.await.unwrap().unwrap();
    wait_child(child);
    assert_eq!(left, right);
    let child_line = std::fs::read_to_string(barrier.join("result-child")).unwrap();
    assert!(
        child_line == "err Conflict" || child_line == "err Closed",
        "{child_line}"
    );
    assert_eq!(
        fetch(&root, &key_text).await.as_deref(),
        Some(bytes.as_slice())
    );
    assert!(
        fetch(&root, &format!("staging/{session_text}/part/00001"))
            .await
            .is_none()
    );
    assert_eq!(
        fetch(&root, &notes).await.as_deref(),
        Some(b"foreign".as_slice())
    );
    assert_eq!(
        fetch(&root, "meta/quota/project_a/notes").await.as_deref(),
        Some(b"999999".as_slice())
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let stored = host.read_object(&ctx, &left.object_key).await.unwrap();
    assert_eq!(stored.bytes, bytes);
    assert_eq!(stored.manifest, left);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn streaming_complete_memory_is_bounded() {
    if child_for("streaming_complete_memory_is_bounded") {
        memory_child().await;
        return;
    }
    for size in [8 * 1024 * 1024_usize, 32 * 1024 * 1024] {
        let scratch = Scratch::new(&format!("mem-{size}"));
        let root = scratch.store();
        std::fs::create_dir_all(&root).unwrap();
        let metrics = scratch.root.join("metrics.txt");
        let child = spawn_child(
            "streaming_complete_memory_is_bounded",
            &[
                ("OO_FH_KIND", "memory"),
                ("OO_FH_ROOT", root.to_str().unwrap()),
                ("OO_FH_METRICS", metrics.to_str().unwrap()),
                ("OO_FH_SIZE", &size.to_string()),
            ],
        );
        wait_child(child);
        let text = std::fs::read_to_string(&metrics).unwrap();
        println!("{text}");
        if let Some(dir) = std::env::var_os("OO_FH_METRICS_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(Path::new(&dir).join(format!("memory-{size}.txt")), &text).unwrap();
        }
        let delta = metric_u64(&text, "memory_hwm_delta_bytes");
        assert!(
            delta.saturating_mul(2) < size as u64,
            "hwm delta {delta} part {size}\n{text}"
        );
        assert!(text.contains("memory_vmpeak_delta_bytes="));
        assert!(text.contains("memory_threads_before="));
        assert!(text.contains("memory_ok=1"));
    }
}

#[tokio::test]
async fn download_pull_verifies_identity_without_an_object_buffer() {
    let scratch = Scratch::new("download-pull");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-download");
    let bytes = b"abcdefghij0123456789";
    let manifest = publish(&host, &ctx, "pull.bin", bytes).await;
    let missing = ObjectKey::parse(format!(
        "uploads/{}/{}/original",
        "ab".repeat(16),
        "cd".repeat(32)
    ))
    .unwrap();
    assert_eq!(
        host.open_download(&ctx, &missing, 0, None)
            .await
            .unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        host.open_download(&ctx, &manifest.object_key, 8193, None)
            .await
            .unwrap_err(),
        FileHostError::Limit
    );
    assert_eq!(
        ObjectKey::parse("../secret").unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        ObjectKey::parse("/etc/passwd").unwrap_err(),
        FileHostError::NotFound
    );

    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    let shown = format!("{reader:?}");
    assert!(!shown.contains(ctx.grant_evidence.as_str()));
    assert!(!shown.contains(root.to_str().unwrap()));
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &manifest);
    let mut hasher = Sha256::new();
    let mut pulled = 0usize;
    let mut widths = Vec::new();
    loop {
        assert_eq!(reader.manifest(), &manifest);
        match reader.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(!chunk.is_empty() && chunk.len() <= 8);
                assert!(!reader.is_complete());
                hasher.update(&chunk);
                pulled += chunk.len();
                widths.push(chunk.len());
            }
            None => break,
        }
    }
    assert_eq!(widths, vec![8, 8, 4]);
    assert_eq!(pulled, bytes.len());
    let pulled_hash = hasher.finalize();
    assert_eq!(&pulled_hash[..], &manifest.object_digest);
    assert!(reader.is_complete());
    assert_eq!(reader.manifest(), &manifest);
    assert!(reader.next_chunk().await.unwrap().is_none());
    assert!(reader.is_complete());
    let stored = host.read_object(&ctx, &manifest.object_key).await.unwrap();
    assert_eq!(stored.bytes, bytes);
    assert_eq!(stored.manifest, manifest);

    let mut early = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    let first = early.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.len(), 8);
    assert!(!early.is_complete());
    drop(early);

    let mut cancelling = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    assert!(cancelling.next_chunk().await.unwrap().is_some());
    cancelling.cancel();
    let stopped = cancelling.next_chunk().await.unwrap_err();
    assert_eq!(stopped, FileHostError::Stopped);
    assert_eq!(
        stopped.to_string(),
        "download stopped before the object was verified"
    );
    assert!(!cancelling.is_complete());
    assert_eq!(cancelling.manifest(), &manifest);

    let mut expired = host
        .open_download(
            &ctx,
            &manifest.object_key,
            8,
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .await
        .unwrap();
    assert_eq!(
        expired.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!expired.is_complete());

    let mut timed = host
        .open_download(
            &ctx,
            &manifest.object_key,
            8,
            Some(Instant::now() + Duration::from_secs(1)),
        )
        .await
        .unwrap();
    assert!(timed.next_chunk().await.unwrap().is_some());
    assert!(!timed.is_complete());
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        timed.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!timed.is_complete());

    let mut polled = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    let pending = {
        let fut = polled.next_chunk();
        tokio::pin!(fut);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        std::future::Future::poll(fut.as_mut(), &mut cx).is_pending()
    };
    if pending {
        assert_eq!(
            polled.next_chunk().await.unwrap_err(),
            FileHostError::Stopped
        );
    }
    assert!(!polled.is_complete());
}

#[tokio::test]
async fn download_pull_refuses_other_context_and_altered_bytes() {
    let scratch = Scratch::new("download-refuse");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-owner");
    let other = admission("project_b", "user_b", "grant-other");
    let bytes = b"owner-bytes-0123456789";
    let manifest = publish(&host, &ctx, "owned.bin", bytes).await;
    let object_path = root.join(manifest.object_key.as_str());
    let before = std::fs::read(&object_path).unwrap();
    let denied = host
        .open_download(&other, &manifest.object_key, 8192, None)
        .await
        .unwrap_err();
    assert_eq!(denied, FileHostError::Denied);
    assert!(!denied.to_string().contains('/'));
    assert_eq!(std::fs::read(&object_path).unwrap(), before);

    let tampered = publish(&host, &ctx, "tamper.bin", bytes).await;
    let tamper_path = root.join(tampered.object_key.as_str());
    let mut raw = std::fs::read(&tamper_path).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 0x5a;
    std::fs::write(&tamper_path, &raw).unwrap();
    let mut reader = host
        .open_download(&ctx, &tampered.object_key, 8, None)
        .await
        .unwrap();
    let mut saw_chunk = false;
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(chunk)) => {
                assert!(!chunk.is_empty());
                saw_chunk = true;
                assert!(!reader.is_complete());
                assert_eq!(reader.manifest(), &tampered);
            }
            Ok(None) => panic!("tampered object was reported complete"),
            Err(error) => break error,
        }
    };
    assert!(saw_chunk);
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &tampered);

    let truncated = publish(&host, &ctx, "empty-file.bin", bytes).await;
    let truncated_path = root.join(truncated.object_key.as_str());
    std::fs::OpenOptions::new()
        .write(true)
        .open(&truncated_path)
        .unwrap()
        .set_len(0)
        .unwrap();
    assert_eq!(
        host.open_download(&ctx, &truncated.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::Corrupt
    );

    let mid = publish(&host, &ctx, "mid-cut.bin", bytes).await;
    let mut mid_reader = host
        .open_download(&ctx, &mid.object_key, 8, None)
        .await
        .unwrap();
    assert!(mid_reader.next_chunk().await.unwrap().is_some());
    std::fs::OpenOptions::new()
        .write(true)
        .open(root.join(mid.object_key.as_str()))
        .unwrap()
        .set_len(0)
        .unwrap();
    let mid_err = mid_reader.next_chunk().await.unwrap_err();
    assert!(matches!(
        mid_err,
        FileHostError::Store | FileHostError::Corrupt
    ));
    assert!(!mid_err.to_string().contains(root.to_str().unwrap()));
    assert!(!mid_reader.is_complete());

    let edited = publish(&host, &ctx, "manifest.bin", bytes).await;
    let manifest_path = root.join(format!("meta/manifest/{}", edited.session_id.as_str()));
    assert_eq!(
        std::fs::read(&manifest_path).unwrap(),
        encode_manifest_oracle(&edited)
    );
    let mut reader = host
        .open_download(&ctx, &edited.object_key, 8, None)
        .await
        .unwrap();
    let mut manifest_bytes = std::fs::read(&manifest_path).unwrap();
    manifest_bytes[20] ^= 0x01;
    std::fs::write(&manifest_path, &manifest_bytes).unwrap();
    let manifest_err = loop {
        match reader.next_chunk().await {
            Ok(Some(_)) => assert!(!reader.is_complete()),
            Ok(None) => panic!("altered manifest was reported complete"),
            Err(error) => break error,
        }
    };
    assert_eq!(manifest_err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &edited);

    let zeroed = publish(&host, &ctx, "zero-manifest.bin", bytes).await;
    let zero_path = root.join(format!("meta/manifest/{}", zeroed.session_id.as_str()));
    let mut forged = zeroed.clone();
    forged.size = 0;
    forged.lineage_digest = lineage_oracle(&forged);
    std::fs::write(&zero_path, encode_manifest_oracle(&forged)).unwrap();
    assert_eq!(
        host.open_download(&ctx, &zeroed.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::Corrupt
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn download_pull_memory_is_bounded() {
    if child_for("download_pull_memory_is_bounded") {
        download_memory_child().await;
        return;
    }
    for size in [8 * 1024 * 1024_usize, 32 * 1024 * 1024] {
        let scratch = Scratch::new(&format!("download-mem-{size}"));
        let root = scratch.store();
        std::fs::create_dir_all(&root).unwrap();
        let metrics = scratch.root.join("metrics.txt");
        let child = spawn_child(
            "download_pull_memory_is_bounded",
            &[
                ("OO_FH_ROOT", root.to_str().unwrap()),
                ("OO_FH_METRICS", metrics.to_str().unwrap()),
                ("OO_FH_SIZE", &size.to_string()),
            ],
        );
        wait_child(child);
        let text = std::fs::read_to_string(&metrics).unwrap();
        println!("{text}");
        if let Some(dir) = std::env::var_os("OO_FH_METRICS_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                Path::new(&dir).join(format!("download-memory-{size}.txt")),
                &text,
            )
            .unwrap();
        }
        let delta = metric_u64(&text, "download_hwm_delta_bytes");
        assert!(delta < 4 * 1024 * 1024, "hwm delta {delta}\n{text}");
        assert!(delta.saturating_mul(2) < size as u64, "{text}");
        let object_key = metric_str(&text, "object_key");
        let file_hash = hash_file(&root.join(&object_key), 8192);
        let pattern = pattern_digest(size);
        assert_eq!(hex(&file_hash), metric_str(&text, "download_sha256"));
        assert_eq!(hex(&pattern), metric_str(&text, "pattern_sha256"));
        assert_eq!(
            metric_str(&text, "download_sha256"),
            metric_str(&text, "file_sha256")
        );
        assert_eq!(metric_u64(&text, "download_chunk_max"), 8192);
        assert_eq!(metric_u64(&text, "download_pulled"), size as u64);
        assert_eq!(metric_u64(&text, "download_complete"), 1);
        assert!(text.contains("fixture_object_vec=0"));
        assert!(text.contains("caller_retained_chunks=1"));
    }
}

#[tokio::test]
async fn download_eof_rejects_flipped_bytes_and_keeps_a_clean_object() {
    let scratch = Scratch::new("download-eof");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-eof");
    let source = b"abcdefghij0123456789".to_vec();
    let independent = digest(&source);

    let clean = publish(&host, &ctx, "clean.bin", &source).await;
    let clean_path = root.join(clean.object_key.as_str());
    let mut reader = host
        .open_download(&ctx, &clean.object_key, 8, None)
        .await
        .unwrap();
    let mut returned = Sha256::new();
    let mut offset = 0usize;
    loop {
        assert_eq!(reader.manifest(), &clean);
        match reader.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(!chunk.is_empty() && chunk.len() <= 8);
                assert!(!reader.is_complete());
                assert_eq!(&source[offset..offset + chunk.len()], chunk.as_slice());
                returned.update(&chunk);
                offset += chunk.len();
            }
            None => break,
        }
    }
    assert_eq!(offset, source.len());
    assert_eq!(finish_sha(returned), independent);
    assert_eq!(digest(&std::fs::read(&clean_path).unwrap()), independent);
    assert_eq!(clean.object_digest, independent);
    assert!(reader.is_complete());
    let mut raced = std::fs::read(&clean_path).unwrap();
    raced[0] ^= 0xff;
    std::fs::write(&clean_path, &raced).unwrap();
    assert!(reader.is_complete());
    assert!(reader.next_chunk().await.unwrap().is_none());
    assert!(reader.is_complete());
    assert_eq!(reader.manifest(), &clean);
    assert_ne!(digest(&std::fs::read(&clean_path).unwrap()), independent);

    let consumed = publish(&host, &ctx, "consumed.bin", &source).await;
    let consumed_path = root.join(consumed.object_key.as_str());
    let mut reader = host
        .open_download(&ctx, &consumed.object_key, 8, None)
        .await
        .unwrap();
    let first = reader.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.as_slice(), &source[..8]);
    assert!(!reader.is_complete());
    let mut edited = std::fs::read(&consumed_path).unwrap();
    edited[0] ^= 0xff;
    std::fs::write(&consumed_path, &edited).unwrap();
    let mut returned = Sha256::new();
    returned.update(&first);
    let mut offset = first.len();
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(chunk)) => {
                assert!(!reader.is_complete());
                returned.update(&chunk);
                offset += chunk.len();
            }
            Ok(None) => panic!("consumed prefix flip completed"),
            Err(error) => break error,
        }
    };
    assert_eq!(offset, source.len());
    assert_eq!(finish_sha(returned), independent);
    assert_ne!(digest(&std::fs::read(&consumed_path).unwrap()), independent);
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &consumed);

    let unread = publish(&host, &ctx, "unread.bin", &source).await;
    let unread_path = root.join(unread.object_key.as_str());
    let mut reader = host
        .open_download(&ctx, &unread.object_key, 8, None)
        .await
        .unwrap();
    assert!(reader.next_chunk().await.unwrap().is_some());
    let mut edited = std::fs::read(&unread_path).unwrap();
    let last = edited.len() - 1;
    edited[last] ^= 0x5a;
    std::fs::write(&unread_path, &edited).unwrap();
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(_)) => assert!(!reader.is_complete()),
            Ok(None) => panic!("unread byte flip completed"),
            Err(error) => break error,
        }
    };
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_ne!(digest(&std::fs::read(&unread_path).unwrap()), independent);
    assert_eq!(reader.manifest(), &unread);
}

#[tokio::test]
async fn download_deadline_does_not_start_bytes_after_held_get() {
    let scratch = Scratch::new("download-held");
    let root = scratch.store();
    let publisher = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-held");
    let source = b"abcdefghij0123456789".to_vec();
    let manifest = publish(&publisher, &ctx, "held.bin", &source).await;
    let (store, gate) = CountingStore::open(&root);
    let host = FileHost::new(store, HostLimits::standard());

    gate.hold_get.store(true, Ordering::SeqCst);
    gate.hold_bytes.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 8, Some(deadline))
        .await
        .unwrap();
    let stopped = {
        let mut fut = std::pin::pin!(reader.next_chunk());
        poll_until(&mut fut, || gate.entered_get.load(Ordering::SeqCst));
        assert!(!gate.entered_bytes.load(Ordering::SeqCst));
        assert!(!gate.done_get.load(Ordering::SeqCst));
        while Instant::now() < deadline + Duration::from_millis(50) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !gate.dropped_get.load(Ordering::SeqCst),
            "deadline dropped the in-flight get_opts"
        );
        assert!(!gate.entered_bytes.load(Ordering::SeqCst));
        gate.release_get.store(true, Ordering::SeqCst);
        let stopped = poll_ready(&mut fut).unwrap_err();
        assert!(
            !gate.entered_bytes.load(Ordering::SeqCst),
            "bytes() started after the deadline"
        );
        stopped
    };
    assert_eq!(stopped, FileHostError::Stopped);
    assert_eq!(
        stopped.to_string(),
        "download stopped before the object was verified"
    );
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &manifest);
    gate.hold_get.store(false, Ordering::SeqCst);
    gate.hold_bytes.store(false, Ordering::SeqCst);
    assert_eq!(
        reader.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!reader.is_complete());

    reset_gate(&gate);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, source.len(), Some(deadline))
        .await
        .unwrap();
    let chunk = reader.next_chunk().await.unwrap().unwrap();
    assert_eq!(chunk.as_slice(), source.as_slice());
    assert!(!reader.is_complete());
    gate.hold_get.store(true, Ordering::SeqCst);
    gate.hold_bytes.store(true, Ordering::SeqCst);
    let stopped = {
        let mut fut = std::pin::pin!(reader.next_chunk());
        poll_until(&mut fut, || gate.entered_get.load(Ordering::SeqCst));
        assert!(!gate.entered_bytes.load(Ordering::SeqCst));
        while Instant::now() < deadline + Duration::from_millis(50) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!gate.dropped_get.load(Ordering::SeqCst));
        assert!(!gate.entered_bytes.load(Ordering::SeqCst));
        gate.release_get.store(true, Ordering::SeqCst);
        let stopped = poll_ready(&mut fut).unwrap_err();
        assert!(
            !gate.entered_bytes.load(Ordering::SeqCst),
            "EOF rehash started bytes() after the deadline"
        );
        stopped
    };
    assert_eq!(stopped, FileHostError::Stopped);
    assert!(!reader.is_complete());

    reset_gate(&gate);
    gate.hold_get.store(true, Ordering::SeqCst);
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    {
        let mut fut = std::pin::pin!(reader.next_chunk());
        poll_until(&mut fut, || gate.entered_get.load(Ordering::SeqCst));
        assert!(!gate.done_get.load(Ordering::SeqCst));
    }
    assert!(gate.dropped_get.load(Ordering::SeqCst));
    assert!(!gate.done_get.load(Ordering::SeqCst));
    gate.release_get.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while !gate.done_get.load(Ordering::SeqCst) {
        if start.elapsed() > Duration::from_secs(5) {
            panic!("in-flight get_opts did not finish after release");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!reader.is_complete());
    assert_eq!(
        reader.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
}

#[tokio::test]
async fn download_regressions_deadline_cancel_manifest_truncate_quota() {
    let scratch = Scratch::new("download-regress");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-regress");
    let source = b"abcdefghij0123456789".to_vec();
    let manifest = publish(&host, &ctx, "regress.bin", &source).await;
    let quota_path = root.join(format!(
        "meta/quota/{}/{}",
        ctx.project_id.as_str(),
        manifest.session_id.as_str()
    ));
    let quota_before = std::fs::read(&quota_path).unwrap();

    let mut expired = host
        .open_download(
            &ctx,
            &manifest.object_key,
            8,
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .await
        .unwrap();
    let stopped = expired.next_chunk().await.unwrap_err();
    assert_eq!(stopped, FileHostError::Stopped);
    assert_eq!(
        stopped.to_string(),
        "download stopped before the object was verified"
    );
    assert!(!expired.is_complete());

    let mut cancelling = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    assert!(cancelling.next_chunk().await.unwrap().is_some());
    cancelling.cancel();
    assert_eq!(
        cancelling.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!cancelling.is_complete());
    assert_eq!(cancelling.manifest(), &manifest);

    let edited = publish(&host, &ctx, "manifest.bin", &source).await;
    let manifest_path = root.join(format!("meta/manifest/{}", edited.session_id.as_str()));
    let mut reader = host
        .open_download(&ctx, &edited.object_key, 8, None)
        .await
        .unwrap();
    let mut manifest_bytes = std::fs::read(&manifest_path).unwrap();
    manifest_bytes[20] ^= 0x01;
    std::fs::write(&manifest_path, &manifest_bytes).unwrap();
    let manifest_err = loop {
        match reader.next_chunk().await {
            Ok(Some(_)) => assert!(!reader.is_complete()),
            Ok(None) => panic!("changed manifest completed"),
            Err(error) => break error,
        }
    };
    assert_eq!(manifest_err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &edited);

    let cut = publish(&host, &ctx, "cut.bin", &source).await;
    let mut reader = host
        .open_download(&ctx, &cut.object_key, 8, None)
        .await
        .unwrap();
    assert!(reader.next_chunk().await.unwrap().is_some());
    std::fs::OpenOptions::new()
        .write(true)
        .open(root.join(cut.object_key.as_str()))
        .unwrap()
        .set_len(0)
        .unwrap();
    let cut_err = reader.next_chunk().await.unwrap_err();
    assert!(matches!(
        cut_err,
        FileHostError::Store | FileHostError::Corrupt
    ));
    assert!(!cut_err.to_string().contains('/'));
    assert!(!reader.is_complete());

    let flipped = publish(&host, &ctx, "generic-flip.bin", &source).await;
    let names_before = quota_names(&root, ctx.project_id.as_str());
    let generic_store = Arc::new(LocalFileSystem::new_with_prefix(&root).unwrap());
    let generic = FileHost::new(generic_store, HostLimits::standard());
    let fresh = upload("other.bin", b"zzzzzzzz", None);
    assert_eq!(
        generic.begin(&ctx, &fresh).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        generic
            .put_part(&ctx, &manifest.session_id, 1, b"zzzzzzzz")
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        generic
            .complete(&ctx, &manifest.session_id)
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        generic.abort(&ctx, &manifest.session_id).await.unwrap_err(),
        FileHostError::Unsupported
    );
    let mut reader = generic
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    let mut returned = Sha256::new();
    let mut offset = 0usize;
    while let Some(chunk) = reader.next_chunk().await.unwrap() {
        assert!(!reader.is_complete());
        assert!(chunk.len() <= 8);
        returned.update(&chunk);
        offset += chunk.len();
    }
    assert_eq!(offset, source.len());
    assert!(reader.is_complete());
    assert_eq!(finish_sha(returned), digest(&source));
    assert_eq!(
        digest(&std::fs::read(root.join(manifest.object_key.as_str())).unwrap()),
        digest(&source)
    );
    let flip_path = root.join(flipped.object_key.as_str());
    let mut reader = generic
        .open_download(&ctx, &flipped.object_key, 8, None)
        .await
        .unwrap();
    assert!(reader.next_chunk().await.unwrap().is_some());
    let mut raw = std::fs::read(&flip_path).unwrap();
    raw[0] ^= 0xff;
    std::fs::write(&flip_path, &raw).unwrap();
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(_)) => assert!(!reader.is_complete()),
            Ok(None) => panic!("generic consumed-prefix flip completed"),
            Err(error) => break error,
        }
    };
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(std::fs::read(&quota_path).unwrap(), quota_before);
    assert_eq!(quota_names(&root, ctx.project_id.as_str()), names_before);
}

fn finish_sha(hasher: Sha256) -> [u8; 32] {
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn quota_names(root: &Path, project: &str) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(root.join(format!("meta/quota/{project}"))).unwrap() {
        names.push(entry.unwrap().file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

fn reset_gate(gate: &HoldGate) {
    gate.hold_get.store(false, Ordering::SeqCst);
    gate.hold_bytes.store(false, Ordering::SeqCst);
    gate.release_get.store(false, Ordering::SeqCst);
    gate.release_bytes.store(false, Ordering::SeqCst);
    gate.entered_get.store(false, Ordering::SeqCst);
    gate.entered_bytes.store(false, Ordering::SeqCst);
    gate.done_get.store(false, Ordering::SeqCst);
    gate.done_bytes.store(false, Ordering::SeqCst);
    gate.dropped_get.store(false, Ordering::SeqCst);
    gate.dropped_bytes.store(false, Ordering::SeqCst);
}

fn poll_until<F: Future>(fut: &mut Pin<&mut F>, pred: impl Fn() -> bool) {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let start = Instant::now();
    loop {
        if pred() {
            return;
        }
        match fut.as_mut().poll(&mut context) {
            Poll::Pending => {}
            Poll::Ready(_) => panic!("download future finished before the hold was entered"),
        }
        if start.elapsed() > Duration::from_secs(20) {
            panic!("timed out waiting for the held operation");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn poll_ready<F: Future>(fut: &mut Pin<&mut F>) -> F::Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let start = Instant::now();
    loop {
        match fut.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => {
                if start.elapsed() > Duration::from_secs(30) {
                    panic!("timed out waiting for the held download");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn child_for(test_name: &str) -> bool {
    std::env::var_os("OO_FH_CHILD").is_some()
        && std::env::var("OO_FH_TEST").as_deref() == Ok(test_name)
}

fn ctx_q() -> AdmissionContext {
    admission("project_q", "user_q", "grant-q")
}

fn ctx_a() -> AdmissionContext {
    admission("project_a", "user_a", "grant-a")
}

async fn quota_child() {
    let root = PathBuf::from(std::env::var("OO_FH_ROOT").unwrap());
    let barrier = PathBuf::from(std::env::var("OO_FH_BARRIER").unwrap());
    let result = PathBuf::from(std::env::var("OO_FH_RESULT").unwrap());
    let kind = std::env::var("OO_FH_KIND").unwrap();
    barrier_arrive(&barrier, "child").await;
    let host = FileHost::open_local(&root, HostLimits::try_new(64, 64, 4, 10).unwrap()).unwrap();
    let begun = match kind.as_str() {
        "distinct" => {
            host.begin(&ctx_q(), &upload("pc.bin", &[3u8; 8], None))
                .await
        }
        "same" => {
            host.begin(&ctx_q(), &upload("same.bin", &[7u8; 8], None))
                .await
        }
        other => panic!("quota child kind {other}"),
    };
    write_begin_result(&result, begun);
}

async fn conflict_child() {
    let root = PathBuf::from(std::env::var("OO_FH_ROOT").unwrap());
    let barrier = PathBuf::from(std::env::var("OO_FH_BARRIER").unwrap());
    let result = PathBuf::from(std::env::var("OO_FH_RESULT").unwrap());
    let session = SessionId::parse(std::env::var("OO_FH_SESSION").unwrap()).unwrap();
    barrier_arrive(&barrier, "child").await;
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let begun = host
        .put_part(&ctx_a(), &session, 1, b"xxxxxxxx")
        .await
        .map(|_| ());
    let line = match begun {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("err {error:?}"),
    };
    std::fs::write(result, line).unwrap();
}

fn pattern_digest(size: usize) -> [u8; 32] {
    let chunk = [0xA5_u8; 8192];
    let mut hasher = Sha256::new();
    let mut left = size;
    while left > 0 {
        let n = 8192.min(left);
        hasher.update(&chunk[..n]);
        left -= n;
    }
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

#[cfg(target_os = "linux")]
async fn download_memory_child() {
    let root = PathBuf::from(std::env::var("OO_FH_ROOT").unwrap());
    let metrics_path = PathBuf::from(std::env::var("OO_FH_METRICS").unwrap());
    let size: usize = std::env::var("OO_FH_SIZE").unwrap().parse().unwrap();
    assert!(size == 8 * 1024 * 1024 || size == 32 * 1024 * 1024);
    let digest_bytes = pattern_digest(size);
    let limits =
        HostLimits::try_new(32 * 1024 * 1024, 32 * 1024 * 1024, 4, 64 * 1024 * 1024).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let cfg = UploadConfig {
        display_name: "stream.bin".to_owned(),
        expected_sha256: digest_bytes,
        expected_size: u64::try_from(size).unwrap(),
        content_type: None,
    };
    let session = host.begin(&ctx_a(), &cfg).await.unwrap();
    let part_path = root.join(format!(
        "staging/{}/part/00001",
        session.session_id.as_str()
    ));
    std::fs::create_dir_all(part_path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(&part_path).unwrap();
    let chunk = [0xA5_u8; 8192];
    let mut left = size;
    while left > 0 {
        let n = 8192.min(left);
        std::io::Write::write_all(&mut file, &chunk[..n]).unwrap();
        left -= n;
    }
    drop(file);
    let published = host.complete(&ctx_a(), &session.session_id).await.unwrap();
    let hwm_before = proc_kib("VmHWM:");
    let rss_before = proc_kib("VmRSS:");
    let peak_before = proc_kib("VmPeak:");
    let threads_before = proc_kib("Threads:");
    let mut reader = host
        .open_download(
            &ctx_a(),
            &published.object_key,
            8192,
            Some(Instant::now() + Duration::from_secs(120)),
        )
        .await
        .unwrap();
    assert_eq!(reader.manifest(), &published);
    let mut hasher = Sha256::new();
    let mut pulled = 0u64;
    let mut chunk_max = 0usize;
    loop {
        assert_eq!(reader.manifest().object_key, published.object_key);
        assert_eq!(reader.manifest().object_digest, published.object_digest);
        assert_eq!(reader.manifest().size, published.size);
        assert_eq!(reader.manifest().lineage_digest, published.lineage_digest);
        match reader.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(!chunk.is_empty() && chunk.len() <= 8192);
                assert!(!reader.is_complete());
                chunk_max = chunk_max.max(chunk.len());
                hasher.update(&chunk);
                pulled += u64::try_from(chunk.len()).unwrap();
            }
            None => break,
        }
    }
    let hash = hasher.finalize();
    let mut download_sha = [0u8; 32];
    download_sha.copy_from_slice(&hash);
    let file_sha = hash_file(&root.join(published.object_key.as_str()), 8192);
    let hwm_after = proc_kib("VmHWM:");
    let rss_after = proc_kib("VmRSS:");
    let peak_after = proc_kib("VmPeak:");
    let threads_after = proc_kib("Threads:");
    let hwm_delta = hwm_after.saturating_sub(hwm_before) * 1024;
    let complete =
        u8::from(reader.is_complete() && download_sha == digest_bytes && file_sha == digest_bytes);
    let ok = complete == 1
        && pulled == size as u64
        && chunk_max <= 8192
        && hwm_delta < 4 * 1024 * 1024
        && hwm_delta.saturating_mul(2) < size as u64;
    let text = format!(
        "download_bytes={size}\nobject_key={}\ndownload_sha256={}\npattern_sha256={}\nfile_sha256={}\ndownload_hwm_before_kib={hwm_before}\ndownload_hwm_after_kib={hwm_after}\ndownload_hwm_delta_bytes={hwm_delta}\ndownload_rss_before_kib={rss_before}\ndownload_rss_after_kib={rss_after}\ndownload_vmpeak_before_kib={peak_before}\ndownload_vmpeak_after_kib={peak_after}\ndownload_threads_before={threads_before}\ndownload_threads_after={threads_after}\ndownload_chunk_max={chunk_max}\ndownload_pulled={pulled}\ndownload_complete={complete}\ncaller_retained_chunks=1\nfixture_object_vec=0\ndownload_ok={}\n",
        published.object_key.as_str(),
        hex(&download_sha),
        hex(&digest_bytes),
        hex(&file_sha),
        u8::from(ok)
    );
    std::fs::write(&metrics_path, &text).unwrap();
    if let Some(dir) = std::env::var_os("OO_FH_METRICS_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("download-memory-{size}.txt")), &text).unwrap();
    }
    assert!(ok, "{text}");
}

#[cfg(target_os = "linux")]
async fn memory_child() {
    let root = PathBuf::from(std::env::var("OO_FH_ROOT").unwrap());
    let metrics_path = PathBuf::from(std::env::var("OO_FH_METRICS").unwrap());
    let size: usize = std::env::var("OO_FH_SIZE").unwrap().parse().unwrap();
    assert!(size == 8 * 1024 * 1024 || size == 32 * 1024 * 1024);
    let chunk_len = 8192;
    let chunk = vec![0xA5_u8; chunk_len];
    let mut hasher = Sha256::new();
    let mut left = size;
    while left > 0 {
        let n = chunk_len.min(left);
        hasher.update(&chunk[..n]);
        left -= n;
    }
    let expected = hasher.finalize();
    let mut digest_bytes = [0u8; 32];
    digest_bytes.copy_from_slice(&expected);
    let limits =
        HostLimits::try_new(32 * 1024 * 1024, 32 * 1024 * 1024, 4, 64 * 1024 * 1024).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let cfg = UploadConfig {
        display_name: "stream.bin".to_owned(),
        expected_sha256: digest_bytes,
        expected_size: u64::try_from(size).unwrap(),
        content_type: None,
    };
    let session = host.begin(&ctx_a(), &cfg).await.unwrap();
    let part_path = root.join(format!(
        "staging/{}/part/00001",
        session.session_id.as_str()
    ));
    std::fs::create_dir_all(part_path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(&part_path).unwrap();
    let mut left = size;
    while left > 0 {
        let n = chunk_len.min(left);
        std::io::Write::write_all(&mut file, &chunk[..n]).unwrap();
        left -= n;
    }
    drop(file);
    let hwm_before = proc_kib("VmHWM:");
    let rss_before = proc_kib("VmRSS:");
    let peak_before = proc_kib("VmPeak:");
    let threads_before = proc_kib("Threads:");
    let manifest = host.complete(&ctx_a(), &session.session_id).await.unwrap();
    let hwm_after = proc_kib("VmHWM:");
    let rss_after = proc_kib("VmRSS:");
    let peak_after = proc_kib("VmPeak:");
    let threads_after = proc_kib("Threads:");
    let hwm_delta = hwm_after.saturating_sub(hwm_before) * 1024;
    let peak_delta = peak_after.saturating_sub(peak_before) * 1024;
    let object_path = root.join(manifest.object_key.as_str());
    let streamed = hash_file(&object_path, chunk_len);
    let part_gone = !part_path.exists();
    let ok = hwm_delta.saturating_mul(2) < size as u64
        && streamed == digest_bytes
        && manifest.size == size as u64
        && part_gone;
    let text = format!(
        "memory_bytes={size}\nmemory_hwm_before_kib={hwm_before}\nmemory_hwm_after_kib={hwm_after}\nmemory_hwm_delta_bytes={hwm_delta}\nmemory_rss_before_kib={rss_before}\nmemory_rss_after_kib={rss_after}\nmemory_vmpeak_before_kib={peak_before}\nmemory_vmpeak_after_kib={peak_after}\nmemory_vmpeak_delta_bytes={peak_delta}\nmemory_threads_before={threads_before}\nmemory_threads_after={threads_after}\nwrite_chunk={chunk_len}\npart_gone={part_gone}\nobject_sha_match={}\nmemory_ok={}\n",
        u8::from(streamed == digest_bytes),
        u8::from(ok)
    );
    std::fs::write(&metrics_path, &text).unwrap();
    if let Some(dir) = std::env::var_os("OO_FH_METRICS_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("memory-{size}.txt")), &text).unwrap();
    }
    assert!(ok, "{text}");
}

#[cfg(target_os = "linux")]
fn proc_kib(field: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            return rest.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("missing {field}");
}

#[cfg(target_os = "linux")]
fn hash_file(path: &Path, chunk_len: usize) -> [u8; 32] {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; chunk_len];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn metric_str(text: &str, key: &str) -> String {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            return rest.trim().to_owned();
        }
    }
    panic!("missing {key} in {text}");
}

fn metric_u64(text: &str, key: &str) -> u64 {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            return rest.trim().parse().unwrap();
        }
    }
    panic!("missing {key} in {text}");
}

fn write_begin_result(path: &Path, result: Result<oo_control::UploadSession, FileHostError>) {
    let line = match result {
        Ok(session) => format!("ok {}", session.session_id.as_str()),
        Err(error) => format!("err {error:?}"),
    };
    std::fs::write(path, line).unwrap();
}

fn spawn_child(test_name: &str, vars: &[(&str, &str)]) -> std::process::Child {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.arg(test_name)
        .arg("--exact")
        .arg("--test-threads=1")
        .env("OO_FH_CHILD", "1")
        .env("OO_FH_TEST", test_name)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in vars {
        cmd.env(key, value);
    }
    cmd.spawn().unwrap()
}

fn wait_child(child: std::process::Child) {
    let output = child.wait_with_output().unwrap();
    if !output.status.success() {
        panic!(
            "child exit {:?}\nstdout {}\nstderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

async fn barrier_arrive(dir: &Path, id: &str) {
    let dir = dir.to_path_buf();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        std::fs::write(dir.join(format!("ready-{id}")), b"ready").unwrap();
        let go = dir.join("go");
        let started = std::time::Instant::now();
        while !go.is_file() {
            if started.elapsed() > std::time::Duration::from_secs(60) {
                panic!("barrier timeout for {id}");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    })
    .await
    .unwrap();
}

fn release_when_ready(dir: &Path, ids: &[&str]) {
    let started = std::time::Instant::now();
    loop {
        if ids
            .iter()
            .all(|id| dir.join(format!("ready-{id}")).is_file())
        {
            std::fs::write(dir.join("go"), b"go").unwrap();
            return;
        }
        if started.elapsed() > std::time::Duration::from_secs(60) {
            panic!("ready timeout in {}", dir.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

async fn reserved_bytes(root: &Path, project: &str) -> u64 {
    let prefix = format!("meta/quota/{project}/");
    let mut total = 0u64;
    for key in object_keys(root).await {
        if !key.starts_with(&prefix) {
            continue;
        }
        let name = key.rsplit('/').next().unwrap_or("");
        if name.len() != 32
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            continue;
        }
        let body = fetch(root, &key).await.unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        total += text.parse::<u64>().unwrap();
    }
    total
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

async fn publish(
    host: &FileHost,
    ctx: &AdmissionContext,
    name: &str,
    bytes: &[u8],
) -> ObjectManifest {
    let session = host.begin(ctx, &upload(name, bytes, None)).await.unwrap();
    host.put_part(ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    host.complete(ctx, &session.session_id).await.unwrap()
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

fn encode_manifest_oracle(manifest: &ObjectManifest) -> Vec<u8> {
    let mut out = String::new();
    out.push('{');
    push_key(&mut out, "content_type", false);
    out.push(':');
    match manifest.content_type.as_deref() {
        Some(value) => push_json_string(&mut out, value),
        None => out.push_str("null"),
    }
    push_key(&mut out, "contract", true);
    out.push(':');
    push_json_string(&mut out, manifest.contract);
    push_key(&mut out, "display_name", true);
    out.push(':');
    push_json_string(&mut out, &manifest.display_name);
    push_key(&mut out, "grant_evidence", true);
    out.push(':');
    push_json_string(&mut out, manifest.grant_evidence.as_str());
    push_key(&mut out, "idempotency_key", true);
    out.push(':');
    push_json_string(&mut out, manifest.idempotency_key.as_str());
    push_key(&mut out, "lineage_digest", true);
    out.push(':');
    push_json_string(&mut out, &hex(&manifest.lineage_digest));
    push_key(&mut out, "object_digest", true);
    out.push(':');
    push_json_string(&mut out, &hex(&manifest.object_digest));
    push_key(&mut out, "object_key", true);
    out.push(':');
    push_json_string(&mut out, manifest.object_key.as_str());
    push_key(&mut out, "part_count", true);
    out.push(':');
    out.push_str(&manifest.part_count.to_string());
    push_key(&mut out, "principal_id", true);
    out.push(':');
    push_json_string(&mut out, manifest.principal_id.as_str());
    push_key(&mut out, "project_id", true);
    out.push(':');
    push_json_string(&mut out, manifest.project_id.as_str());
    push_key(&mut out, "session_id", true);
    out.push(':');
    push_json_string(&mut out, manifest.session_id.as_str());
    push_key(&mut out, "size", true);
    out.push(':');
    out.push_str(&manifest.size.to_string());
    push_key(&mut out, "version", true);
    out.push_str(":1}");
    out.into_bytes()
}

fn push_key(out: &mut String, key: &str, comma: bool) {
    if comma {
        out.push(',');
    }
    push_json_string(out, key);
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out.push('"');
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

struct HoldGate {
    hold_get: AtomicBool,
    hold_bytes: AtomicBool,
    release_get: AtomicBool,
    release_bytes: AtomicBool,
    entered_get: AtomicBool,
    entered_bytes: AtomicBool,
    done_get: AtomicBool,
    done_bytes: AtomicBool,
    dropped_get: AtomicBool,
    dropped_bytes: AtomicBool,
    waker_get: Mutex<Option<Waker>>,
    waker_bytes: Mutex<Option<Waker>>,
}

impl HoldGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            hold_get: AtomicBool::new(false),
            hold_bytes: AtomicBool::new(false),
            release_get: AtomicBool::new(false),
            release_bytes: AtomicBool::new(false),
            entered_get: AtomicBool::new(false),
            entered_bytes: AtomicBool::new(false),
            done_get: AtomicBool::new(false),
            done_bytes: AtomicBool::new(false),
            dropped_get: AtomicBool::new(false),
            dropped_bytes: AtomicBool::new(false),
            waker_get: Mutex::new(None),
            waker_bytes: Mutex::new(None),
        })
    }
}

#[derive(Clone, Copy)]
enum Hold {
    Get,
    Bytes,
}

struct Pause {
    hold: Hold,
    gate: Arc<HoldGate>,
    completed: bool,
    started: bool,
}

impl Pause {
    fn flag(
        gate: &HoldGate,
        hold: Hold,
    ) -> (
        &AtomicBool,
        &AtomicBool,
        &AtomicBool,
        &AtomicBool,
        &Mutex<Option<Waker>>,
    ) {
        match hold {
            Hold::Get => (
                &gate.release_get,
                &gate.entered_get,
                &gate.done_get,
                &gate.dropped_get,
                &gate.waker_get,
            ),
            Hold::Bytes => (
                &gate.release_bytes,
                &gate.entered_bytes,
                &gate.done_bytes,
                &gate.dropped_bytes,
                &gate.waker_bytes,
            ),
        }
    }
}

impl Future for Pause {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let (release, entered, done, _dropped, waker_slot) = Self::flag(&this.gate, this.hold);
        entered.store(true, Ordering::SeqCst);
        if release.load(Ordering::SeqCst) {
            this.completed = true;
            done.store(true, Ordering::SeqCst);
            return Poll::Ready(());
        }
        *waker_slot.lock().unwrap() = Some(cx.waker().clone());
        if !this.started {
            this.started = true;
            let gate = Arc::clone(&this.gate);
            let hold = this.hold;
            std::thread::Builder::new()
                .name("oo-a202-hold".to_owned())
                .spawn(move || {
                    let (release, _entered, done, _dropped, waker_slot) = Pause::flag(&gate, hold);
                    while !release.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    done.store(true, Ordering::SeqCst);
                    if let Some(waker) = waker_slot.lock().unwrap().take() {
                        waker.wake();
                    }
                })
                .unwrap();
        }
        Poll::Pending
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let (_release, _entered, _done, dropped, _waker) = Self::flag(&self.gate, self.hold);
        dropped.store(true, Ordering::SeqCst);
    }
}

struct CountingStore {
    inner: Arc<dyn ObjectStore>,
    gate: Arc<HoldGate>,
}

impl CountingStore {
    fn open(root: &Path) -> (Arc<Self>, Arc<HoldGate>) {
        let local = LocalFileSystem::new_with_prefix(root).unwrap();
        let gate = HoldGate::new();
        let store = Arc::new(Self {
            inner: Arc::new(local),
            gate: Arc::clone(&gate),
        });
        (store, gate)
    }
}

impl std::fmt::Debug for CountingStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CountingStore")
    }
}

impl std::fmt::Display for CountingStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("counting-store")
    }
}

fn pin_box<'a, T: Send + 'a>(
    fut: impl Future<Output = T> + Send + 'a,
) -> Pin<Box<dyn Future<Output = T> + Send + 'a>> {
    Box::pin(fut)
}

impl ObjectStore for CountingStore {
    fn put_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 StorePath,
        payload: PutPayload,
        opts: PutOptions,
    ) -> Pin<Box<dyn Future<Output = object_store::Result<PutResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let inner = Arc::clone(&self.inner);
        let location = location.clone();
        pin_box(async move { inner.put_opts(&location, payload, opts).await })
    }

    fn put_multipart_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 StorePath,
        opts: PutMultipartOptions,
    ) -> Pin<
        Box<
            dyn Future<Output = object_store::Result<Box<dyn MultipartUpload>>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let inner = Arc::clone(&self.inner);
        let location = location.clone();
        pin_box(async move { inner.put_multipart_opts(&location, opts).await })
    }

    fn get_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 StorePath,
        options: GetOptions,
    ) -> Pin<Box<dyn Future<Output = object_store::Result<GetResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let inner = Arc::clone(&self.inner);
        let gate = Arc::clone(&self.gate);
        let location = location.clone();
        pin_box(async move {
            let key = location.to_string();
            let body = !options.head && key.ends_with("original");
            if body {
                match &options.range {
                    Some(GetRange::Bounded(_)) => {}
                    Some(_) | None => {
                        return Err(object_store::Error::NotImplemented {
                            operation: "unbounded body get".to_owned(),
                            implementer: "CountingStore".to_owned(),
                        });
                    }
                }
                if gate.hold_get.load(Ordering::SeqCst) {
                    Pause {
                        hold: Hold::Get,
                        gate: Arc::clone(&gate),
                        completed: false,
                        started: false,
                    }
                    .await;
                }
            }
            if body && gate.hold_bytes.load(Ordering::SeqCst) {
                let got = inner.get_opts(&location, options).await?;
                let range = got.range.clone();
                let meta = got.meta.clone();
                let payload = got.bytes().await?;
                let gate_stream = Arc::clone(&gate);
                let stream = stream::once(async move {
                    Pause {
                        hold: Hold::Bytes,
                        gate: gate_stream,
                        completed: false,
                        started: false,
                    }
                    .await;
                    Ok::<_, object_store::Error>(payload)
                })
                .boxed();
                return Ok(GetResult {
                    payload: GetResultPayload::Stream(stream),
                    meta,
                    range,
                    attributes: Attributes::default(),
                    extensions: Default::default(),
                });
            }
            inner.get_opts(&location, options).await
        })
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<StorePath>>,
    ) -> BoxStream<'static, object_store::Result<StorePath>> {
        self.inner.delete_stream(locations)
    }

    fn list(
        &self,
        prefix: Option<&StorePath>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    fn list_with_delimiter<'life0, 'life1, 'async_trait>(
        &'life0 self,
        prefix: Option<&'life1 StorePath>,
    ) -> Pin<Box<dyn Future<Output = object_store::Result<ListResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let inner = Arc::clone(&self.inner);
        let prefix = prefix.cloned();
        pin_box(async move { inner.list_with_delimiter(prefix.as_ref()).await })
    }

    fn copy_opts<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        from: &'life1 StorePath,
        to: &'life2 StorePath,
        options: CopyOptions,
    ) -> Pin<Box<dyn Future<Output = object_store::Result<()>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        let inner = Arc::clone(&self.inner);
        let from = from.clone();
        let to = to.clone();
        pin_box(async move { inner.copy_opts(&from, &to, options).await })
    }
}
