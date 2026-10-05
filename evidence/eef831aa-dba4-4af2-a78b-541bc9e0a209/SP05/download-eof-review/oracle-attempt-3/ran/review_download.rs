//! Independent review of the local download at the fixed archive.
//!
//! This file is copied onto that archive for the review run. It is not part of
//! the product commit. The oracle is `sha2` in this process. No Python hasher.

#![forbid(unsafe_code)]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};
use object_store::local::LocalFileSystem;
use object_store::path::Path as StorePath;
use object_store::{
    Attributes, CopyOptions, GetOptions, GetRange, GetResult, GetResultPayload, ListResult,
    MultipartUpload, ObjectMeta, ObjectStore, PutMultipartOptions, PutOptions, PutPayload,
    PutResult,
};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectManifest,
    PrincipalId, ProjectId, UploadConfig,
};
use sha2::{Digest, Sha256};

const KNOWN: &[u8] = b"OOA209-known-payload-v1";
const PATTERN: u8 = 0x5A;
const BLOCK_MS: u64 = 1500;

/// `ObjectDownload` is not re-exported, so the rest of a pull cannot name it.
macro_rules! pull_rest {
    ($reader:ident) => {{
        loop {
            match $reader.next_chunk().await {
                Ok(Some(_)) => assert!(!$reader.is_complete()),
                Ok(None) => panic!("download completed"),
                Err(error) => break error,
            }
        }
    }};
}

#[tokio::test]
async fn review_local_clean_eof_prefix_flip_and_quota() {
    let scratch = Scratch::new("local");
    let root = scratch.store();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission();
    let source_hash = digest(KNOWN);
    let published = publish(&host, &ctx, "known.bin", KNOWN).await;
    assert_eq!(published.object_digest, source_hash);
    assert_eq!(published.size, KNOWN.len() as u64);
    let object_path = root.join(published.object_key.as_str());
    assert_eq!(hash_file(&object_path), source_hash);

    let quota_path = quota_file(
        &root,
        ctx.project_id.as_str(),
        published.session_id.as_str(),
    );
    let quota_before = std::fs::read(&quota_path).unwrap();
    let names_before = quota_names(&root, ctx.project_id.as_str());

    let generic_store = Arc::new(LocalFileSystem::new_with_prefix(&root).unwrap());
    let generic = FileHost::new(generic_store, HostLimits::standard());
    let fresh = upload("other.bin", b"zzzzzzzz");
    assert_eq!(
        generic.begin(&ctx, &fresh).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        generic
            .put_part(&ctx, &published.session_id, 1, b"zzzzzzzz")
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        generic
            .complete(&ctx, &published.session_id)
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        generic
            .abort(&ctx, &published.session_id)
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );

    let generic_pull = pull_all(&generic, &ctx, &published, 5).await;
    assert!(generic_pull.complete);
    assert_eq!(generic_pull.returned, source_hash);
    assert_eq!(generic_pull.bytes, KNOWN);
    assert_eq!(hash_file(&object_path), source_hash);
    assert_eq!(std::fs::read(&quota_path).unwrap(), quota_before);
    assert_eq!(quota_names(&root, ctx.project_id.as_str()), names_before);

    let flipped = publish(&host, &ctx, "generic-flip.bin", KNOWN).await;
    let flip_path = root.join(flipped.object_key.as_str());
    let mut reader = generic
        .open_download(&ctx, &flipped.object_key, 5, None)
        .await
        .unwrap();
    let first = reader.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.as_slice(), &KNOWN[..first.len()]);
    assert!(!reader.is_complete());
    flip_byte(&flip_path, 0);
    let generic_err = pull_rest!(reader);
    assert_eq!(generic_err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &flipped);
    assert_ne!(hash_file(&flip_path), source_hash);
    assert_eq!(std::fs::read(&quota_path).unwrap(), quota_before);

    let mut clean = host
        .open_download(&ctx, &published.object_key, 5, None)
        .await
        .unwrap();
    let mut returned = Sha256::new();
    let mut got = Vec::new();
    loop {
        assert_eq!(clean.manifest(), &published);
        match clean.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(!chunk.is_empty() && chunk.len() <= 5);
                assert!(!clean.is_complete());
                assert_eq!(&KNOWN[got.len()..got.len() + chunk.len()], chunk.as_slice());
                returned.update(&chunk);
                got.extend_from_slice(&chunk);
            }
            None => break,
        }
    }
    assert_eq!(got, KNOWN);
    assert_eq!(finish(returned), source_hash);
    assert_eq!(hash_file(&object_path), source_hash);
    assert_eq!(published.object_digest, source_hash);
    assert!(clean.is_complete());

    flip_byte(&object_path, 0);
    assert!(clean.is_complete());
    assert!(clean.next_chunk().await.unwrap().is_none());
    assert!(clean.is_complete());
    assert_eq!(clean.manifest(), &published);
    assert_ne!(hash_file(&object_path), source_hash);
    let mut later = host
        .open_download(&ctx, &published.object_key, 5, None)
        .await
        .unwrap();
    let later_err = pull_rest!(later);
    assert_eq!(later_err, FileHostError::Corrupt);
    assert!(!later.is_complete());
    assert_eq!(later.manifest().object_digest, source_hash);

    let consumed = publish(&host, &ctx, "consumed.bin", KNOWN).await;
    let consumed_path = root.join(consumed.object_key.as_str());
    let mut reader = host
        .open_download(&ctx, &consumed.object_key, 5, None)
        .await
        .unwrap();
    let first = reader.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.as_slice(), &KNOWN[..first.len()]);
    let mut running = Sha256::new();
    running.update(&first);
    assert!(!reader.is_complete());
    flip_byte(&consumed_path, 0);
    let mut offset = first.len();
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(chunk)) => {
                assert!(!reader.is_complete());
                running.update(&chunk);
                offset += chunk.len();
            }
            Ok(None) => panic!("consumed prefix flip completed"),
            Err(error) => break error,
        }
    };
    assert_eq!(offset, KNOWN.len());
    assert_eq!(finish(running), source_hash);
    assert_ne!(hash_file(&consumed_path), source_hash);
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(reader.manifest(), &consumed);

    let unread = publish(&host, &ctx, "unread.bin", KNOWN).await;
    let unread_path = root.join(unread.object_key.as_str());
    let mut reader = host
        .open_download(&ctx, &unread.object_key, 5, None)
        .await
        .unwrap();
    assert!(reader.next_chunk().await.unwrap().is_some());
    let last = KNOWN.len() - 1;
    flip_byte(&unread_path, last);
    let unread_err = pull_rest!(reader);
    assert_eq!(unread_err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_ne!(hash_file(&unread_path), source_hash);
    assert_eq!(reader.manifest(), &unread);

    let cut = publish(&host, &ctx, "cut.bin", KNOWN).await;
    let mut reader = host
        .open_download(&ctx, &cut.object_key, 5, None)
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

    let mut note = String::new();
    note.push_str(&format!("source_len={}\n", KNOWN.len()));
    note.push_str(&format!("source_sha256={}\n", hex(&source_hash)));
    note.push_str("clean_complete=1\n");
    note.push_str("post_complete_flip_still_complete=1\n");
    note.push_str("later_reader_after_flip=Corrupt\n");
    note.push_str("consumed_prefix_flip=Corrupt\n");
    note.push_str("consumed_prefix_running_matches_source=1\n");
    note.push_str("consumed_prefix_file_matches_source=0\n");
    note.push_str("unread_tail_flip=Corrupt\n");
    note.push_str(&format!("truncate_after_first_chunk={cut_err:?}\n"));
    note.push_str("generic_mutations=Unsupported\n");
    note.push_str("generic_clean_complete=1\n");
    note.push_str("generic_prefix_flip=Corrupt\n");
    note.push_str("quota_bytes_unchanged=1\n");
    note.push_str("quota_names_unchanged=1\n");
    write_note("local.txt", &note);
}

