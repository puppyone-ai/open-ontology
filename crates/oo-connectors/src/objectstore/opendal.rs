//! `s3` backend of `oo.doc-source/v1` on OpenDAL `services-s3` (10 §4.3.3).
//!
//! S3, R2, RustFS and MinIO all go through here; what a store can do is
//! learned from its answers (`compat.rs`), never from its name.
//!
//! Transport. OpenDAL with only `services-s3` has no HTTP client. Rather than
//! admit `opendal-http-transport-reqwest`, `opendal-layer-retry`, `backon` and
//! friends, this file gives OpenDAL a transport of its own on the `reqwest`
//! 0.13.5 that is already in the lock (feature `opensearch` uses it), and
//! retries per request itself (200 ms doubling, 10 s cap, 4 attempts). The
//! transport buffers one response at a time, which is at most one 8 MiB range
//! of a download, so memory stays flat for any object size.
//!
//! Redaction. Errors map to the closed [`DocSourceError`] set with static
//! text; nothing here formats a key, endpoint, bucket or credential.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use opendal::raw::percent_decode_path;
use opendal::services::S3;
use opendal::{
    Buffer, ErrorKind, HttpBody, HttpTransport, HttpTransporter, OperationContext, Operator,
};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::objectstore::hash::{DualDigest, DualHasher};
use crate::objectstore::path::{ExcludeSet, is_skipped_path, normalize_raw_key};
use crate::objectstore::{
    DocBackend, DocCapability, DocSourceError, DocumentSource, EntryKind, ListCursor, ListPage,
    ObjectMeta, ReadExpect,
};
use crate::objectstore_compat::{
    self as compat, Crc64, Crc64Kind, Crc64Value, EndpointHints, Probed, Provider,
};

/// Bytes per ranged GET (10 §4.3.3).
pub const RANGE_BYTES: u64 = 8 * 1024 * 1024;
/// Whole-object re-downloads after a CRC64 mismatch before giving up.
const CRC_ATTEMPTS: u32 = 3;
const RETRY_ATTEMPTS: u32 = 4;
const RETRY_FIRST: Duration = Duration::from_millis(200);
const RETRY_CAP: Duration = Duration::from_secs(10);
const SEEN_MAX: usize = 256;

/// Keys and values that are secret. Dropped zeroed.
pub struct S3Credentials {
    access_key_id: Zeroizing<String>,
    secret_access_key: Zeroizing<String>,
    session_token: Option<Zeroizing<String>>,
}

impl std::fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3Credentials(..)")
    }
}

impl S3Credentials {
    pub fn new(
        access_key_id: &str,
        secret_access_key: &str,
        session_token: Option<&str>,
    ) -> Result<Self, DocSourceError> {
        if access_key_id.is_empty() || secret_access_key.is_empty() {
            return Err(DocSourceError::InvalidConfig("credential is incomplete"));
        }
        Ok(Self {
            access_key_id: Zeroizing::new(access_key_id.to_owned()),
            secret_access_key: Zeroizing::new(secret_access_key.to_owned()),
            session_token: session_token.map(|t| Zeroizing::new(t.to_owned())),
        })
    }

    /// `{"access_key_id": ..., "secret_access_key": ..., "session_token": ...}`.
    pub fn from_secret_text(text: &str) -> Result<Self, DocSourceError> {
        let v: Value = serde_json::from_str(text.trim())
            .map_err(|_| DocSourceError::InvalidConfig("credential is not valid json"))?;
        let get = |k: &str| v.get(k).and_then(Value::as_str);
        Self::new(
            get("access_key_id").unwrap_or(""),
            get("secret_access_key").unwrap_or(""),
            get("session_token"),
        )
    }
}

/// True for a well formed `secret_ref` (`env:NAME`, `file:/abs` or a name).
pub fn is_secret_ref(reference: &str) -> bool {
    oo_stack::stack::secret_reference(reference)
}

/// Resolves a `secret_ref` to its text: `env:NAME`, `file:/abs/path` or a name
/// in the secrets directory (the daemon copied it there at registration,
/// 0600).
#[derive(Clone, Debug)]
pub struct SecretResolver {
    pub secrets_dir: PathBuf,
}

