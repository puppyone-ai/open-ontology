//! Pre-edit reproduction of the a194 download defects against unmodified e336 behavior.
//!
//! This file is evidence. It is copied into an archive only after the focused smoke.
//! It expects the defects: a consumed-prefix flip still completes, and `bytes` starts
//! after a deadline that expired during a held `get_opts`.

#![forbid(unsafe_code)]

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use futures_util::stream::{self, BoxStream};
use futures_util::StreamExt;
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

#[tokio::test]
async fn repro_consumed_prefix_flip_still_completes() {
    let root = scratch("prefix");
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("project_a", "user_a", "grant-repro");
    let source = b"abcdefghij0123456789".to_vec();
    let manifest = publish(&host, &ctx, "consumed.bin", &source).await;
    let path = root.join(manifest.object_key.as_str());
    let mut reader = host
        .open_download(&ctx, &manifest.object_key, 8, None)
        .await
        .unwrap();
    let first = reader.next_chunk().await.unwrap().unwrap();
    assert_eq!(first.as_slice(), &source[..8]);
    assert!(!reader.is_complete());
    let mut edited = std::fs::read(&path).unwrap();
    edited[0] ^= 0xff;
    std::fs::write(&path, &edited).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&first);
    let mut offset = first.len();
    let end = loop {
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
    let pulled = finalize(hasher);
    let stored = hash_file(&path);
    let complete = reader.is_complete();
    let note = format!(
        "consumed_prefix_flip_complete={}\nconsumed_prefix_end={}\nconsumed_prefix_running_hash_matches_snapshot={}\nconsumed_prefix_stored_file_matches_snapshot={}\nconsumed_prefix_offset={offset}\nconsumed_prefix_source_len={}\n",
        u8::from(complete),
        match &end {
            None => "None".to_owned(),
            Some(error) => format!("{error:?}"),
        },
        u8::from(pulled == manifest.object_digest),
        u8::from(stored == manifest.object_digest),
        source.len()
    );
    write_note("consumed-prefix.txt", &note);
    println!("{note}");
    assert_eq!(offset, source.len());
    assert_eq!(pulled, manifest.object_digest);
    assert_ne!(stored, manifest.object_digest);
    assert!(end.is_none(), "expected Ok(None), got {end:?}");
    assert!(complete, "a194 consumed-prefix flip was expected to still complete");
}

#[tokio::test]
async fn repro_held_get_opts_starts_bytes_after_deadline() {
    let root = scratch("held");
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
    let (stopped, bytes_started) = {
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
        gate.release_bytes.store(true, Ordering::SeqCst);
        let stopped = poll_ready(&mut fut).unwrap_err();
        (stopped, gate.entered_bytes.load(Ordering::SeqCst))
    };
    let note = format!(
        "bytes_started_after_deadline_during_get_opts={}\ndeadline_dropped_get_opts={}\nstopped={stopped:?}\nstopped_text={}\ncomplete={}\n",
        u8::from(bytes_started),
        u8::from(gate.dropped_get.load(Ordering::SeqCst)),
        stopped,
        u8::from(reader.is_complete())
    );
    write_note("held-get.txt", &note);
    println!("{note}");
    assert_eq!(stopped, FileHostError::Stopped);
    assert!(!reader.is_complete());
    assert!(
        bytes_started,
        "a194 held get_opts was expected to start bytes() after the deadline"
    );
}

fn write_note(name: &str, text: &str) {
    let dir = std::env::var("OO_A202_REPRO_DIR").unwrap();
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oo-a202-repro-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
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
    let config = UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: digest(bytes),
        expected_size: u64::try_from(bytes.len()).unwrap(),
        content_type: None,
    };
    let session = host.begin(ctx, &config).await.unwrap();
    host.put_part(ctx, &session.session_id, 1, bytes)
        .await
        .unwrap();
    host.complete(ctx, &session.session_id).await.unwrap()
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let hash = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn finalize(hasher: Sha256) -> [u8; 32] {
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

fn hash_file(path: &std::path::Path) -> [u8; 32] {
    let bytes = std::fs::read(path).unwrap();
    digest(&bytes)
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
    waker_get: Mutex<Option<Waker>>,
    waker_bytes: Mutex<Option<Waker>>,
}

impl Gate {
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
    gate: Arc<Gate>,
    completed: bool,
    started: bool,
}

impl Pause {
    fn flag(
        gate: &Gate,
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
                .name("oo-a202-repro-hold".to_owned())
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
    gate: Arc<Gate>,
}

impl CountingStore {
    fn open(root: &std::path::Path) -> (Arc<Self>, Arc<Gate>) {
        let local = object_store::local::LocalFileSystem::new_with_prefix(root).unwrap();
        let gate = Gate::new();
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
