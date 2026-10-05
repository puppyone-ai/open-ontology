//! Independent checks of the local upload primitive on a real LocalFileSystem.
//!
//! The assertions recompute sha256, the idempotency digest, and the lineage
//! digest in this file. They do not call the library's private helpers.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectKey, ObjectManifest,
    PrincipalId, ProjectId, SessionId, UploadConfig,
};
use sha2::{Digest, Sha256};

const MARKER: &str = "oo-a183-review";

#[tokio::test]
async fn resume_complete_and_lineage() {
    let scratch = Scratch::new("resume");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let part_a = "季度-α".as_bytes();
    let part_b = b"\x00\xff\xfe";
    let mut object = Vec::new();
    object.extend_from_slice(part_a);
    object.extend_from_slice(part_b);
    let object_sha = sha256(&object);
    let cfg = config("季度 \"a\".csv", &object_sha, object.len() as u64, Some("text/csv"));
    let first = admission("project_a", "user_a", "g \"α\" \\ z");
    let refreshed = admission("project_a", "user_a", "other-grant");

    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    assert!(!format!("{host:?}").contains(MARKER));
    let session = host.begin(&first, &cfg).await.unwrap();
    let sid = session.session_id.as_str().to_owned();
    let key_text = session.object_key.as_str().to_owned();
    assert_eq!(
        key_text,
        format!("uploads/{sid}/{}/original", hex(&object_sha))
    );
    assert!(!key_text.contains("季度"));
    assert!(!key_text.contains('"'));
    let expected_idem = idempotency_oracle(
        "project_a",
        "user_a",
        &object_sha,
        object.len() as u64,
        "季度 \"a\".csv",
        Some("text/csv"),
    );
    assert_eq!(hex(&expected_idem), session.idempotency_key.as_str());
    assert_eq!(
        oo_control::idempotency_key(&first, &cfg)
            .unwrap()
            .as_str(),
        session.idempotency_key.as_str()
    );
    assert_eq!(
        oo_control::idempotency_key(&refreshed, &cfg)
            .unwrap()
            .as_str(),
        session.idempotency_key.as_str()
    );
    let bare = config("季度 \"a\".csv", &object_sha, object.len() as u64, None);
    assert_ne!(
        oo_control::idempotency_key(&first, &bare)
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
    let again = host
        .put_part(&first, &session.session_id, 2, part_b)
        .await
        .unwrap();
    assert_eq!(again.part_sha256, sha256(part_b));
    assert_eq!(
        host.put_part(&first, &session.session_id, 2, b"nope")
            .await
            .unwrap_err(),
        FileHostError::Conflict
    );
    assert_eq!(
        fetch(&root, &format!("staging/{sid}/part/00002"))
            .await
            .as_deref(),
        Some(part_b.as_slice())
    );

    drop(host);
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let parsed = SessionId::parse(&sid).unwrap();
    let resumed = host.begin(&refreshed, &cfg).await.unwrap();
    assert_eq!(resumed.session_id.as_str(), sid);
    host.put_part(&refreshed, &parsed, 1, part_a).await.unwrap();
    let manifest = host.complete(&refreshed, &parsed).await.unwrap();
    assert_eq!(manifest.contract, "oo.control.local-file-hosting/v1");
    assert_eq!(manifest.grant_evidence.as_str(), "g \"α\" \\ z");
    assert_eq!(manifest.display_name, "季度 \"a\".csv");
    assert_eq!(manifest.content_type.as_deref(), Some("text/csv"));
    assert_eq!(manifest.object_digest, object_sha);
    assert_eq!(manifest.size, object.len() as u64);
    assert_eq!(manifest.part_count, 2);
    assert_eq!(manifest.lineage_digest, lineage_oracle(&manifest));
    assert_ne!(manifest.lineage_digest, manifest.object_digest);
    assert!(fetch(&root, &format!("staging/{sid}/part/00001")).await.is_none());
    assert!(fetch(&root, &format!("staging/{sid}/part/00002")).await.is_none());
    assert_eq!(fetch(&root, &key_text).await.as_deref(), Some(object.as_slice()));

    drop(host);
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let second = host.complete(&first, &parsed).await.unwrap();
    assert_eq!(second, manifest);
    let key = ObjectKey::parse(&key_text).unwrap();
    assert_eq!(host.read_manifest(&refreshed, &key).await.unwrap(), manifest);
    let stored = host.read_object(&first, &key).await.unwrap();
    assert_eq!(stored.bytes, object);
    assert_eq!(sha256(&stored.bytes), manifest.object_digest);
    assert_eq!(stored.manifest.lineage_digest, lineage_oracle(&manifest));
    write_store(&root, &key_text, b"mutated").await;
    assert_eq!(
        host.read_object(&first, &key).await.unwrap_err(),
        FileHostError::Corrupt
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");
    note(&format!(
        "resume_lineage_ok session={sid} object_sha256={}",
        hex(&object_sha)
    ));
}

#[tokio::test]
async fn two_contexts_grant_is_not_authority() {
    let scratch = Scratch::new("grant");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let bytes = b"same-bytes";
    let grant = "shared-grant-not-a-signature";
    let owner = admission("project_a", "user_a", grant);
    let other = admission("project_b", "user_b", grant);
    let other_principal = admission("project_a", "user_b", grant);
    let refreshed = admission("project_a", "user_a", "different-grant");
    let cfg = config("same.bin", &sha256(bytes), bytes.len() as u64, None);
    let owned = host.begin(&owner, &cfg).await.unwrap();
    host.put_part(&owner, &owned.session_id, 1, bytes)
        .await
        .unwrap();
    let manifest = host.complete(&owner, &owned.session_id).await.unwrap();
    assert_eq!(manifest.grant_evidence.as_str(), grant);
    assert_eq!(manifest.lineage_digest, lineage_oracle(&manifest));

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
    assert!(!denied.to_string().contains(MARKER));
    assert!(!denied.to_string().contains(owned.session_id.as_str()));
    for err in [
        host.read_object(&other, &owned.object_key)
            .await
            .unwrap_err(),
        host.put_part(&other, &owned.session_id, 1, bytes)
            .await
            .unwrap_err(),
        host.complete(&other, &owned.session_id)
            .await
            .unwrap_err(),
        host.abort(&other, &owned.session_id).await.unwrap_err(),
        host.read_manifest(&other_principal, &owned.object_key)
            .await
            .unwrap_err(),
    ] {
        assert_eq!(err, FileHostError::Denied);
        assert!(!err.to_string().contains(MARKER));
    }
    let still = host.read_object(&refreshed, &owned.object_key).await.unwrap();
    assert_eq!(still.bytes, bytes);
    assert_eq!(still.manifest.grant_evidence.as_str(), grant);
    let before = keys(&root).await;
    assert_eq!(host.abort(&other, &owned.session_id).await.unwrap_err(), FileHostError::Denied);
    assert_eq!(keys(&root).await, before);

    let second = host.begin(&other, &cfg).await.unwrap();
    assert_ne!(second.session_id.as_str(), owned.session_id.as_str());
    assert_ne!(second.object_key.as_str(), owned.object_key.as_str());
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
    note("grant_copied_to_foreign_context=Denied owner_grant_change_still_reads=1");
}

#[tokio::test]
async fn invalid_complete_hides_the_object() {
    let scratch = Scratch::new("invalid");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let limits = HostLimits::try_new(64, 64, 8, 128).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let ctx = admission("project_a", "user_a", "grant-a");
    let uploaded = b"\x00\xffnot-the-hash!";
    let expected_bytes = b"\x00\xffyes-the-hash!";
    assert_eq!(uploaded.len(), expected_bytes.len());
    let cfg = config(
        "bad.bin",
        &sha256(expected_bytes),
        uploaded.len() as u64,
        None,
    );
    let session = host.begin(&ctx, &cfg).await.unwrap();
    host.put_part(&ctx, &session.session_id, 1, uploaded)
        .await
        .unwrap();
    let notes = format!("staging/{}/part/notes", session.session_id.as_str());
    write_store(&root, &notes, b"foreign").await;
    let object_key = session.object_key.as_str().to_owned();
    assert_eq!(
        host.complete(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::Checksum
    );
    assert!(fetch(&root, &object_key).await.is_none());
    assert!(
        fetch(
            &root,
            &format!("staging/{}/part/00001", session.session_id.as_str())
        )
        .await
        .is_none()
    );
    assert!(
        fetch(
            &root,
            &format!("meta/session/{}", session.session_id.as_str())
        )
        .await
        .is_none()
    );
    assert_eq!(fetch(&root, &notes).await.as_deref(), Some(b"foreign".as_slice()));
    assert_eq!(
        host.read_manifest(&ctx, &session.object_key)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );

    let payload = b"abcd";
    let size_cfg = config("size.bin", &sha256(payload), payload.len() as u64, None);
    let size_session = host.begin(&ctx, &size_cfg).await.unwrap();
    host.put_part(&ctx, &size_session.session_id, 1, payload)
        .await
        .unwrap();
    let size_part = format!("staging/{}/part/00001", size_session.session_id.as_str());
    let size_notes = format!("staging/{}/part/notes", size_session.session_id.as_str());
    write_store(&root, &size_part, b"abcde").await;
    write_store(&root, &size_notes, b"foreign-size").await;
    assert_eq!(
        host.complete(&ctx, &size_session.session_id)
            .await
            .unwrap_err(),
        FileHostError::Size
    );
    assert!(fetch(&root, size_session.object_key.as_str()).await.is_none());
    assert!(fetch(&root, &size_part).await.is_none());
    assert_eq!(
        fetch(&root, &size_notes).await.as_deref(),
        Some(b"foreign-size".as_slice())
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");
    note("invalid_complete_object_visible=0 foreign_notes_kept=1");
}

#[tokio::test]
async fn abort_deletes_owned_keys_only() {
    let scratch = Scratch::new("abort");
    let outside = scratch.root.join("outside-keep.txt");
    std::fs::write(&outside, b"keep-me").unwrap();
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::try_new(32, 32, 4, 24).unwrap()).unwrap();
    let ctx = admission("project_q", "user_q", "grant-q");
    let other = admission("project_r", "user_r", "grant-q");
    let bytes = [1u8; 8];
    let cfg = config("a.bin", &sha256(&bytes), bytes.len() as u64, None);
    let session = host.begin(&ctx, &cfg).await.unwrap();
    host.put_part(&ctx, &session.session_id, 1, &bytes)
        .await
        .unwrap();
    let sid = session.session_id.as_str();
    let notes = format!("staging/{sid}/part/notes");
    let quota_notes = format!("meta/quota/{}/notes", ctx.project_id.as_str());
    write_store(&root, &notes, b"foreign").await;
    write_store(&root, &quota_notes, b"999999").await;
    let owned = [
        format!("staging/{sid}/part/00001"),
        format!("meta/session/{sid}"),
        format!("meta/quota/{}/{sid}", ctx.project_id.as_str()),
        format!(
            "meta/idempotency/{}/{}/{}",
            ctx.project_id.as_str(),
            ctx.principal_id.as_str(),
            session.idempotency_key.as_str()
        ),
    ];
    for key in &owned {
        assert!(fetch(&root, key).await.is_some(), "missing {key}");
    }
    let before = keys(&root).await;
    assert_eq!(
        host.abort(&other, &session.session_id).await.unwrap_err(),
        FileHostError::Denied
    );
    assert_eq!(keys(&root).await, before);
    host.abort(&ctx, &session.session_id).await.unwrap();
    let after = keys(&root).await;
    for key in &owned {
        assert!(!after.iter().any(|item| item == key), "still present {key}");
    }
    assert_eq!(fetch(&root, &notes).await.as_deref(), Some(b"foreign".as_slice()));
    assert_eq!(
        fetch(&root, &quota_notes).await.as_deref(),
        Some(b"999999".as_slice())
    );
    assert!(fetch(&root, session.object_key.as_str()).await.is_none());
    assert_eq!(
        host.abort(&ctx, &session.session_id).await.unwrap_err(),
        FileHostError::NotFound
    );
    let removed: Vec<_> = before
        .iter()
        .filter(|key| !after.iter().any(|item| item == *key))
        .cloned()
        .collect();
    removed.iter().for_each(|key| assert!(owned.iter().any(|item| item == key), "unexpected delete {key}"));
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep-me");

    let finished_bytes = [2u8; 8];
    let finished_cfg = config(
        "b.bin",
        &sha256(&finished_bytes),
        finished_bytes.len() as u64,
        None,
    );
    let finished = host.begin(&ctx, &finished_cfg).await.unwrap();
    host.put_part(&ctx, &finished.session_id, 1, &finished_bytes)
        .await
        .unwrap();
    let manifest = host.complete(&ctx, &finished.session_id).await.unwrap();
    assert_eq!(
        host.abort(&ctx, &finished.session_id).await.unwrap_err(),
        FileHostError::Closed
    );
    assert_eq!(
        fetch(&root, manifest.object_key.as_str()).await.as_deref(),
        Some(finished_bytes.as_slice())
    );
    note(&format!(
        "abort_removed={}",
        removed.join(",")
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_complete_returns_one_manifest() {
    let scratch = Scratch::new("concurrent");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_c", "user_c", "grant-c");
    let part_a = b"left-";
    let part_b = b"right";
    let mut object = Vec::new();
    object.extend_from_slice(part_a);
    object.extend_from_slice(part_b);
    let cfg = config("both.bin", &sha256(&object), object.len() as u64, Some("text/plain"));
    let session = host.begin(&ctx, &cfg).await.unwrap();
    host.put_part(&ctx, &session.session_id, 1, part_a)
        .await
        .unwrap();
    host.put_part(&ctx, &session.session_id, 2, part_b)
        .await
        .unwrap();
    let host = Arc::new(host);
    let left_host = Arc::clone(&host);
    let right_host = Arc::clone(&host);
    let left_ctx = ctx.clone();
    let right_ctx = ctx.clone();
    let left_id = session.session_id.clone();
    let right_id = session.session_id.clone();
    let (left, right) = tokio::join!(
        left_host.complete(&left_ctx, &left_id),
        right_host.complete(&right_ctx, &right_id),
    );
    let mut oks = Vec::new();
    let mut errs = Vec::new();
    for result in [left, right] {
        match result {
            Ok(manifest) => oks.push(manifest),
            Err(err) => errs.push(err),
        }
    }
    note(&format!(
        "concurrent_complete oks={} errs={}",
        oks.len(),
        errs.iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    assert!(!oks.is_empty(), "neither complete returned a manifest: {errs:?}");
    let manifest = host.complete(&ctx, &session.session_id).await.unwrap();
    let stored = host.read_object(&ctx, &manifest.object_key).await.unwrap();
    assert_eq!(stored.bytes, object);
    assert_eq!(sha256(&stored.bytes), manifest.object_digest);
    assert_eq!(manifest.lineage_digest, lineage_oracle(&manifest));
    for item in &oks {
        assert_eq!(item, &manifest);
    }
    assert_eq!(
        fetch(&root, manifest.object_key.as_str()).await.as_deref(),
        Some(object.as_slice())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quota_window_one_attempt() {
    let scratch = Scratch::new("quota-seq");
    let root = scratch.store();
    let limits = HostLimits::try_new(16, 16, 4, 10).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let ctx = admission("project_q", "user_q", "grant-q");
    let first = [1u8; 8];
    let second = [2u8; 8];
    let first_cfg = config("one.bin", &sha256(&first), 8, None);
    let second_cfg = config("two.bin", &sha256(&second), 8, None);
    host.begin(&ctx, &first_cfg).await.unwrap();
    assert_eq!(
        host.begin(&ctx, &second_cfg).await.unwrap_err(),
        FileHostError::Quota
    );

    let scratch = Scratch::new("quota-race");
    let root = scratch.store();
    let host = Arc::new(FileHost::open_local(&root, limits).unwrap());
    let left_host = Arc::clone(&host);
    let right_host = Arc::clone(&host);
    let left_ctx = ctx.clone();
    let right_ctx = ctx.clone();
    let left_cfg = first_cfg.clone();
    let right_cfg = second_cfg.clone();
    let (left, right) = tokio::join!(
        left_host.begin(&left_ctx, &left_cfg),
        right_host.begin(&right_ctx, &right_cfg),
    );
    let mut oks = 0u32;
    let mut quota = 0u32;
    let mut other = Vec::new();
    for result in [left, right] {
        match result {
            Ok(_) => oks += 1,
            Err(FileHostError::Quota) => quota += 1,
            Err(err) => other.push(format!("{err:?}")),
        }
    }
    assert!(other.is_empty(), "unexpected begin errors: {other:?}");
    assert!(oks >= 1, "both begins failed");
    let mut reserved = 0u64;
    for key in keys(&root).await {
        if key.starts_with("meta/quota/project_q/") && key.rsplit('/').next().is_some_and(|name| name.len() == 32) {
            let body = fetch(&root, &key).await.unwrap();
            let text = std::str::from_utf8(&body).unwrap();
            reserved += text.parse::<u64>().unwrap();
        }
    }
    note(&format!(
        "quota_one_attempt oks={oks} quota_errors={quota} reserved={reserved} limit=10"
    ));
    if oks == 2 {
        assert!(reserved > 10, "both begins fit inside the quota");
    }
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("{MARKER}-{name}-{}", std::process::id()));
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

fn note(line: &str) {
    println!("oracle_note {line}");
    if let Ok(path) = std::env::var("OO_REVIEW_OUT") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{line}").unwrap();
    }
}

fn admission(project: &str, principal: &str, grant: &str) -> AdmissionContext {
    AdmissionContext {
        project_id: ProjectId::new(project).unwrap(),
        principal_id: PrincipalId::new(principal).unwrap(),
        grant_evidence: GrantEvidence::new(grant).unwrap(),
    }
}

fn config(name: &str, sha: &[u8; 32], size: u64, content_type: Option<&str>) -> UploadConfig {
    UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: *sha,
        expected_size: size,
        content_type: content_type.map(str::to_owned),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
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
    store.put(&path, bytes.to_vec().into()).await.expect("store put");
}

async fn keys(root: &Path) -> Vec<String> {
    let store = open_raw(root);
    let mut stream = store.list(None);
    let mut found = Vec::new();
    while let Some(item) = stream.next().await {
        found.push(item.expect("list").location.to_string());
    }
    found.sort();
    found
}