impl SecretResolver {
    pub fn resolve(&self, reference: &str) -> Result<Zeroizing<String>, DocSourceError> {
        if !oo_stack::stack::secret_reference(reference) {
            return Err(DocSourceError::InvalidConfig("secret_ref is not valid"));
        }
        let missing = || DocSourceError::InvalidConfig("credential is missing");
        if let Some(name) = reference.strip_prefix("env:") {
            return std::env::var(name)
                .map(Zeroizing::new)
                .map_err(|_| missing());
        }
        let path = if let Some(file) = reference.strip_prefix("file:") {
            PathBuf::from(file)
        } else {
            self.secrets_dir.join(reference)
        };
        std::fs::read_to_string(path)
            .map(Zeroizing::new)
            .map_err(|_| missing())
    }
}

/// `drive.config` of an `s3` Drive, minus the secret.
#[derive(Debug)]
pub struct S3Config {
    /// Which cloud the bucket is on; chooses defaults only (`compat::Provider`).
    pub provider: Provider,
    pub bucket: String,
    /// Key prefix of the Drive; no leading `/`, empty or ending in `/`.
    pub prefix: String,
    pub endpoint: Option<String>,
    pub region: String,
    pub path_style: bool,
    pub excludes: Vec<String>,
    pub credentials: S3Credentials,
}

impl S3Config {
    /// Parse `drive.config`: `bucket`, `prefix`, `endpoint`, `region`,
    /// `path_style`, `exclude` and `{secret_ref}` (top level or under
    /// `credentials`). Inline keys are refused (10 §9).
    pub fn from_drive_config(
        config: &Value,
        resolver: &SecretResolver,
    ) -> Result<Self, DocSourceError> {
        Self::from_drive_config_for("s3", config, resolver)
    }

    /// Same for any of the `s3`, `r2`, `oss` and `cos` backends. Extra keys:
    /// `account_id` (r2), `internal` (oss, cos: the in-cloud endpoint). `region`
    /// is required for `oss` and `cos`; `endpoint` overrides the cloud's own
    /// endpoint (a self-hosted store, or a test), and then `path_style`
    /// defaults to true.
    pub fn from_drive_config_for(
        backend: &str,
        config: &Value,
        resolver: &SecretResolver,
    ) -> Result<Self, DocSourceError> {
        let provider = Provider::parse(backend).ok_or(DocSourceError::InvalidConfig(
            "backend is not an s3 backend",
        ))?;
        // A key or secret written into the config as text is refused; `{"secret_ref": ...}` is a reference.
        for inline in [
            "access_key_id",
            "secret_access_key",
            "session_token",
            "secret",
        ] {
            if config.get(inline).is_some_and(|v| !v.is_object()) {
                return Err(DocSourceError::InvalidConfig(
                    "inline credentials are refused",
                ));
            }
        }
        let text = |k: &str| config.get(k).and_then(Value::as_str);
        let bucket = text("bucket")
            .filter(|b| !b.is_empty())
            .ok_or(DocSourceError::InvalidConfig("s3 config has no bucket"))?
            .to_owned();
        let mut prefix = text("prefix")
            .unwrap_or("")
            .trim_start_matches('/')
            .to_owned();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        let custom_endpoint = text("endpoint")
            .filter(|e| !e.is_empty())
            .map(str::to_owned);
        if matches!(provider, Provider::Oss | Provider::Cos)
            && custom_endpoint.is_none()
            && text("region").is_none_or(str::is_empty)
        {
            return Err(DocSourceError::InvalidConfig("config has no region"));
        }
        let region = text("region")
            .filter(|r| !r.is_empty())
            .unwrap_or(provider.default_region())
            .to_owned();
        let endpoint = match &custom_endpoint {
            Some(e) => Some(e.clone()),
            None => provider
                .endpoint(
                    &region,
                    EndpointHints {
                        account_id: text("account_id"),
                        internal: config
                            .get("internal")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    },
                )
                .map_err(DocSourceError::InvalidConfig)?,
        };
        let path_style = config.get("path_style").and_then(Value::as_bool).unwrap_or(
            if custom_endpoint.is_some() {
                true
            } else {
                provider.cloud_path_style()
            },
        );
        let credentials = credentials_of(config, resolver)?;
        let excludes = config
            .get("exclude")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            provider,
            bucket,
            prefix,
            endpoint,
            region,
            path_style,
            excludes,
            credentials,
        })
    }
}

