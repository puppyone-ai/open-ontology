//! Measures whether `complete` keeps the local store's 8 KiB stream chunk.
//!
//! The part file is written in 8 KiB pieces. A direct `into_stream` read records
//! the largest chunk the store yields. `complete` then runs, and VmHWM is compared
//! with the value taken after that streaming read.

#![forbid(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStoreExt;
use oo_control::{AdmissionContext, FileHost, GrantEvidence, HostLimits, PrincipalId, ProjectId, UploadConfig};
use sha2::{Digest, Sha256};

const OBJECT_BYTES: usize = 8 * 1024 * 1024;
const CHUNK: usize = 8 * 1024;

#[tokio::test]
async fn complete_allocates_the_whole_part() {
    let scratch = Scratch::new();
    let root = scratch.store();
    let digest = pattern_digest(OBJECT_BYTES);
    let limits = HostLimits::try_new(
        OBJECT_BYTES as u64,
        OBJECT_BYTES as u64,
        4,
        OBJECT_BYTES as u64,
    )
    .unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let ctx = AdmissionContext {
        project_id: ProjectId::new("project_m").unwrap(),
        principal_id: PrincipalId::new("user_m").unwrap(),
        grant_evidence: GrantEvidence::new("grant-m").unwrap(),
    };
    let cfg = UploadConfig {
        display_name: "block.bin".to_owned(),
        expected_sha256: digest,
        expected_size: OBJECT_BYTES as u64,
        content_type: None,
    };
    let session = host.begin(&ctx, &cfg).await.unwrap();
    let part_rel = format!("staging/{}/part/00001", session.session_id.as_str());
    write_pattern(&root.join(&part_rel), OBJECT_BYTES);

    let raw = LocalFileSystem::new_with_prefix(&root).unwrap();
    let got = raw
        .get(&ObjectPath::parse(&part_rel).unwrap())
        .await
        .unwrap();
    let mut stream = got.into_stream();
    let mut max_chunk = 0usize;
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("stream chunk");
        max_chunk = max_chunk.max(chunk.len());
        total += chunk.len();
    }
    drop(stream);
    let hwm_before = vm_kib("VmHWM");
    let rss_before = vm_kib("VmRSS");
    let manifest = host.complete(&ctx, &session.session_id).await.unwrap();
    let hwm_after = vm_kib("VmHWM");
    let rss_after = vm_kib("VmRSS");
    let delta = hwm_after.saturating_sub(hwm_before).saturating_mul(1024);
    let lines = [
        format!("memory_object_bytes={OBJECT_BYTES}"),
        format!("memory_store_stream_max_chunk={max_chunk}"),
        format!("memory_store_stream_total={total}"),
        format!("memory_hwm_before_kib={hwm_before}"),
        format!("memory_hwm_after_kib={hwm_after}"),
        format!("memory_hwm_delta_bytes={delta}"),
        format!("memory_rss_before_kib={rss_before}"),
        format!("memory_rss_after_kib={rss_after}"),
        format!("memory_manifest_size={}", manifest.size),
    ];
    for line in &lines {
        println!("oracle_note {line}");
    }
    if let Ok(path) = std::env::var("OO_REVIEW_OUT") {
        let mut file = std::fs::File::create(path).unwrap();
        for line in &lines {
            writeln!(file, "{line}").unwrap();
        }
    }
    assert_eq!(total, OBJECT_BYTES);
    assert_eq!(max_chunk, CHUNK, "direct LocalFileSystem stream chunk");
    assert_eq!(manifest.object_digest, digest);
    assert_eq!(manifest.size, OBJECT_BYTES as u64);
    assert!(
        delta >= OBJECT_BYTES as u64,
        "complete VmHWM delta {delta} is below the {OBJECT_BYTES}-byte part"
    );
}

fn write_pattern(path: &Path, n: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(path).unwrap();
    let chunk = [0xA5u8; CHUNK];
    let mut left = n;
    while left > 0 {
        let k = left.min(CHUNK);
        file.write_all(&chunk[..k]).unwrap();
        left -= k;
    }
    file.sync_all().unwrap();
}

fn pattern_digest(n: usize) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let chunk = [0xA5u8; CHUNK];
    let mut left = n;
    while left > 0 {
        let k = left.min(CHUNK);
        hasher.update(&chunk[..k]);
        left -= k;
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn vm_kib(field: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap();
    let prefix = format!("{field}:");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&prefix) {
            return rest.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("missing {field}");
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("oo-a183-review-memory-{}", std::process::id()));
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