#[tokio::test]
async fn review_ranges_etag_and_deadline() {
    let scratch = Scratch::new("probe");
    let root = scratch.store();
    let publisher = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission();
    let mut source = vec![0x11_u8; 20_000];
    source[0] = 0x21;
    source[19_999] = 0x22;
    let source_hash = digest(&source);
    let manifest = publish(&publisher, &ctx, "ranges.bin", &source).await;
    assert_eq!(manifest.object_digest, source_hash);

    let (probe, log) = Probe::open(&root);
    let host = FileHost::new(probe, HostLimits::standard());
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 1000, None)
        .await
        .unwrap();
    let mut returned = Sha256::new();
    let mut pulled = 0usize;
    let mut data_max = 0usize;
    loop {
        assert_eq!(reader.manifest().object_digest, source_hash);
        match reader.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(chunk.len() <= 1000);
                assert!(!reader.is_complete());
                data_max = data_max.max(chunk.len());
                returned.update(&chunk);
                pulled += chunk.len();
            }
            None => break,
        }
    }
    assert_eq!(pulled, source.len());
    assert_eq!(finish(returned), source_hash);
    assert_eq!(
        hash_file(&root.join(manifest.object_key.as_str())),
        source_hash
    );
    assert!(reader.is_complete());

    let body = log.body_ranges();
    assert_eq!(body.len(), 23, "{body:?}");
    for seen in body.iter().take(20) {
        assert_eq!(seen.len, 1000, "{seen:?}");
    }
    assert_eq!(body[20].len, 8192);
    assert_eq!(body[21].len, 8192);
    assert_eq!(body[22].len, 3616);
    assert!(body.iter().all(|seen| seen.len > 0 && seen.len <= 8192));
    assert_eq!(log.body_unbounded.load(Ordering::SeqCst), 0);
    assert_eq!(log.body_over_8192.load(Ordering::SeqCst), 0);
    for seen in log.manifest_ranges() {
        assert!(seen.len > 0 && seen.len <= 4096, "{seen:?}");
    }
    let before_extra = log.body_gets.load(Ordering::SeqCst);
    flip_byte(&root.join(manifest.object_key.as_str()), 3);
    assert!(reader.is_complete());
    assert!(reader.next_chunk().await.unwrap().is_none());
    assert_eq!(log.body_gets.load(Ordering::SeqCst), before_extra);
    assert!(reader.is_complete());

    let stable = publish(&publisher, &ctx, "etag-stable.bin", KNOWN).await;
    let (probe, log) = Probe::open(&root);
    log.flip_etag_at.store(0, Ordering::SeqCst);
    let host = FileHost::new(probe, HostLimits::standard());
    let stable_pull = pull_all(&host, &ctx, &stable, KNOWN.len()).await;
    assert!(stable_pull.complete);
    assert_eq!(stable_pull.returned, digest(KNOWN));
    let stable_heads = log.head_tags();
    assert!(stable_heads.len() >= 3, "{stable_heads:?}");
    assert!(stable_heads.iter().all(|tag| tag == "a209-etag-a"));

    let changed = publish(&publisher, &ctx, "etag-change.bin", KNOWN).await;
    let (probe, log) = Probe::open(&root);
    log.flip_etag_at.store(2, Ordering::SeqCst);
    let host = FileHost::new(probe, HostLimits::standard());
    let mut reader = host
        .open_download(&ctx, &changed.object_key, KNOWN.len(), None)
        .await
        .unwrap();
    let mut returned = Sha256::new();
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(chunk)) => {
                assert!(!reader.is_complete());
                returned.update(&chunk);
            }
            Ok(None) => panic!("etag change across rehash completed"),
            Err(error) => break error,
        }
    };
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_eq!(finish(returned), digest(KNOWN));
    assert_eq!(
        hash_file(&root.join(changed.object_key.as_str())),
        digest(KNOWN)
    );
    let tags = log.head_tags();
    assert!(tags.contains(&"a209-etag-a".to_owned()), "{tags:?}");
    assert!(tags.contains(&"a209-etag-b".to_owned()), "{tags:?}");

    let held = publish(&publisher, &ctx, "held.bin", KNOWN).await;
    let (probe, log) = Probe::open(&root);
    let host = FileHost::new(probe, HostLimits::standard());
    log.hold_get.store(true, Ordering::SeqCst);
    log.hold_bytes.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut reader = host
        .open_download(&ctx, &held.object_key, 5, Some(deadline))
        .await
        .unwrap();
    let stopped = {
        let mut fut = std::pin::pin!(reader.next_chunk());
        poll_until(&mut fut, || log.entered_get.load(Ordering::SeqCst));
        assert!(!log.entered_bytes.load(Ordering::SeqCst));
        while Instant::now() < deadline + Duration::from_millis(80) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!log.dropped_get.load(Ordering::SeqCst));
        assert!(!log.entered_bytes.load(Ordering::SeqCst));
        log.release_get.store(true, Ordering::SeqCst);
        poll_ready(&mut fut).unwrap_err()
    };
    assert_eq!(stopped, FileHostError::Stopped);
    assert_eq!(
        stopped.to_string(),
        "download stopped before the object was verified"
    );
    assert!(!log.entered_bytes.load(Ordering::SeqCst));
    assert!(!reader.is_complete());
    log.hold_get.store(false, Ordering::SeqCst);
    log.hold_bytes.store(false, Ordering::SeqCst);
    assert_eq!(
        reader.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );

    let (probe, log) = Probe::open(&root);
    let host = FileHost::new(probe, HostLimits::standard());
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut reader = host
        .open_download(&ctx, &held.object_key, 5, Some(deadline))
        .await
        .unwrap();
    log.hold_bytes.store(true, Ordering::SeqCst);
    let stopped = {
        let mut fut = std::pin::pin!(reader.next_chunk());
        poll_until(&mut fut, || log.entered_bytes.load(Ordering::SeqCst));
        assert!(!log.dropped_bytes.load(Ordering::SeqCst));
        while Instant::now() < deadline + Duration::from_millis(80) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!log.dropped_bytes.load(Ordering::SeqCst));
        log.release_bytes.store(true, Ordering::SeqCst);
        poll_ready(&mut fut).unwrap_err()
    };
    assert_eq!(stopped, FileHostError::Stopped);
    assert!(!reader.is_complete());

    let (probe, log) = Probe::open(&root);
    let host = FileHost::new(probe, HostLimits::standard());
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut reader = host
        .open_download(&ctx, &held.object_key, KNOWN.len(), Some(deadline))
        .await
        .unwrap();
    log.block.store(true, Ordering::SeqCst);
    let block_err = {
        let mut fut = std::pin::pin!(reader.next_chunk());
        drive_block(
            &mut fut,
            deadline + Duration::from_millis(80),
            &log.block_started,
            &log.block_finished,
            &log.block_alive,
        )
    };
    assert_eq!(block_err, FileHostError::Stopped);
    assert!(!log.entered_bytes.load(Ordering::SeqCst));
    assert!(!log.dropped_block.load(Ordering::SeqCst));
    assert!(!reader.is_complete());

    let (probe, log) = Probe::open(&root);
    let host = FileHost::new(probe, HostLimits::standard());
    let mut expired = host
        .open_download(
            &ctx,
            &held.object_key,
            5,
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .await
        .unwrap();
    let expired_err = expired.next_chunk().await.unwrap_err();
    assert_eq!(expired_err, FileHostError::Stopped);
    assert_eq!(log.body_gets.load(Ordering::SeqCst), 0);
    assert!(!expired.is_complete());

    let mut cancelling = host
        .open_download(&ctx, &held.object_key, 5, None)
        .await
        .unwrap();
    assert!(cancelling.next_chunk().await.unwrap().is_some());
    cancelling.cancel();
    assert_eq!(
        cancelling.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!cancelling.is_complete());

    let mut note = String::new();
    note.push_str("body_ranges=23\n");
    note.push_str("data_budget=1000\n");
    note.push_str("data_range_len=1000\n");
    note.push_str("rehash_lens=8192,8192,3616\n");
    note.push_str("body_unbounded=0\n");
    note.push_str("body_over_8192=0\n");
    note.push_str("manifest_ranges_at_most_4096=1\n");
    note.push_str(&format!("source_sha256={}\n", hex(&source_hash)));
    note.push_str("running_sha_matches_source=1\n");
    note.push_str("file_sha_matches_source=1\n");
    note.push_str("manifest_sha_matches_source=1\n");
    note.push_str("post_complete_extra_body_gets=0\n");
    note.push_str("stable_etag_complete=1\n");
    note.push_str("etag_change_across_rehash=Corrupt\n");
    note.push_str("etag_change_bytes_still_match_source=1\n");
    note.push_str("held_get_dropped_by_deadline=0\n");
    note.push_str("bytes_started_after_held_get=0\n");
    note.push_str("held_bytes_dropped_by_deadline=0\n");
    note.push_str("held_bytes_result=Stopped\n");
    note.push_str(&format!("blocking_sleep_ms={BLOCK_MS}\n"));
    note.push_str("blocking_get_preempted=0\n");
    note.push_str("blocking_get_bytes_started=0\n");
    note.push_str("blocking_get_result=Stopped\n");
    note.push_str("expired_deadline_body_gets=0\n");
    note.push_str(&format!("data_chunk_max={data_max}\n"));
    write_note("probe.txt", &note);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn review_memory_is_incremental() {
    if std::env::var_os("OO_A209_CHILD").is_some()
        && std::env::var("OO_A209_TEST").as_deref() == Ok("review_memory_is_incremental")
    {
        memory_child().await;
        return;
    }
    let scratch = Scratch::new("memory");
    let root = scratch.store();
    std::fs::create_dir_all(&root).unwrap();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission();
    let mut lines = String::new();
    for size in [8 * 1024 * 1024_usize, 32 * 1024 * 1024] {
        let expected = pattern_digest(size, PATTERN);
        let manifest = publish_pattern(&host, &ctx, &root, size, PATTERN, expected).await;
        let metrics = scratch.root.join(format!("memory-{size}.txt"));
        let child = spawn_child(
            "review_memory_is_incremental",
            &[
                ("OO_A209_ROOT", root.to_str().unwrap()),
                ("OO_A209_KEY", manifest.object_key.as_str()),
                ("OO_A209_SHA", &hex(&expected)),
                ("OO_A209_SIZE", &size.to_string()),
                ("OO_A209_METRICS", metrics.to_str().unwrap()),
            ],
        );
        wait_child(child);
        let text = std::fs::read_to_string(&metrics).unwrap();
        let file_hash = hash_file(&root.join(manifest.object_key.as_str()));
        assert_eq!(hex(&file_hash), hex(&expected));
        assert_eq!(hex(&file_hash), metric_str(&text, "download_sha256"));
        assert_eq!(metric_str(&text, "pattern_sha256"), hex(&expected));
        assert_eq!(metric_u64(&text, "download_chunk_max"), 8192);
        assert_eq!(metric_u64(&text, "download_pulled"), size as u64);
        assert_eq!(metric_u64(&text, "download_complete"), 1);
        let delta = metric_u64(&text, "download_hwm_delta_bytes");
        assert!(delta < 4 * 1024 * 1024, "{text}");
        assert!(delta.saturating_mul(2) < size as u64, "{text}");
        lines.push_str(&format!("--- {size} ---\n{text}\n"));
        write_note(&format!("memory-{size}.txt"), &text);
    }
    write_note("memory.txt", &lines);
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn review_memory_is_incremental() {
    panic!("resident measurement runs on the Linux builder");
}

struct Pull {
    bytes: Vec<u8>,
    returned: [u8; 32],
    complete: bool,
}

async fn pull_all(
    host: &FileHost,
    ctx: &AdmissionContext,
    manifest: &ObjectManifest,
    budget: usize,
) -> Pull {
    let mut reader = host
        .open_download(ctx, &manifest.object_key, budget, None)
        .await
        .unwrap();
    let mut hasher = Sha256::new();
    let mut bytes = Vec::new();
    loop {
        match reader.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(!reader.is_complete());
                assert!(chunk.len() <= budget);
                hasher.update(&chunk);
                bytes.extend_from_slice(&chunk);
            }
            None => break,
        }
    }
    Pull {
        bytes,
        returned: finish(hasher),
        complete: reader.is_complete(),
    }
}