/// The credentials a Drive config names: one `secret_ref` (top level or under `credentials`) that resolves to
/// the JSON `{"access_key_id", "secret_access_key", "session_token"?}`, or the contract's pair
/// `access_key_id` and `secret_access_key` (and optionally `session_token`), each `{"secret_ref": ...}` that
/// resolves to the plain value (docs-api 1.10).
fn credentials_of(
    config: &Value,
    resolver: &SecretResolver,
) -> Result<S3Credentials, DocSourceError> {
    let ref_of = |v: &Value| {
        v.get("secret_ref")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let single = config
        .get("secret_ref")
        .and_then(Value::as_str)
        .or_else(|| {
            config
                .get("credentials")
                .and_then(|c| c.get("secret_ref"))
                .and_then(Value::as_str)
        });
    let pair = config
        .get("access_key_id")
        .zip(config.get("secret_access_key"));
    match (single, pair) {
        (Some(_), Some(_)) => Err(DocSourceError::InvalidConfig(
            "give one secret_ref or the access key pair, not both",
        )),
        (Some(reference), None) => S3Credentials::from_secret_text(&resolver.resolve(reference)?),
        (None, Some((id, secret))) => {
            let id_ref = ref_of(id).ok_or(DocSourceError::InvalidConfig(
                "access_key_id has no secret_ref",
            ))?;
            let secret_ref = ref_of(secret).ok_or(DocSourceError::InvalidConfig(
                "secret_access_key has no secret_ref",
            ))?;
            let token = match config.get("session_token") {
                Some(t) => Some(resolver.resolve(&ref_of(t).ok_or(
                    DocSourceError::InvalidConfig("session_token has no secret_ref"),
                )?)?),
                None => None,
            };
            S3Credentials::new(
                resolver.resolve(&id_ref)?.trim(),
                resolver.resolve(&secret_ref)?.trim(),
                token.as_ref().map(|t| t.trim()),
            )
        }
        (None, None) => Err(DocSourceError::InvalidConfig("s3 config has no secret_ref")),
    }
}

/// What the transport saw in responses that the OpenDAL types drop.
#[derive(Default)]
pub struct Seen {
    crc64: Mutex<HashMap<String, Crc64Value>>,
    retry_after_ms: AtomicU64,
    /// Largest `Date` offset of the store seen so far (signed, ms).
    skew_ms: AtomicI64,
}

impl Seen {
    fn note(&self, path: &str, value: Crc64Value) {
        let mut m = self.crc64.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= SEEN_MAX {
            m.clear();
        }
        m.insert(path.to_owned(), value);
    }

    fn take(&self, candidates: &[String]) -> Option<Crc64Value> {
        let mut m = self.crc64.lock().unwrap_or_else(|e| e.into_inner());
        candidates.iter().find_map(|c| m.remove(c))
    }
}

/// OpenDAL's HTTP transport on `reqwest`.
struct ReqwestTransport {
    client: reqwest::Client,
    seen: Arc<Seen>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn http_error(message: &'static str) -> opendal::Error {
    opendal::Error::new(ErrorKind::Unexpected, message).set_temporary()
}

impl HttpTransport for ReqwestTransport {
    async fn fetch(&self, req: http::Request<Buffer>) -> opendal::Result<http::Response<HttpBody>> {
        let (parts, body) = req.into_parts();
        let head = parts.method == http::Method::HEAD;
        let path = percent_decode_path(parts.uri.path());
        let mut builder = self
            .client
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers);
        if !body.is_empty() {
            builder = builder.body(body.to_vec());
        }
        let resp = builder.send().await.map_err(|e| {
            if e.is_timeout() {
                http_error("request timed out")
            } else {
                http_error("request failed")
            }
        })?;
        let status = resp.status();
        let headers = resp.headers().clone();
        if let Some(c) = compat::crc64_from_headers(&headers) {
            self.seen.note(&path, c);
        }
        if let Some(skew) = headers
            .get(http::header::DATE)
            .and_then(|v| v.to_str().ok())
            .and_then(|d| compat::clock_skew_ms(d, now_ms()))
        {
            let cur = self.seen.skew_ms.load(Ordering::Relaxed);
            if skew.abs() > cur.abs() {
                self.seen.skew_ms.store(skew, Ordering::Relaxed);
            }
        }
        if let Some(ms) = headers
            .get(http::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
        {
            self.seen
                .retry_after_ms
                .store(ms.saturating_mul(1000), Ordering::Relaxed);
        }
        let bytes = if head {
            Default::default()
        } else {
            resp.bytes()
                .await
                .map_err(|_| http_error("response body failed"))?
        };
        let size = (!head).then(|| bytes.len() as u64);
        let stream = futures_util::stream::iter(vec![Ok(Buffer::from(bytes))]);
        let mut out = http::Response::new(HttpBody::new(stream, size));
        *out.status_mut() = status;
        *out.headers_mut() = headers;
        Ok(out)
    }
}

/// Build the OpenDAL operator of a Drive with the transport above. Public so
/// that tests can put fixtures in a bucket; the source itself never writes.
pub fn build_operator(config: &S3Config) -> Result<(Operator, Arc<Seen>), DocSourceError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| DocSourceError::Io("http client could not be built"))?;
    let seen = Arc::new(Seen::default());
    let mut b = S3::default()
        .bucket(&config.bucket)
        .region(&config.region)
        .root(&format!("/{}", config.prefix))
        .access_key_id(&config.credentials.access_key_id)
        .secret_access_key(&config.credentials.secret_access_key)
        .disable_config_load()
        .disable_ec2_metadata();
    if let Some(t) = &config.credentials.session_token {
        b = b.session_token(t);
    }
    if let Some(e) = &config.endpoint {
        b = b.endpoint(e);
    }
    if !config.path_style {
        b = b.enable_virtual_host_style();
    }
    let op = Operator::new(b).map_err(|_| DocSourceError::InvalidConfig("s3 config is invalid"))?;
    let transport = HttpTransporter::new(ReqwestTransport {
        client,
        seen: seen.clone(),
    });
    let op = op.with_context(OperationContext::new().with_http_transport(transport));
    Ok((op, seen))
}

