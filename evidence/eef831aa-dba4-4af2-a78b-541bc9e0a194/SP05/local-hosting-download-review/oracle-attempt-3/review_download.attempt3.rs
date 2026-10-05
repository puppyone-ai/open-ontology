//! Independent review of the local download pull.
//!
//! The harness is Safe Rust. Digests are `sha2`. The product sources are not edited.

#![forbid(unsafe_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use futures_util::stream::{BoxStream, StreamExt};
use object_store::path::Path as StorePath;
use object_store::{
    Attributes, CopyOptions, GetOptions, GetRange, GetResult, GetResultPayload, ListResult,
    MultipartUpload, ObjectMeta, ObjectStore, PutMultipartOptions, PutOptions, PutPayload,
    PutResult,
};
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectKey,
    ObjectManifest, PrincipalId, ProjectId, UploadConfig,
};
use sha2::{Digest, Sha256};

const STOPPED: &str = "download stopped before the object was verified";
const PATTERN: u8 = 0x3C;

fn scratch(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oo-a194-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn admission(project: &str, principal: &str, grant: &str) -> AdmissionContext {
    AdmissionContext {
        project_id: ProjectId::new(project).unwrap(),
        principal_id: PrincipalId::new(principal).unwrap(),
        grant_evidence: GrantEvidence::new(grant).unwrap(),
    }
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

fn hash_file(path: &std::path::Path) -> [u8; 32] {
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
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn pattern_digest(size: usize) -> [u8; 32] {
    let chunk = [PATTERN; 8192];
    let mut hasher = Sha256::new();
    let mut left = size;
    while left > 0 {
        let n = 8192.min(left);
        hasher.update(&chunk[..n]);
        left -= n;
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn write_note(name: &str, body: &str) {
    let Ok(dir) = std::env::var("OO_A194_NOTES") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), body).unwrap();
}

fn quota_path(root: &std::path::Path, project: &str, session: &str) -> std::path::PathBuf {
    root.join(format!("meta/quota/{project}/{session}"))
}

fn quota_names(root: &std::path::Path, project: &str) -> Vec<String> {
    let dir = root.join(format!("meta/quota/{project}"));
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        names.push(entry.unwrap().file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

async fn publish(
    host: &FileHost,
    ctx: &AdmissionContext,
    name: &str,
    bytes: &[u8],
) -> ObjectManifest {
    let config = UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: sha256(bytes),
        expected_size: u64::try_from(bytes.len()).unwrap(),
        content_type: None,
    };
    let session = host.begin(ctx, &config).await.unwrap();
    host.put_part(ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    host.complete(ctx, &session.session_id).await.unwrap()
}

fn manifest_file(root: &std::path::Path, manifest: &ObjectManifest) -> std::path::PathBuf {
    root.join(format!("meta/manifest/{}", manifest.session_id.as_str()))
}

fn object_file(root: &std::path::Path, manifest: &ObjectManifest) -> std::path::PathBuf {
    root.join(manifest.object_key.as_str())
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

fn push_key(out: &mut String, key: &str, comma: bool) {
    if comma {
        out.push(',');
    }
    push_json_string(out, key);
}

fn encode_manifest(manifest: &ObjectManifest) -> Vec<u8> {
    let mut out = String::new();
    out.push('{');
    push_key(&mut out, "content_type", false);
    out.push(':');
    match &manifest.content_type {
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

fn lineage_of(
    manifest: &ObjectManifest,
    object_key: &str,
    size: u64,
    digest: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"oo.file-hosting/v1 lineage\0");
    let feed = |hasher: &mut Sha256, value: &str| {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    };
    feed(&mut hasher, manifest.session_id.as_str());
    feed(&mut hasher, manifest.project_id.as_str());
    feed(&mut hasher, manifest.principal_id.as_str());
    feed(&mut hasher, manifest.grant_evidence.as_str());
    feed(&mut hasher, object_key);
    feed(&mut hasher, &hex(&digest));
    feed(&mut hasher, &size.to_string());
    feed(&mut hasher, &manifest.part_count.to_string());
    feed(&mut hasher, manifest.idempotency_key.as_str());
    feed(&mut hasher, &manifest.display_name);
    feed(&mut hasher, manifest.content_type.as_deref().unwrap_or(""));
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn assert_snapshot(live: &ObjectManifest, opened: &ObjectManifest) {
    assert_eq!(live.contract, opened.contract);
    assert_eq!(live.object_key, opened.object_key);
    assert_eq!(live.session_id, opened.session_id);
    assert_eq!(live.project_id, opened.project_id);
    assert_eq!(live.principal_id, opened.principal_id);
    assert_eq!(live.object_digest, opened.object_digest);
    assert_eq!(live.size, opened.size);
    assert_eq!(live.part_count, opened.part_count);
    assert_eq!(live.idempotency_key, opened.idempotency_key);
    assert_eq!(live.lineage_digest, opened.lineage_digest);
    assert_eq!(live, opened);
}

struct Gate {
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
    head_gets: AtomicU64,
    body_calls: AtomicU64,
    body_finished: AtomicU64,
    body_bytes: AtomicU64,
    body_max: AtomicU64,
    body_unbounded: AtomicU64,
    log_ranges: AtomicBool,
    ranges: Mutex<Vec<(u64, u64)>>,
    waker_get: Mutex<Option<Waker>>,
    waker_bytes: Mutex<Option<Waker>>,
}

impl Gate {
    fn new(log_ranges: bool) -> Arc<Self> {
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
            head_gets: AtomicU64::new(0),
            body_calls: AtomicU64::new(0),
            body_finished: AtomicU64::new(0),
            body_bytes: AtomicU64::new(0),
            body_max: AtomicU64::new(0),
            body_unbounded: AtomicU64::new(0),
            log_ranges: AtomicBool::new(log_ranges),
            ranges: Mutex::new(Vec::new()),
            waker_get: Mutex::new(None),
            waker_bytes: Mutex::new(None),
        })
    }

    fn note_body(&self, start: u64, end: u64) {
        let len = end.saturating_sub(start);
        self.body_calls.fetch_add(1, Ordering::SeqCst);
        self.body_bytes.fetch_add(len, Ordering::SeqCst);
        self.body_max.fetch_max(len, Ordering::SeqCst);
        if self.log_ranges.load(Ordering::SeqCst) {
            self.ranges.lock().unwrap().push((start, end));
        }
    }
}

struct ReleaseBoth<'a> {
    get: &'a AtomicBool,
    bytes: &'a AtomicBool,
}

impl Drop for ReleaseBoth<'_> {
    fn drop(&mut self) {
        self.get.store(true, Ordering::SeqCst);
        self.bytes.store(true, Ordering::SeqCst);
    }
}

enum Hold {
    Get,
    Bytes,
}

struct Pause {
    hold: Hold,
    gate: Arc<Gate>,
    completed: bool,
    started: bool,
}

impl Pause {
    fn flag<'a>(
        gate: &'a Gate,
        hold: &Hold,
    ) -> (
        &'a AtomicBool,
        &'a AtomicBool,
        &'a AtomicBool,
        &'a AtomicBool,
        &'a Mutex<Option<Waker>>,
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
        let (release, entered, done, _dropped, waker_slot) = Self::flag(&this.gate, &this.hold);
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
            let hold = match this.hold {
                Hold::Get => Hold::Get,
                Hold::Bytes => Hold::Bytes,
            };
            let _join = std::thread::Builder::new()
                .name("oo-a194-hold".to_owned())
                .spawn(move || {
                    let (release, _entered, done, _dropped, waker_slot) = Pause::flag(&gate, &hold);
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
        let (_release, _entered, _done, dropped, _waker) = Self::flag(&self.gate, &self.hold);
        dropped.store(true, Ordering::SeqCst);
    }
}

struct CountingStore {
    inner: Arc<dyn ObjectStore>,
    gate: Arc<Gate>,
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

impl CountingStore {
    fn open(root: &std::path::Path, log_ranges: bool) -> (Arc<Self>, Arc<Gate>) {
        let local = object_store::local::LocalFileSystem::new_with_prefix(root).unwrap();
        let gate = Gate::new(log_ranges);
        let store = Arc::new(Self {
            inner: Arc::new(local),
            gate: Arc::clone(&gate),
        });
        (store, gate)
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
            if options.head {
                gate.head_gets.fetch_add(1, Ordering::SeqCst);
            }
            if body {
                match &options.range {
                    Some(GetRange::Bounded(range)) => gate.note_body(range.start, range.end),
                    Some(_) | None => {
                        gate.body_unbounded.fetch_add(1, Ordering::SeqCst);
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
                gate.body_finished.fetch_add(1, Ordering::SeqCst);
                let range = got.range.clone();
                let meta = got.meta.clone();
                let payload = got.bytes().await?;
                let gate_stream = Arc::clone(&gate);
                let stream = futures_util::stream::once(async move {
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
            let got = inner.get_opts(&location, options).await?;
            if body {
                gate.body_finished.fetch_add(1, Ordering::SeqCst);
            }
            Ok(got)
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
            Poll::Ready(_) => panic!("download future finished before the hold was entered"),
        }
        if start.elapsed() > Duration::from_secs(20) {
            panic!("timed out waiting for the held get");
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
                if start.elapsed() > Duration::from_secs(30) {
                    panic!("timed out waiting for the held get to finish");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn host_on(root: &std::path::Path) -> FileHost {
    FileHost::open_local(root, HostLimits::standard()).unwrap()
}

async fn pull_checked(
    host: &FileHost,
    ctx: &AdmissionContext,
    manifest: &ObjectManifest,
    budget: usize,
    source: &[u8],
    gate: &Gate,
) -> (Vec<usize>, [u8; 32]) {
    let calls_before = gate.body_calls.load(Ordering::SeqCst);
    let mut reader = host
        .open_download(ctx, &manifest.object_key, budget, None)
        .await
        .unwrap();
    assert!(!reader.is_complete());
    assert_snapshot(reader.manifest(), manifest);
    let mut hasher = Sha256::new();
    let mut widths = Vec::new();
    let mut offset = 0usize;
    loop {
        assert_snapshot(reader.manifest(), manifest);
        match reader.next_chunk().await.unwrap() {
            Some(chunk) => {
                assert!(!chunk.is_empty());
                assert!(chunk.len() <= budget);
                assert!(chunk.len() <= 8192);
                assert!(!reader.is_complete());
                assert_eq!(&source[offset..offset + chunk.len()], chunk.as_slice());
                hasher.update(&chunk);
                offset += chunk.len();
                widths.push(chunk.len());
            }
            None => break,
        }
    }
    assert_eq!(offset, source.len());
    assert!(reader.is_complete());
    assert_snapshot(reader.manifest(), manifest);
    let calls_at_end = gate.body_calls.load(Ordering::SeqCst);
    assert!(reader.next_chunk().await.unwrap().is_none());
    assert!(reader.is_complete());
    assert_eq!(
        gate.body_calls.load(Ordering::SeqCst),
        calls_at_end,
        "second EOF read the object again"
    );
    assert!(calls_at_end > calls_before);
    reader.cancel();
    assert!(reader.is_complete());
    assert!(reader.next_chunk().await.unwrap().is_none());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    assert_eq!(out, manifest.object_digest);
    assert_eq!(out, sha256(source));
    (widths, out)
}

#[tokio::test]
async fn review_download_exact_pull_and_refusals() {
    let root = scratch("exact");
    let publisher = host_on(&root);
    let ctx = admission("project_a", "user_a", "grant-review");
    let other_grant = admission("project_a", "user_a", "grant-other-string");
    let other_project = admission("project_b", "user_a", "grant-review");
    let other_principal = admission("project_a", "user_b", "grant-review");
    let source = b"abcdefghij0123456789".to_vec();
    let manifest = publish(&publisher, &ctx, "pull.bin", &source).await;
    assert_eq!(
        std::fs::read(manifest_file(&root, &manifest)).unwrap(),
        encode_manifest(&manifest)
    );
    assert_eq!(
        lineage_of(
            &manifest,
            manifest.object_key.as_str(),
            manifest.size,
            manifest.object_digest
        ),
        manifest.lineage_digest
    );

    let (store, gate) = CountingStore::open(&root, true);
    let host = FileHost::new(store, HostLimits::standard());
    let quota_before = std::fs::read(quota_path(
        &root,
        ctx.project_id.as_str(),
        manifest.session_id.as_str(),
    ))
    .unwrap();
    let names_before = quota_names(&root, ctx.project_id.as_str());

    let missing = ObjectKey::parse(format!(
        "uploads/{}/{}/original",
        "ab".repeat(16),
        "cd".repeat(32)
    ))
    .unwrap();
    let calls = gate.body_calls.load(Ordering::SeqCst);
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
    assert_eq!(gate.body_calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        ObjectKey::parse("../secret").unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        ObjectKey::parse("/etc/passwd").unwrap_err(),
        FileHostError::NotFound
    );

    let (widths, digest) = pull_checked(&host, &ctx, &manifest, 8, &source, &gate).await;
    assert_eq!(widths, vec![8, 8, 4]);
    let ranges = gate.ranges.lock().unwrap().clone();
    assert_eq!(ranges, vec![(0, 8), (8, 16), (16, 20)]);
    assert!(ranges.iter().all(|(start, end)| end - start <= 8192));
    assert_eq!(gate.body_unbounded.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read(quota_path(
            &root,
            ctx.project_id.as_str(),
            manifest.session_id.as_str()
        ))
        .unwrap(),
        quota_before
    );

    gate.ranges.lock().unwrap().clear();
    let (one_widths, _) = pull_checked(&host, &ctx, &manifest, 1, &source, &gate).await;
    assert_eq!(
        one_widths,
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    );
    gate.ranges.lock().unwrap().clear();
    let (three_widths, _) = pull_checked(&host, &other_grant, &manifest, 3, &source, &gate).await;
    assert_eq!(three_widths, vec![3, 3, 3, 3, 3, 3, 2]);
    gate.ranges.lock().unwrap().clear();
    let (wide, _) = pull_checked(&host, &ctx, &manifest, 8192, &source, &gate).await;
    assert_eq!(wide, vec![20]);
    let wide_ranges = gate.ranges.lock().unwrap().clone();
    assert_eq!(wide_ranges, vec![(0, 20)]);
    assert!(
        wide_ranges
            .iter()
            .all(|(start, end)| end - start <= 8192 && *start < *end)
    );

    let nine: Vec<u8> = (0..9000).map(|index| (index % 251) as u8).collect();
    let nine_manifest = publish(&publisher, &ctx, "nine.bin", &nine).await;
    let names_after_publish = quota_names(&root, ctx.project_id.as_str());
    assert_eq!(names_after_publish.len(), names_before.len() + 1);
    let nine_quota = std::fs::read(quota_path(
        &root,
        ctx.project_id.as_str(),
        nine_manifest.session_id.as_str(),
    ))
    .unwrap();
    gate.ranges.lock().unwrap().clear();
    let calls_before = gate.body_calls.load(Ordering::SeqCst);
    let (nine_widths, nine_hash) =
        pull_checked(&host, &ctx, &nine_manifest, 8192, &nine, &gate).await;
    assert_eq!(nine_widths, vec![8192, 808]);
    let nine_ranges = gate.ranges.lock().unwrap().clone();
    assert_eq!(nine_ranges, vec![(0, 8192), (8192, 9000)]);
    assert!(
        nine_ranges
            .iter()
            .all(|(start, end)| end - start <= 8192 && end - start < 9000)
    );
    assert_eq!(end_len(&nine_ranges), 9000);
    assert!(gate.body_calls.load(Ordering::SeqCst) - calls_before == 2);
    assert_eq!(nine_hash, sha256(&nine));

    let object_before = std::fs::read(object_file(&root, &manifest)).unwrap();
    let denied_project = host
        .open_download(&other_project, &manifest.object_key, 8192, None)
        .await
        .unwrap_err();
    let denied_principal = host
        .open_download(&other_principal, &manifest.object_key, 8192, None)
        .await
        .unwrap_err();
    assert_eq!(denied_project, FileHostError::Denied);
    assert_eq!(denied_principal, FileHostError::Denied);
    assert!(!denied_project.to_string().contains('/'));
    assert!(!denied_project.to_string().contains("grant-review"));
    assert_eq!(
        std::fs::read(object_file(&root, &manifest)).unwrap(),
        object_before
    );
    assert_eq!(
        quota_names(&root, ctx.project_id.as_str()),
        names_after_publish
    );
    assert_eq!(
        std::fs::read(quota_path(
            &root,
            ctx.project_id.as_str(),
            nine_manifest.session_id.as_str(),
        ))
        .unwrap(),
        nine_quota
    );

    let mut early = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    let first = early.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.as_slice(), &source[..8]);
    assert!(!early.is_complete());
    drop(early);
    assert_eq!(
        std::fs::read(object_file(&root, &manifest)).unwrap(),
        object_before
    );
    let (_again, _) = pull_checked(&host, &ctx, &manifest, 8, &source, &gate).await;

    let mut cancelling = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    assert!(cancelling.next_chunk().await.unwrap().is_some());
    let calls_mid = gate.body_calls.load(Ordering::SeqCst);
    cancelling.cancel();
    let stopped = cancelling.next_chunk().await.unwrap_err();
    assert_eq!(stopped, FileHostError::Stopped);
    assert_eq!(stopped.to_string(), STOPPED);
    assert!(!stopped.to_string().contains('/'));
    assert!(!cancelling.is_complete());
    assert_snapshot(cancelling.manifest(), &manifest);
    assert_eq!(
        cancelling.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert_eq!(gate.body_calls.load(Ordering::SeqCst), calls_mid);

    let mut expired = host
        .open_download(
            &ctx,
            &manifest.object_key,
            8,
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .await
        .unwrap();
    let calls_expired = gate.body_calls.load(Ordering::SeqCst);
    assert_eq!(
        expired.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!expired.is_complete());
    assert_eq!(gate.body_calls.load(Ordering::SeqCst), calls_expired);

    let mut timed = host
        .open_download(
            &ctx,
            &manifest.object_key,
            8,
            Some(Instant::now() + Duration::from_millis(300)),
        )
        .await
        .unwrap();
    assert!(timed.next_chunk().await.unwrap().is_some());
    assert!(!timed.is_complete());
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        timed.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert!(!timed.is_complete());
    assert_snapshot(timed.manifest(), &manifest);

    let shown = {
        let reader = host
            .open_download(&ctx, &manifest.object_key, 8, None)
            .await
            .unwrap();
        format!("{reader:?}")
    };
    assert!(!shown.contains("grant-review"));
    assert!(!shown.contains(root.to_str().unwrap()));

    let stored = host.read_object(&ctx, &manifest.object_key).await.unwrap();
    assert_eq!(stored.bytes, source);
    assert_eq!(stored.bytes.len(), source.len());
    assert_eq!(stored.manifest, manifest);

    let mut note = String::new();
    note.push_str(&format!("exact_sha256={}\n", hex(&digest)));
    note.push_str("budget_8_widths=8,8,4\n");
    note.push_str("budget_1_chunks=20\n");
    note.push_str("budget_3_last=2\n");
    note.push_str("budget_8192_on_20=20\n");
    note.push_str("budget_8192_on_9000=8192,808\n");
    note.push_str("second_eof_extra_body_gets=0\n");
    note.push_str("early_complete=0\n");
    note.push_str("cancel_after_complete_clears=0\n");
    note.push_str("other_grant_opens=1\n");
    note.push_str("grant_authenticated=0\n");
    note.push_str("read_object_returns_full_buffer=1\n");
    note.push_str(&format!(
        "quota_unchanged={}\n",
        u8::from(
            std::fs::read(quota_path(
                &root,
                ctx.project_id.as_str(),
                manifest.session_id.as_str()
            ))
            .unwrap()
                == quota_before
        )
    ));
    write_note("exact.txt", &note);
}

fn end_len(ranges: &[(u64, u64)]) -> u64 {
    ranges.iter().map(|(start, end)| end - start).sum()
}

#[tokio::test]
async fn review_download_mutations_and_snapshot() {
    let root = scratch("mutate");
    let publisher = host_on(&root);
    let ctx = admission("project_a", "user_a", "grant-mutate");
    let source = b"abcdefghij0123456789".to_vec();
    let (store, gate) = CountingStore::open(&root, false);
    let host = FileHost::new(store, HostLimits::standard());

    let flipped = publish(&publisher, &ctx, "flip.bin", &source).await;
    let flip_path = object_file(&root, &flipped);
    let mut raw = std::fs::read(&flip_path).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 0x5a;
    std::fs::write(&flip_path, &raw).unwrap();
    let mut reader = host
        .open_download(&ctx, &flipped.object_key, 8, None)
        .await
        .unwrap();
    let mut saw_chunk = false;
    let err = loop {
        match reader.next_chunk().await {
            Ok(Some(chunk)) => {
                assert!(!chunk.is_empty());
                saw_chunk = true;
                assert!(!reader.is_complete());
                assert_snapshot(reader.manifest(), &flipped);
            }
            Ok(None) => panic!("unread hash flip was reported complete"),
            Err(error) => break error,
        }
    };
    assert!(saw_chunk);
    assert_eq!(err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_snapshot(reader.manifest(), &flipped);

    let consumed = publish(&publisher, &ctx, "consumed.bin", &source).await;
    let consumed_path = object_file(&root, &consumed);
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
    let mut hasher = Sha256::new();
    hasher.update(&first);
    let mut offset = first.len();
    let consumed_end = loop {
        match reader.next_chunk().await {
            Ok(Some(chunk)) => {
                assert!(!reader.is_complete());
                hasher.update(&chunk);
                offset += chunk.len();
            }
            Ok(None) => break None,
            Err(error) => break Some(error),
        }
    };
    let pulled = {
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    };
    let file_now = hash_file(&consumed_path);
    let consumed_complete = reader.is_complete();
    assert_eq!(offset, source.len());
    assert_eq!(pulled, consumed.object_digest);
    assert_ne!(file_now, consumed.object_digest);
    assert!(consumed_end.is_none());
    assert!(consumed_complete);

    let grown = publish(&publisher, &ctx, "grow.bin", &source).await;
    let mut reader = host
        .open_download(&ctx, &grown.object_key, 8, None)
        .await
        .unwrap();
    let mut seen = 0usize;
    while seen < source.len() {
        let chunk = reader.next_chunk().await.unwrap().unwrap();
        assert!(!reader.is_complete());
        seen += chunk.len();
    }
    assert!(!reader.is_complete());
    let mut grown_bytes = std::fs::read(object_file(&root, &grown)).unwrap();
    grown_bytes.push(0x11);
    std::fs::write(object_file(&root, &grown), &grown_bytes).unwrap();
    assert_eq!(
        reader.next_chunk().await.unwrap_err(),
        FileHostError::Corrupt
    );
    assert!(!reader.is_complete());
    assert_snapshot(reader.manifest(), &grown);

    let cut = publish(&publisher, &ctx, "cut.bin", &source).await;
    let mut reader = host
        .open_download(&ctx, &cut.object_key, 8, None)
        .await
        .unwrap();
    assert!(reader.next_chunk().await.unwrap().is_some());
    std::fs::OpenOptions::new()
        .write(true)
        .open(object_file(&root, &cut))
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
    let cut_kind = format!("{cut_err:?}");

    let changed = publish(&publisher, &ctx, "manifest.bin", &source).await;
    let manifest_path = manifest_file(&root, &changed);
    let mut reader = host
        .open_download(&ctx, &changed.object_key, 8, None)
        .await
        .unwrap();
    let mut manifest_bytes = std::fs::read(&manifest_path).unwrap();
    manifest_bytes[20] ^= 0x01;
    std::fs::write(&manifest_path, &manifest_bytes).unwrap();
    let manifest_err = loop {
        match reader.next_chunk().await {
            Ok(Some(_)) => assert!(!reader.is_complete()),
            Ok(None) => panic!("changed manifest was reported complete"),
            Err(error) => break error,
        }
    };
    assert_eq!(manifest_err, FileHostError::Corrupt);
    assert!(!reader.is_complete());
    assert_snapshot(reader.manifest(), &changed);

    let renamed = publish(&publisher, &ctx, "rename.bin", &source).await;
    let other_key = format!(
        "uploads/{}/{}/original",
        renamed.session_id.as_str(),
        "ab".repeat(32)
    );
    let mut forged = renamed.clone();
    forged.object_key = ObjectKey::parse(&other_key).unwrap();
    forged.lineage_digest = lineage_of(&forged, &other_key, forged.size, forged.object_digest);
    let encoded = encode_manifest(&forged);
    std::fs::write(manifest_file(&root, &renamed), &encoded).unwrap();
    let object_before = std::fs::read(object_file(&root, &renamed)).unwrap();
    assert_eq!(
        host.open_download(&ctx, &renamed.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );
    assert_eq!(
        std::fs::read(object_file(&root, &renamed)).unwrap(),
        object_before
    );

    let zeroed = publish(&publisher, &ctx, "zero.bin", &source).await;
    let mut forged_zero = zeroed.clone();
    forged_zero.size = 0;
    forged_zero.lineage_digest = lineage_of(
        &forged_zero,
        forged_zero.object_key.as_str(),
        0,
        forged_zero.object_digest,
    );
    std::fs::write(manifest_file(&root, &zeroed), encode_manifest(&forged_zero)).unwrap();
    assert_eq!(
        host.open_download(&ctx, &zeroed.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::Corrupt
    );

    let empty = publish(&publisher, &ctx, "empty-manifest.bin", &source).await;
    std::fs::OpenOptions::new()
        .write(true)
        .open(manifest_file(&root, &empty))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert_eq!(
        host.open_download(&ctx, &empty.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::Corrupt
    );

    let huge = publish(&publisher, &ctx, "huge-manifest.bin", &source).await;
    std::fs::write(manifest_file(&root, &huge), vec![b'x'; 4097]).unwrap();
    assert_eq!(
        host.open_download(&ctx, &huge.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::Corrupt
    );

    let gone = publish(&publisher, &ctx, "gone.bin", &source).await;
    std::fs::remove_file(manifest_file(&root, &gone)).unwrap();
    assert_eq!(
        host.open_download(&ctx, &gone.object_key, 8192, None)
            .await
            .unwrap_err(),
        FileHostError::NotFound
    );

    let mut note = String::new();
    note.push_str("unread_hash_flip=Corrupt\n");
    note.push_str("unread_hash_flip_complete=0\n");
    note.push_str(&format!(
        "consumed_prefix_flip_complete={}\n",
        u8::from(consumed_complete)
    ));
    note.push_str("consumed_prefix_running_hash_matches_snapshot=1\n");
    note.push_str("consumed_prefix_stored_file_matches_snapshot=0\n");
    note.push_str("append_before_eof=Corrupt\n");
    note.push_str(&format!("truncate_after_first_chunk={cut_kind}\n"));
    note.push_str("changed_manifest=Corrupt\n");
    note.push_str("snapshot_replaced=0\n");
    note.push_str("different_object_key=NotFound\n");
    note.push_str("forged_size_0=Corrupt\n");
    note.push_str("empty_manifest=Corrupt\n");
    note.push_str("manifest_above_4096=Corrupt\n");
    note.push_str("missing_manifest=NotFound\n");
    note.push_str(&format!(
        "body_unbounded={}\n",
        gate.body_unbounded.load(Ordering::SeqCst)
    ));
    write_note("mutations.txt", &note);
}

#[tokio::test]
async fn review_download_generic_store_leaves_quota() {
    let root = scratch("generic");
    let publisher = host_on(&root);
    let ctx = admission("project_a", "user_a", "grant-generic");
    let source = b"generic-object-bytes".to_vec();
    let manifest = publish(&publisher, &ctx, "generic.bin", &source).await;
    let quota = quota_path(&root, ctx.project_id.as_str(), manifest.session_id.as_str());
    let quota_before = std::fs::read(&quota).unwrap();
    let names_before = quota_names(&root, ctx.project_id.as_str());
    let (store, _gate) = CountingStore::open(&root, false);
    let host = FileHost::new(store, HostLimits::standard());
    let fresh = UploadConfig {
        display_name: "other.bin".to_owned(),
        expected_sha256: sha256(b"zzzzzzzz"),
        expected_size: 8,
        content_type: None,
    };
    assert_eq!(
        host.begin(&ctx, &fresh).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        host.put_part(&ctx, &manifest.session_id, 1, b"zzzzzzzz")
            .await
            .unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        host.complete(&ctx, &manifest.session_id).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(
        host.abort(&ctx, &manifest.session_id).await.unwrap_err(),
        FileHostError::Unsupported
    );
    assert_eq!(std::fs::read(&quota).unwrap(), quota_before);
    assert_eq!(quota_names(&root, ctx.project_id.as_str()), names_before);
    let (_widths, digest) = pull_checked(&host, &ctx, &manifest, 8, &source, &_gate).await;
    assert_eq!(digest, sha256(&source));
    assert_eq!(std::fs::read(&quota).unwrap(), quota_before);
    assert_eq!(quota_names(&root, ctx.project_id.as_str()), names_before);
    let stored = host.read_object(&ctx, &manifest.object_key).await.unwrap();
    assert_eq!(stored.bytes, source);
    assert_eq!(std::fs::read(&quota).unwrap(), quota_before);
    write_note(
        "generic.txt",
        "begin=Unsupported\nput_part=Unsupported\ncomplete=Unsupported\nabort=Unsupported\nquota_mutated=0\ngeneric_pull=1\nread_object_full_buffer=1\n",
    );
}

#[tokio::test]
async fn review_download_pending_get_is_not_preempted() {
    let root = scratch("pending");
    let publisher = host_on(&root);
    let ctx = admission("project_a", "user_a", "grant-pending");
    let source = b"abcdefghij0123456789".to_vec();
    let manifest = publish(&publisher, &ctx, "pending.bin", &source).await;
    let (store, gate) = CountingStore::open(&root, true);
    let host = FileHost::new(store, HostLimits::standard());

    gate.hold_get.store(true, Ordering::SeqCst);
    gate.hold_bytes.store(true, Ordering::SeqCst);
    let _release = ReleaseBoth {
        get: &gate.release_get,
        bytes: &gate.release_bytes,
    };
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 8, Some(deadline))
        .await
        .unwrap();
    let calls_before = gate.body_calls.load(Ordering::SeqCst);
    let (stopped, finished_before_bytes_release) = {
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
        poll_until(&mut fut, || gate.entered_bytes.load(Ordering::SeqCst));
        assert!(
            !gate.dropped_bytes.load(Ordering::SeqCst),
            "deadline dropped bytes() after get_opts returned"
        );
        let finished_before_bytes_release = gate.body_finished.load(Ordering::SeqCst);
        gate.release_bytes.store(true, Ordering::SeqCst);
        let stopped = poll_ready(&mut fut).unwrap_err();
        (stopped, finished_before_bytes_release)
    };
    assert_eq!(stopped, FileHostError::Stopped);
    assert_eq!(stopped.to_string(), STOPPED);
    assert!(!reader.is_complete());
    assert_snapshot(reader.manifest(), &manifest);
    assert!(finished_before_bytes_release >= 1);
    assert!(gate.body_calls.load(Ordering::SeqCst) > calls_before);
    let calls_after = gate.body_calls.load(Ordering::SeqCst);
    assert_eq!(
        reader.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert_eq!(gate.body_calls.load(Ordering::SeqCst), calls_after);

    gate.hold_get.store(true, Ordering::SeqCst);
    gate.hold_bytes.store(false, Ordering::SeqCst);
    gate.release_get.store(false, Ordering::SeqCst);
    gate.entered_get.store(false, Ordering::SeqCst);
    gate.done_get.store(false, Ordering::SeqCst);
    gate.dropped_get.store(false, Ordering::SeqCst);
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    {
        let mut fut = std::pin::pin!(reader.next_chunk());
        poll_until(&mut fut, || gate.entered_get.load(Ordering::SeqCst));
        assert!(!gate.done_get.load(Ordering::SeqCst));
        drop(fut);
    }
    assert!(gate.dropped_get.load(Ordering::SeqCst));
    assert!(
        !gate.done_get.load(Ordering::SeqCst),
        "dropping next_chunk stopped the blocking wait"
    );
    gate.release_get.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while !gate.done_get.load(Ordering::SeqCst) {
        if start.elapsed() > Duration::from_secs(5) {
            panic!("blocking wait did not finish after release");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!reader.is_complete());
    assert_eq!(
        reader.next_chunk().await.unwrap_err(),
        FileHostError::Stopped
    );
    assert_eq!(reader.next_chunk().await.unwrap_err().to_string(), STOPPED);

    let mut note = String::new();
    note.push_str("deadline_aborts_get_opts=0\n");
    note.push_str("deadline_aborts_bytes=0\n");
    note.push_str("bytes_started_after_deadline_during_get_opts=1\n");
    note.push_str("chunk_accepted_after_deadline=0\n");
    note.push_str("complete_after_deadline=0\n");
    note.push_str("drop_sets_stopped=1\n");
    note.push_str("drop_stops_blocking_wait=0\n");
    note.push_str("preemption_claimed=0\n");
    write_note("pending.txt", &note);
}

fn child_for(test_name: &str) -> bool {
    std::env::var_os("OO_A194_CHILD").is_some()
        && std::env::var("OO_A194_TEST").as_deref() == Ok(test_name)
}

fn proc_kib(field: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            return rest.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("missing {field}");
}

fn metric_str(text: &str, key: &str) -> String {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            return rest.trim().to_owned();
        }
    }
    panic!("missing {key}");
}

fn metric_u64(text: &str, key: &str) -> u64 {
    metric_str(text, key).parse().unwrap()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn review_download_memory_is_bounded() {
    if child_for("review_download_memory_is_bounded") {
        memory_child().await;
        return;
    }
    let mut report = String::new();
    let mut recompute = String::new();
    for size in [8 * 1024 * 1024_usize, 32 * 1024 * 1024] {
        let root = scratch(&format!("mem-{size}"));
        let metrics = root.join("metrics.txt");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("review_download_memory_is_bounded")
            .arg("--exact")
            .arg("--test-threads=1")
            .env("OO_A194_CHILD", "1")
            .env("OO_A194_TEST", "review_download_memory_is_bounded")
            .env("OO_A194_ROOT", &root)
            .env("OO_A194_METRICS", &metrics)
            .env("OO_A194_SIZE", size.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.wait_with_output().unwrap();
        if !output.status.success() {
            panic!(
                "memory child {size} exit {:?}\nstdout {}\nstderr {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let text = std::fs::read_to_string(&metrics).unwrap();
        let object_key = metric_str(&text, "object_key");
        let file_hash = hex(&hash_file(&root.join(&object_key)));
        let pattern = hex(&pattern_digest(size));
        assert_eq!(file_hash, metric_str(&text, "download_sha256"));
        assert_eq!(pattern, metric_str(&text, "pattern_sha256"));
        assert_eq!(file_hash, metric_str(&text, "file_sha256"));
        let delta = metric_u64(&text, "hwm_delta_bytes");
        assert!(delta < 4 * 1024 * 1024, "{text}");
        assert!(delta.saturating_mul(2) < size as u64, "{text}");
        assert_eq!(metric_u64(&text, "chunk_max"), 8192);
        assert_eq!(metric_u64(&text, "range_max"), 8192);
        assert_eq!(metric_u64(&text, "pulled"), size as u64);
        assert_eq!(metric_u64(&text, "range_sum"), size as u64);
        assert_eq!(metric_u64(&text, "range_count"), size.div_ceil(8192) as u64);
        assert_eq!(metric_u64(&text, "body_unbounded"), 0);
        assert_eq!(metric_u64(&text, "complete"), 1);
        assert_eq!(metric_u64(&text, "second_eof_extra_gets"), 0);
        assert_eq!(metric_str(&text, "caller_retained_chunks"), "1");
        recompute.push_str(&format!(
            "size={size} pattern_sha256={pattern} file_sha256={file_hash} download_sha256={}\n",
            metric_str(&text, "download_sha256")
        ));
        report.push_str(&text);
        report.push('\n');
    }
    write_note("memory.txt", &report);
    write_note("pattern-recompute.txt", &recompute);
}

#[cfg(target_os = "linux")]
async fn memory_child() {
    let root = std::path::PathBuf::from(std::env::var("OO_A194_ROOT").unwrap());
    let metrics_path = std::path::PathBuf::from(std::env::var("OO_A194_METRICS").unwrap());
    let size: usize = std::env::var("OO_A194_SIZE").unwrap().parse().unwrap();
    let digest_bytes = pattern_digest(size);
    let limits =
        HostLimits::try_new(32 * 1024 * 1024, 32 * 1024 * 1024, 4, 64 * 1024 * 1024).unwrap();
    let publisher = FileHost::open_local(&root, limits).unwrap();
    let ctx = admission("project_a", "user_a", "grant-memory");
    let config = UploadConfig {
        display_name: "stream.bin".to_owned(),
        expected_sha256: digest_bytes,
        expected_size: u64::try_from(size).unwrap(),
        content_type: None,
    };
    let session = publisher.begin(&ctx, &config).await.unwrap();
    let part_path = root.join(format!(
        "staging/{}/part/00001",
        session.session_id.as_str()
    ));
    std::fs::create_dir_all(part_path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(&part_path).unwrap();
    let chunk = [PATTERN; 8192];
    let mut left = size;
    while left > 0 {
        let n = 8192.min(left);
        std::io::Write::write_all(&mut file, &chunk[..n]).unwrap();
        left -= n;
    }
    drop(file);
    let published = publisher.complete(&ctx, &session.session_id).await.unwrap();
    let quota = std::fs::read(quota_path(
        &root,
        ctx.project_id.as_str(),
        published.session_id.as_str(),
    ))
    .unwrap();
    let (store, gate) = CountingStore::open(&root, false);
    let host = FileHost::new(store, limits);
    let hwm_before = proc_kib("VmHWM:");
    let rss_before = proc_kib("VmRSS:");
    let peak_before = proc_kib("VmPeak:");
    let threads_before = proc_kib("Threads:");
    let mut reader = host
        .open_download(&ctx, &published.object_key, 8192, None)
        .await
        .unwrap();
    assert_snapshot(reader.manifest(), &published);
    let mut hasher = Sha256::new();
    let mut pulled = 0u64;
    let mut chunk_max = 0usize;
    loop {
        assert_eq!(reader.manifest().object_digest, published.object_digest);
        assert_eq!(reader.manifest().size, published.size);
        assert_eq!(reader.manifest().object_key, published.object_key);
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
    let mut download_sha = [0u8; 32];
    download_sha.copy_from_slice(&hasher.finalize());
    assert!(reader.is_complete());
    let calls = gate.body_calls.load(Ordering::SeqCst);
    assert!(reader.next_chunk().await.unwrap().is_none());
    let extra = gate.body_calls.load(Ordering::SeqCst) - calls;
    let file_sha = hash_file(&root.join(published.object_key.as_str()));
    let hwm_after = proc_kib("VmHWM:");
    let rss_after = proc_kib("VmRSS:");
    let peak_after = proc_kib("VmPeak:");
    let threads_after = proc_kib("Threads:");
    let hwm_delta = hwm_after.saturating_sub(hwm_before) * 1024;
    let quota_after = std::fs::read(quota_path(
        &root,
        ctx.project_id.as_str(),
        published.session_id.as_str(),
    ))
    .unwrap();
    assert_eq!(quota_after, quota);
    assert_eq!(download_sha, digest_bytes);
    assert_eq!(file_sha, digest_bytes);
    assert_eq!(pulled, size as u64);
    assert_eq!(gate.body_bytes.load(Ordering::SeqCst), size as u64);
    assert!(gate.body_max.load(Ordering::SeqCst) <= 8192);
    assert_eq!(gate.body_unbounded.load(Ordering::SeqCst), 0);
    assert_eq!(extra, 0);
    let text = format!(
        "bytes={size}\nobject_key={}\ndownload_sha256={}\npattern_sha256={}\nfile_sha256={}\nhwm_before_kib={hwm_before}\nhwm_after_kib={hwm_after}\nhwm_delta_bytes={hwm_delta}\nrss_before_kib={rss_before}\nrss_after_kib={rss_after}\nvmpeak_before_kib={peak_before}\nvmpeak_after_kib={peak_after}\nthreads_before={threads_before}\nthreads_after={threads_after}\nchunk_max={chunk_max}\nrange_max={}\nrange_count={}\nrange_sum={}\npulled={pulled}\nbody_unbounded={}\ncomplete=1\nsecond_eof_extra_gets={extra}\ncaller_retained_chunks=1\nobject_vec=0\nquota_unchanged=1\n",
        published.object_key.as_str(),
        hex(&download_sha),
        hex(&digest_bytes),
        hex(&file_sha),
        gate.body_max.load(Ordering::SeqCst),
        gate.body_calls.load(Ordering::SeqCst),
        gate.body_bytes.load(Ordering::SeqCst),
        gate.body_unbounded.load(Ordering::SeqCst),
    );
    std::fs::write(&metrics_path, &text).unwrap();
}