fn admission() -> AdmissionContext {
    AdmissionContext {
        project_id: ProjectId::new("project_a209").unwrap(),
        principal_id: PrincipalId::new("user_a209").unwrap(),
        grant_evidence: GrantEvidence::new("grant-a209-review").unwrap(),
    }
}

fn upload(name: &str, bytes: &[u8]) -> UploadConfig {
    UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: digest(bytes),
        expected_size: u64::try_from(bytes.len()).unwrap(),
        content_type: None,
    }
}

async fn publish(
    host: &FileHost,
    ctx: &AdmissionContext,
    name: &str,
    bytes: &[u8],
) -> ObjectManifest {
    let session = host.begin(ctx, &upload(name, bytes)).await.unwrap();
    host.put_part(ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    host.complete(ctx, &session.session_id).await.unwrap()
}

async fn publish_pattern(
    host: &FileHost,
    ctx: &AdmissionContext,
    root: &Path,
    size: usize,
    byte: u8,
    expected: [u8; 32],
) -> ObjectManifest {
    let cfg = UploadConfig {
        display_name: "stream.bin".to_owned(),
        expected_sha256: expected,
        expected_size: u64::try_from(size).unwrap(),
        content_type: None,
    };
    let session = host.begin(ctx, &cfg).await.unwrap();
    let part_path = root.join(format!(
        "staging/{}/part/00001",
        session.session_id.as_str()
    ));
    std::fs::create_dir_all(part_path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(&part_path).unwrap();
    let chunk = [byte; 8192];
    let mut left = size;
    while left > 0 {
        let n = left.min(8192);
        std::io::Write::write_all(&mut file, &chunk[..n]).unwrap();
        left -= n;
    }
    drop(file);
    host.complete(ctx, &session.session_id).await.unwrap()
}

fn pattern_digest(size: usize, byte: u8) -> [u8; 32] {
    let chunk = [byte; 8192];
    let mut hasher = Sha256::new();
    let mut left = size;
    while left > 0 {
        let n = left.min(8192);
        hasher.update(&chunk[..n]);
        left -= n;
    }
    finish(hasher)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    finish(Sha256::new().chain_update(bytes))
}

fn finish(hasher: Sha256) -> [u8; 32] {
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn hash_file(path: &Path) -> [u8; 32] {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    finish(hasher)
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

fn flip_byte(path: &Path, index: usize) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[index] ^= 0xa9;
    std::fs::write(path, bytes).unwrap();
}

fn quota_file(root: &Path, project: &str, session: &str) -> PathBuf {
    root.join(format!("meta/quota/{project}/{session}"))
}

fn quota_names(root: &Path, project: &str) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(root.join(format!("meta/quota/{project}"))).unwrap() {
        names.push(entry.unwrap().file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

fn write_note(name: &str, text: &str) {
    println!("{text}");
    let Some(dir) = std::env::var_os("OO_A209_NOTES") else {
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
}

fn poll_until<F: Future>(fut: &mut Pin<&mut F>, pred: impl Fn() -> bool) {
    let waker = Waker::noop();
    let mut context = Context::from_waker(&waker);
    let start = Instant::now();
    loop {
        if pred() {
            return;
        }
        match fut.as_mut().poll(&mut context) {
            Poll::Pending => {}
            Poll::Ready(_) => panic!("download finished before the hold was entered"),
        }
        if start.elapsed() > Duration::from_secs(20) {
            panic!("timed out waiting for the held operation");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn poll_ready<F: Future>(fut: &mut Pin<&mut F>) -> F::Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(&waker);
    let start = Instant::now();
    loop {
        match fut.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => {
                if start.elapsed() > Duration::from_secs(20) {
                    panic!("timed out waiting for the held download");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn drive_block<F: Future<Output = Result<Option<Vec<u8>>, FileHostError>>>(
    fut: &mut Pin<&mut F>,
    mark: Instant,
    started: &AtomicBool,
    finished: &AtomicBool,
    alive: &AtomicBool,
) -> FileHostError {
    let waker = Waker::noop();
    let mut context = Context::from_waker(&waker);
    let mut saw_pending = false;
    let start = Instant::now();
    loop {
        match fut.as_mut().poll(&mut context) {
            Poll::Pending => {
                if started.load(Ordering::SeqCst)
                    && Instant::now() >= mark
                    && !finished.load(Ordering::SeqCst)
                    && alive.load(Ordering::SeqCst)
                {
                    saw_pending = true;
                }
                if start.elapsed() > Duration::from_secs(15) {
                    panic!("blocking get timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Poll::Ready(value) => {
                assert!(
                    saw_pending,
                    "get_opts was not still pending past the deadline"
                );
                assert!(finished.load(Ordering::SeqCst));
                return value.unwrap_err();
            }
        }
    }
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oo-a209-{name}-{}", std::process::id()));
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

#[derive(Clone, Debug)]
struct SeenRange {
    start: u64,
    end: u64,
    len: u64,
}

struct ProbeLog {
    body: Mutex<Vec<SeenRange>>,
    manifest: Mutex<Vec<SeenRange>>,
    heads: Mutex<Vec<String>>,
    body_gets: AtomicU64,
    body_unbounded: AtomicU64,
    body_over_8192: AtomicU64,
    flip_etag_at: AtomicU64,
    head_count: AtomicU64,
    hold_get: AtomicBool,
    release_get: AtomicBool,
    entered_get: AtomicBool,
    dropped_get: AtomicBool,
    hold_bytes: AtomicBool,
    release_bytes: AtomicBool,
    entered_bytes: AtomicBool,
    dropped_bytes: AtomicBool,
    block: AtomicBool,
    block_started: AtomicBool,
    block_finished: AtomicBool,
    block_alive: AtomicBool,
    dropped_block: AtomicBool,
    waker_get: Mutex<Option<Waker>>,
    waker_bytes: Mutex<Option<Waker>>,
    waker_block: Mutex<Option<Waker>>,
}

impl ProbeLog {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            body: Mutex::new(Vec::new()),
            manifest: Mutex::new(Vec::new()),
            heads: Mutex::new(Vec::new()),
            body_gets: AtomicU64::new(0),
            body_unbounded: AtomicU64::new(0),
            body_over_8192: AtomicU64::new(0),
            flip_etag_at: AtomicU64::new(0),
            head_count: AtomicU64::new(0),
            hold_get: AtomicBool::new(false),
            release_get: AtomicBool::new(false),
            entered_get: AtomicBool::new(false),
            dropped_get: AtomicBool::new(false),
            hold_bytes: AtomicBool::new(false),
            release_bytes: AtomicBool::new(false),
            entered_bytes: AtomicBool::new(false),
            dropped_bytes: AtomicBool::new(false),
            block: AtomicBool::new(false),
            block_started: AtomicBool::new(false),
            block_finished: AtomicBool::new(false),
            block_alive: AtomicBool::new(false),
            dropped_block: AtomicBool::new(false),
            waker_get: Mutex::new(None),
            waker_bytes: Mutex::new(None),
            waker_block: Mutex::new(None),
        })
    }

    fn body_ranges(&self) -> Vec<SeenRange> {
        self.body.lock().expect("body ranges").clone()
    }

    fn manifest_ranges(&self) -> Vec<SeenRange> {
        self.manifest.lock().expect("manifest ranges").clone()
    }

    fn head_tags(&self) -> Vec<String> {
        self.heads.lock().expect("heads").clone()
    }
}

struct Probe {
    inner: Arc<dyn ObjectStore>,
    log: Arc<ProbeLog>,
}

impl Probe {
    fn open(root: &Path) -> (Arc<Self>, Arc<ProbeLog>) {
        let local = LocalFileSystem::new_with_prefix(root).unwrap();
        let log = ProbeLog::new();
        let probe = Arc::new(Self {
            inner: Arc::new(local),
            log: Arc::clone(&log),
        });
        (probe, log)
    }
}

impl std::fmt::Debug for Probe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Probe")
    }
}

impl std::fmt::Display for Probe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a209-probe")
    }
}

fn pin_box<'a, T: Send + 'a>(
    fut: impl Future<Output = T> + Send + 'a,
) -> Pin<Box<dyn Future<Output = T> + Send + 'a>> {
    Box::pin(fut)
}

fn classify(range: Option<&GetRange>) -> Result<SeenRange, ()> {
    match range {
        Some(GetRange::Bounded(span)) => Ok(SeenRange {
            start: span.start,
            end: span.end,
            len: span.end.saturating_sub(span.start),
        }),
        Some(GetRange::Offset(_)) | Some(GetRange::Suffix(_)) | None => Err(()),
    }
}

#[derive(Clone, Copy)]
enum Hold {
    Get,
    Bytes,
}

struct Pause {
    log: Arc<ProbeLog>,
    hold: Hold,
    completed: bool,
    started: bool,
}

impl Future for Pause {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let log = Arc::clone(&this.log);
        let hold = this.hold;
        let ready = {
            let (release, entered, waker_slot) = match hold {
                Hold::Get => (&log.release_get, &log.entered_get, &log.waker_get),
                Hold::Bytes => (&log.release_bytes, &log.entered_bytes, &log.waker_bytes),
            };
            entered.store(true, Ordering::SeqCst);
            let ready = release.load(Ordering::SeqCst);
            if !ready {
                *waker_slot.lock().expect("pause waker") = Some(cx.waker().clone());
            }
            ready
        };
        if ready {
            this.completed = true;
            return Poll::Ready(());
        }
        if !this.started {
            this.started = true;
            let log = Arc::clone(&log);
            std::thread::Builder::new()
                .name("oo-a209-hold".to_owned())
                .spawn(move || {
                    let release = match hold {
                        Hold::Get => &log.release_get,
                        Hold::Bytes => &log.release_bytes,
                    };
                    while !release.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let slot = match hold {
                        Hold::Get => &log.waker_get,
                        Hold::Bytes => &log.waker_bytes,
                    };
                    if let Some(waker) = slot.lock().expect("pause wake").take() {
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
        if !self.completed {
            let flag = match self.hold {
                Hold::Get => &self.log.dropped_get,
                Hold::Bytes => &self.log.dropped_bytes,
            };
            flag.store(true, Ordering::SeqCst);
        }
    }
}

struct WaitBlock {
    log: Arc<ProbeLog>,
    completed: bool,
    started: bool,
}

impl Future for WaitBlock {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.log.block_finished.load(Ordering::SeqCst) {
            this.completed = true;
            return Poll::Ready(());
        }
        *this.log.waker_block.lock().expect("block waker") = Some(cx.waker().clone());
        if !this.started {
            this.started = true;
            let log = Arc::clone(&this.log);
            std::thread::Builder::new()
                .name("oo-a209-block".to_owned())
                .spawn(move || {
                    log.block_alive.store(true, Ordering::SeqCst);
                    log.block_started.store(true, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(BLOCK_MS));
                    log.block_finished.store(true, Ordering::SeqCst);
                    log.block_alive.store(false, Ordering::SeqCst);
                    if let Some(waker) = log.waker_block.lock().expect("block wake").take() {
                        waker.wake();
                    }
                })
                .unwrap();
        }
        Poll::Pending
    }
}

impl Drop for WaitBlock {
    fn drop(&mut self) {
        if !self.completed {
            self.log.dropped_block.store(true, Ordering::SeqCst);
        }
    }
}

impl ObjectStore for Probe {
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
        let log = Arc::clone(&self.log);
        let location = location.clone();
        pin_box(async move {
            let key = location.to_string();
            let body = key.ends_with("/original");
            let manifest = key.contains("meta/manifest/");
            if options.head {
                let mut got = inner.get_opts(&location, options).await?;
                if body {
                    let index = log.head_count.fetch_add(1, Ordering::SeqCst);
                    let flip_at = log.flip_etag_at.load(Ordering::SeqCst);
                    let tag = if flip_at > 0 && index >= flip_at {
                        "a209-etag-b"
                    } else {
                        "a209-etag-a"
                    };
                    got.meta.e_tag = Some(tag.to_owned());
                    got.meta.version = None;
                    log.heads.lock().expect("heads").push(tag.to_owned());
                }
                return Ok(got);
            }
            if body || manifest {
                match classify(options.range.as_ref()) {
                    Ok(seen) => {
                        if body {
                            log.body_gets.fetch_add(1, Ordering::SeqCst);
                            if seen.len > 8192 {
                                log.body_over_8192.fetch_add(1, Ordering::SeqCst);
                            }
                            log.body.lock().expect("body").push(seen);
                        } else {
                            log.manifest.lock().expect("manifest").push(seen);
                        }
                    }
                    Err(()) => {
                        if body {
                            log.body_unbounded.fetch_add(1, Ordering::SeqCst);
                            log.body_gets.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            }
            if body && log.block.load(Ordering::SeqCst) {
                WaitBlock {
                    log: Arc::clone(&log),
                    completed: false,
                    started: false,
                }
                .await;
            }
            if body && log.hold_get.load(Ordering::SeqCst) {
                Pause {
                    log: Arc::clone(&log),
                    hold: Hold::Get,
                    completed: false,
                    started: false,
                }
                .await;
            }
            if body && log.hold_bytes.load(Ordering::SeqCst) {
                let got = inner.get_opts(&location, options).await?;
                let range = got.range.clone();
                let meta = got.meta.clone();
                let held = Arc::clone(&log);
                let stream = stream::once(async move {
                    Pause {
                        log: held,
                        hold: Hold::Bytes,
                        completed: false,
                        started: false,
                    }
                    .await;
                    let payload = got.bytes().await?;
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

#[cfg(target_os = "linux")]
async fn memory_child() {
    let root = PathBuf::from(std::env::var("OO_A209_ROOT").unwrap());
    let key = oo_control::ObjectKey::parse(std::env::var("OO_A209_KEY").unwrap()).unwrap();
    let expected = std::env::var("OO_A209_SHA").unwrap();
    let size: usize = std::env::var("OO_A209_SIZE").unwrap().parse().unwrap();
    let metrics_path = PathBuf::from(std::env::var("OO_A209_METRICS").unwrap());
    assert!(size == 8 * 1024 * 1024 || size == 32 * 1024 * 1024);
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission();
    let mut reader = host
        .open_download(
            &ctx,
            &key,
            8192,
            Some(Instant::now() + Duration::from_secs(180)),
        )
        .await
        .unwrap();
    let hwm_before = proc_kib("VmHWM:");
    let rss_before = proc_kib("VmRSS:");
    let peak_before = proc_kib("VmPeak:");
    let mut hasher = Sha256::new();
    let mut pulled = 0u64;
    let mut chunk_max = 0usize;
    loop {
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
    let download_sha = finish(hasher);
    let file_sha = hash_file(&root.join(key.as_str()));
    let hwm_after = proc_kib("VmHWM:");
    let rss_after = proc_kib("VmRSS:");
    let peak_after = proc_kib("VmPeak:");
    let hwm_delta = hwm_after.saturating_sub(hwm_before) * 1024;
    let complete = u8::from(
        reader.is_complete() && hex(&download_sha) == expected && hex(&file_sha) == expected,
    );
    let text = format!(
        "download_bytes={size}\npattern_byte={PATTERN}\nobject_key={}\ndownload_sha256={}\npattern_sha256={expected}\nfile_sha256={}\ndownload_hwm_before_kib={hwm_before}\ndownload_hwm_after_kib={hwm_after}\ndownload_hwm_delta_bytes={hwm_delta}\ndownload_rss_before_kib={rss_before}\ndownload_rss_after_kib={rss_after}\ndownload_vmpeak_before_kib={peak_before}\ndownload_vmpeak_after_kib={peak_after}\ndownload_chunk_max={chunk_max}\ndownload_pulled={pulled}\ndownload_complete={complete}\ncaller_retained_chunks=1\nfixture_object_vec=0\n",
        key.as_str(),
        hex(&download_sha),
        hex(&file_sha),
    );
    std::fs::write(&metrics_path, &text).unwrap();
    assert_eq!(complete, 1, "{text}");
    assert!(hwm_delta < 4 * 1024 * 1024, "{text}");
    assert!(hwm_delta.saturating_mul(2) < size as u64, "{text}");
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
fn metric_str(text: &str, key: &str) -> String {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            return rest.trim().to_owned();
        }
    }
    panic!("missing {key}");
}

#[cfg(target_os = "linux")]
fn metric_u64(text: &str, key: &str) -> u64 {
    metric_str(text, key).parse().unwrap()
}

#[cfg(target_os = "linux")]
fn spawn_child(test_name: &str, vars: &[(&str, &str)]) -> std::process::Child {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.arg(test_name)
        .arg("--exact")
        .arg("--test-threads=1")
        .env("OO_A209_CHILD", "1")
        .env("OO_A209_TEST", test_name)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in vars {
        cmd.env(key, value);
    }
    cmd.spawn().unwrap()
}

#[cfg(target_os = "linux")]
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