fn map_error(seen: &Seen, e: &opendal::Error) -> DocSourceError {
    let text = e.to_string();
    match e.kind() {
        ErrorKind::NotFound => DocSourceError::NotFound,
        ErrorKind::ConditionNotMatch => DocSourceError::Changed,
        ErrorKind::PermissionDenied => {
            if text.contains("InvalidAccessKeyId") || text.contains("SignatureDoesNotMatch") {
                DocSourceError::AuthFailed
            } else {
                DocSourceError::PermissionDenied
            }
        }
        ErrorKind::RateLimited => {
            let ms = seen.retry_after_ms.load(Ordering::Relaxed);
            DocSourceError::Throttled {
                retry_after_ms: if ms == 0 { 1000 } else { ms },
            }
        }
        ErrorKind::ConfigInvalid => DocSourceError::InvalidConfig("s3 config is invalid"),
        _ if text.contains("timed out") => DocSourceError::Timeout,
        _ if e.is_temporary() => DocSourceError::Unavailable,
        _ => DocSourceError::Io("object store request failed"),
    }
}

async fn retry<T, Fut>(mut f: impl FnMut() -> Fut) -> Result<T, opendal::Error>
where
    Fut: Future<Output = Result<T, opendal::Error>>,
{
    let mut delay = RETRY_FIRST;
    let mut attempt = 1;
    loop {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e)
                if attempt < RETRY_ATTEMPTS
                    && (e.is_temporary() || e.kind() == ErrorKind::RateLimited) =>
            {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(RETRY_CAP);
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// What [`S3Source::stream_object`] hands to its sink.
#[derive(Debug)]
pub enum Streamed {
    /// The next bytes of the object, in order.
    Bytes(Vec<u8>),
    /// Everything sent so far is void: the transfer failed its CRC64 and the
    /// object is being downloaded again from the start.
    Restart,
}

/// A Drive on an S3 compatible bucket as a read-only [`DocumentSource`].
#[derive(Clone)]
pub struct S3Source {
    inner: Arc<Inner>,
}

struct Inner {
    provider: Provider,
    op: Operator,
    seen: Arc<Seen>,
    excludes: ExcludeSet,
    bucket: String,
    prefix: String,
    probed: Mutex<Option<Probed>>,
}

impl S3Source {
    /// First level scopes (10 11.3): every top level "directory" of the Drive's prefix, plus `""` when
    /// objects sit directly under it. `None` when a top level name is odd (empty segment): the Drive is
    /// then listed whole, because an object under a scope nobody lists would look deleted.
    pub async fn list_scopes(&self) -> Result<Option<Vec<String>>, DocSourceError> {
        let i = &self.inner;
        let (dirs, root_files, odd) = retry(|| async {
            let mut lister = i.op.lister_with("/").recursive(false).limit(1000).await?;
            let mut dirs: Vec<String> = Vec::new();
            let mut root_files = false;
            let mut odd = false;
            while let Some(entry) = lister.next().await {
                let (path, meta) = entry?.into_parts();
                let key = path.trim_start_matches('/').to_owned();
                if meta.is_file() {
                    if !self.skipped(&key) {
                        root_files = true;
                    }
                } else if let Some(name) = key.strip_suffix('/') {
                    if name.is_empty() || name.contains('/') {
                        odd = true;
                    } else if !is_skipped_path(name) {
                        dirs.push(name.to_owned());
                    }
                }
            }
            Ok((dirs, root_files, odd))
        })
        .await
        .map_err(|e| map_error(&i.seen, &e))?;
        if odd {
            return Ok(None);
        }
        let mut scopes = dirs;
        scopes.sort();
        scopes.dedup();
        if root_files {
            scopes.insert(0, String::new());
        }
        Ok(Some(scopes))
    }

    /// One page of one first level scope, listed under its own key prefix. `""` is the objects directly
    /// under the Drive's prefix. The cursor is the last key, as in [`DocumentSource::list`].
    pub async fn list_scope(
        &self,
        scope: &str,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        if scope.contains('/') || scope == "." || scope == ".." {
            return Err(DocSourceError::InvalidConfig(
                "scope is not a first level name",
            ));
        }
        let page = page_size.clamp(1, 1000) as usize;
        let after = cursor.map(|c| c.0.clone());
        let path = if scope.is_empty() {
            "/".to_owned()
        } else {
            format!("/{scope}/")
        };
        let recursive = !scope.is_empty();
        let i = &self.inner;
        let (last, keys, done) = retry(|| {
            let after = after.clone();
            let path = path.clone();
            async move {
                let mut b = i.op.lister_with(&path).recursive(recursive).limit(page);
                if let Some(a) = &after {
                    b = b.start_after(a);
                }
                let mut lister = b.await?;
                let mut out: Vec<(String, opendal::Metadata)> = Vec::new();
                let mut last = after;
                let mut seen_count = 0usize;
                let mut done = true;
                while let Some(entry) = lister.next().await {
                    let (p, meta) = entry?.into_parts();
                    let key = p.trim_start_matches('/').to_owned();
                    last = Some(key.clone());
                    seen_count += 1;
                    if meta.is_file() {
                        out.push((key, meta));
                    }
                    if seen_count >= page {
                        done = false;
                        break;
                    }
                }
                Ok((last, out, done))
            }
        })
        .await
        .map_err(|e| map_error(&i.seen, &e))?;
        let entries = keys
            .iter()
            .filter(|(k, _)| !self.skipped(k))
            .map(|(k, m)| self.meta_of(k, m, None))
            .collect();
        Ok(ListPage {
            entries,
            next: if done { None } else { last.map(ListCursor) },
            complete: done,
        })
    }
}

impl S3Source {
    pub fn open(config: &S3Config) -> Result<Self, DocSourceError> {
        let (op, seen) = build_operator(config)?;
        Ok(Self::from_parts(op, seen, config))
    }

    pub fn from_parts(op: Operator, seen: Arc<Seen>, config: &S3Config) -> Self {
        Self {
            inner: Arc::new(Inner {
                provider: config.provider,
                op,
                seen,
                excludes: ExcludeSet::new(config.excludes.clone()),
                bucket: config.bucket.clone(),
                prefix: config.prefix.clone(),
                probed: Mutex::new(None),
            }),
        }
    }

    fn key(raw_key: &[u8]) -> Result<&str, DocSourceError> {
        if !crate::objectstore::path::raw_key_is_safe(raw_key) {
            return Err(DocSourceError::InvalidConfig(
                "object key is not addressable",
            ));
        }
        std::str::from_utf8(raw_key)
            .map_err(|_| DocSourceError::InvalidConfig("object key is not utf-8"))
    }

    fn meta_of(&self, key: &str, m: &opendal::Metadata, crc64: Option<u64>) -> ObjectMeta {
        let path = normalize_raw_key(key.as_bytes());
        ObjectMeta {
            raw_key: key.as_bytes().to_vec(),
            path,
            kind: EntryKind::File,
            size: m.content_length(),
            etag: m.etag().map(str::to_owned),
            last_modified_ns: m.last_modified().map(|t| t.into_inner().as_nanosecond()),
            version_id: m.version().map(str::to_owned),
            crc64,
            fs_ino: None,
            fs_ctime_ns: None,
        }
    }

    fn skipped(&self, key: &str) -> bool {
        if key.ends_with('/') || is_skipped_path(key) {
            return true;
        }
        normalize_raw_key(key.as_bytes()).is_some_and(|p| self.inner.excludes.excludes(p.as_str()))
    }

    fn seen_candidates(&self, key: &str) -> Vec<String> {
        let i = &self.inner;
        vec![
            format!("/{}/{}{}", i.bucket, i.prefix, key),
            format!("/{}{}", i.prefix, key),
        ]
    }

    /// Stream the whole object in 8 MiB ranges, each with `If-Match` (or the
    /// version id), and hand every chunk to `sink`. A 412 anywhere, or an
    /// object that is not the one it was at the start, ends in
    /// [`DocSourceError::Changed`] and the caller starts over (10 §4.3.3). A
    /// CRC64 reported by the store is verified, with up to three downloads.
    /// Memory is one chunk.
    pub async fn stream_object<F>(
        &self,
        raw_key: &[u8],
        mut sink: F,
    ) -> Result<(DualDigest, ObjectMeta), DocSourceError>
    where
        F: FnMut(Streamed) -> Result<(), DocSourceError>,
    {
        let mut mismatches = 0;
        loop {
            let before = self.stat(raw_key).await?.ok_or(DocSourceError::NotFound)?;
            let expect = ReadExpect {
                etag: before.etag.clone(),
                version_id: before.version_id.clone(),
                size: Some(before.size),
            };
            let mut hasher = DualHasher::new();
            let check = before.crc64.is_some();
            let mut crcs = [Crc64::new(Crc64Kind::Nvme), Crc64::new(Crc64Kind::Ecma)];
            let mut offset = 0u64;
            while offset < before.size {
                let n = RANGE_BYTES.min(before.size - offset);
                let chunk = self.read(raw_key, offset, n, &expect).await?;
                if chunk.len() as u64 != n {
                    // Shorter than the size said: the object moved.
                    return Err(DocSourceError::Changed);
                }
                hasher.update(&chunk);
                if check {
                    crcs[0].update(&chunk);
                    crcs[1].update(&chunk);
                }
                sink(Streamed::Bytes(chunk))?;
                offset += n;
            }
            let digest = hasher.finalize();
            let after = self.stat(raw_key).await?.ok_or(DocSourceError::Changed)?;
            if after.size != before.size
                || after.etag != before.etag
                || after.version_id != before.version_id
                || after.last_modified_ns != before.last_modified_ns
            {
                return Err(DocSourceError::Changed);
            }
            if let Some(want) = before.crc64 {
                let ok = crcs.iter().any(|c| c.finalize() == want);
                if !ok {
                    mismatches += 1;
                    if mismatches >= CRC_ATTEMPTS {
                        return Err(DocSourceError::ChecksumMismatch);
                    }
                    sink(Streamed::Restart)?;
                    continue;
                }
            }
            return Ok((digest, before));
        }
    }

    /// Learn what the store does from one existing object (read only).
    pub async fn probe(&self, raw_key: &[u8]) -> Result<Probed, DocSourceError> {
        let meta = self.stat(raw_key).await?.ok_or(DocSourceError::NotFound)?;
        let mut p = Probed {
            strong_etag: meta
                .etag
                .as_deref()
                .is_some_and(compat::etag_is_content_md5),
            versioning: meta.version_id.is_some(),
            crc64: meta
                .crc64
                .map(|_| self.inner.provider.crc64_kind().unwrap_or(Crc64Kind::Nvme)),
            if_match: false,
        };
        if meta.size > 0 {
            let stale = ReadExpect {
                etag: Some("\"00000000000000000000000000000000\"".to_owned()),
                ..ReadExpect::default()
            };
            p.if_match = matches!(
                self.read(raw_key, 0, 1, &stale).await,
                Err(DocSourceError::Changed)
            );
        }
        *self.inner.probed.lock().unwrap_or_else(|e| e.into_inner()) = Some(p.clone());
        Ok(p)
    }

    /// `DOCS_SOURCE_CLOCK_SKEW` (10 §4.3.3): the store's clock differs from ours
    /// by more than five minutes; `Some(offset_ms)` (store minus local). Read
    /// after any request, from the `Date` headers seen so far.
    pub fn clock_skew(&self) -> Option<i64> {
        let ms = self.inner.seen.skew_ms.load(Ordering::Relaxed);
        (ms.abs() > compat::CLOCK_SKEW_LIMIT_MS).then_some(ms)
    }

    /// The operator, for tests that put fixtures into a bucket.
    #[doc(hidden)]
    pub fn operator_for_tests(&self) -> &Operator {
        &self.inner.op
    }
}

impl DocumentSource for S3Source {
    fn backend(&self) -> DocBackend {
        self.inner.provider.backend()
    }

    fn capabilities(&self) -> BTreeSet<DocCapability> {
        let mut caps: BTreeSet<DocCapability> = [
            DocCapability::List,
            DocCapability::Stat,
            DocCapability::RangedRead,
        ]
        .into();
        if let Some(p) = &*self.inner.probed.lock().unwrap_or_else(|e| e.into_inner()) {
            if p.if_match {
                caps.insert(DocCapability::ConditionalRead);
            }
            if p.strong_etag {
                caps.insert(DocCapability::StrongEtag);
            }
            if p.versioning {
                caps.insert(DocCapability::Versioning);
            }
            if p.crc64.is_some() {
                caps.insert(DocCapability::Crc64);
            }
        }
        caps
    }

    async fn list(
        &self,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        let page = page_size.clamp(1, 1000) as usize;
        let after = cursor.map(|c| c.0.clone());
        let i = &self.inner;
        let (last, keys, done) = retry(|| {
            let after = after.clone();
            async move {
                let mut b = i.op.lister_with("/").recursive(true).limit(page);
                if let Some(a) = &after {
                    b = b.start_after(a);
                }
                let mut lister = b.await?;
                let mut out: Vec<(String, opendal::Metadata)> = Vec::new();
                let mut last = after;
                let mut seen_count = 0usize;
                let mut done = true;
                while let Some(entry) = lister.next().await {
                    let (path, meta) = entry?.into_parts();
                    let key = path.trim_start_matches('/').to_owned();
                    last = Some(key.clone());
                    seen_count += 1;
                    if meta.is_file() {
                        out.push((key, meta));
                    }
                    if seen_count >= page {
                        done = false;
                        break;
                    }
                }
                Ok((last, out, done))
            }
        })
        .await
        .map_err(|e| map_error(&i.seen, &e))?;
        let entries = keys
            .iter()
            .filter(|(k, _)| !self.skipped(k))
            .map(|(k, m)| self.meta_of(k, m, None))
            .collect();
        Ok(ListPage {
            entries,
            next: if done { None } else { last.map(ListCursor) },
            complete: done,
        })
    }

    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        let key = Self::key(raw_key)?;
        let i = &self.inner;
        match retry(|| i.op.stat(key)).await {
            Ok(m) if m.is_file() => {
                let crc = i.seen.take(&self.seen_candidates(key)).map(|c| c.value);
                Ok(Some(self.meta_of(key, &m, crc)))
            }
            Ok(_) => Ok(None),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(map_error(&i.seen, &e)),
        }
    }

    async fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        let key = Self::key(raw_key)?;
        if len == 0 {
            return Ok(Vec::new());
        }
        let end = offset.saturating_add(len);
        let i = &self.inner;
        let r = retry(|| {
            let mut f = i.op.read_with(key).range(offset..end);
            if let Some(v) = &expect.version_id {
                f = f.version(v);
            } else if let Some(t) = &expect.etag {
                f = f.if_match(t);
            }
            f.into_future()
        })
        .await;
        match r {
            Ok(b) => Ok(b.to_vec()),
            Err(e) if e.kind() == ErrorKind::RangeNotSatisfied => Ok(Vec::new()),
            Err(e) => Err(map_error(&i.seen, &e)),
        }
    }
}
