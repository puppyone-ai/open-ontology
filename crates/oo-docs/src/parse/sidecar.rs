//! Client of the Docling sidecar `oo.doc-parser/v1` (10 §2.6, §4.4.3, §4.4.4).
//!
//! Blocking `std::net` HTTP/1.1 over plain TCP (the sidecar lives on the
//! internal compose network): call it from `spawn_blocking`. No HTTP crate is
//! needed because the server always answers with `Content-Length` and the
//! client sends `Connection: close`.
//!
//! What lives here: the wire client, the circuit breaker (3 consecutive
//! infrastructure errors open it for 30 s, then one probe request), the health
//! monitor that asks for a restart after 3 failed `/healthz` checks, the
//! PDF window loop (20 pages, halve once on output-limit or OOM) and progress.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use oo_core::docir::DocFormat;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::blob_b3;
use crate::normalize::docling::{self, WindowResponse};
use crate::normalize::paddle;
use crate::normalize::{Normalized, finish};

pub const PROTOCOL: &str = "oo.doc-parser/v1";
/// PDF pages per request (10 §4.4.3).
pub const PDF_WINDOW: u32 = 20;
/// PDF page limit (10 §4.11).
pub const MAX_PDF_PAGES: u32 = 2_000;
/// Longest response the client reads: 64 MiB body plus headroom.
pub const MAX_RESPONSE_BYTES: usize = 80 << 20;
/// Request deadline sent to the sidecar and the longest it accepts.
pub const DEADLINE_MS: u64 = 120_000;
/// PDF pages per OCR request: PP-StructureV3 takes seconds per page on CPU, and a request is limited to
/// [`DEADLINE_MS`].
pub const OCR_PDF_WINDOW: u32 = 4;
/// A scanned PDF has fewer than this share of pages with a text layer.
pub const TEXT_LAYER_MIN_PERCENT: u32 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidecarError {
    /// `429 PARSER_BUSY`: retry later, not an attempt.
    Busy { retry_after_ms: u64 },
    /// `422 PARSER_ENCRYPTED` or a probe that says a password is needed.
    Encrypted,
    /// `422 PARSER_CORRUPT`.
    Corrupt(String),
    /// `422 PARSER_UNSUPPORTED`.
    Unsupported(String),
    /// `422 PARSER_OUTPUT_LIMIT` after the window was already one page.
    OutputLimit,
    /// `504 PARSER_TIMEOUT`: the worker was killed at the deadline.
    Timeout,
    /// `500 PARSER_CRASH`: the worker died; attribute to the blob.
    Crash(String),
    /// `507 PARSER_OOM`.
    Oom,
    /// `404 PARSER_BLOB_MISSING` that survived a re-upload.
    BlobMissing,
    /// `409 PARSER_HASH_MISMATCH`: the upload was corrupted in flight.
    HashMismatch,
    /// `507 PARSER_SCRATCH_FULL`.
    ScratchFull,
    /// `400 PARSER_BAD_INPUT`: a client defect.
    BadInput(String),
    /// Connection refused or reset, timeout without an answer, 5xx without a
    /// typed body. Counts toward the circuit breaker.
    Unavailable(String),
    /// The breaker is open; jobs for sidecar formats stay `queued`.
    CircuitOpen { retry_after_ms: u64 },
    /// The sidecar answered something this client does not understand.
    Protocol(String),
    /// More than [`MAX_PDF_PAGES`] pages (`DOCS_PARSE_LIMIT_PAGES`).
    LimitPages { pages: u32 },
    /// Text layer on fewer than 20% of the pages (`DOCS_PARSE_SCANNED_PDF`).
    ScannedPdf,
    /// The response was valid HTTP but not a document the normalizer accepts.
    Normalize(String),
    /// `PARSER_CODEC_UNAVAILABLE`: H.264, HEVC, AAC or another codec our FFmpeg does not decode, and neither a
    /// host decode helper nor `media.external_ffmpeg` is available (D65). `how_to` says how to configure.
    CodecUnavailable { codecs: Vec<String>, how_to: String },
    /// `PARSER_MEDIA_TOO_LONG`: over 4 GiB or 4 hours (`too_large`).
    MediaTooLong,
    /// `PARSER_HOST_DECODE_MISMATCH`: host decode output belongs to another version of the file.
    HostDecodeMismatch,
    /// `PARSER_ASR_FAILED`.
    AsrFailed,
    /// `PARSER_DIARIZATION_FAILED`.
    DiarizationFailed,
    /// `PARSER_DISK_LOW`: the job waits (`blocked_reason = disk_low`), the file does not change state.
    DiskLow,
    /// The caller cancelled the task.
    Cancelled,
}

impl SidecarError {
    /// `DOCS_PARSE_*` state code of a per-blob failure, `None` for
    /// infrastructure conditions (the job is blocked or retried, the file does
    /// not change state).
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            Self::Encrypted => "DOCS_PARSE_ENCRYPTED",
            Self::Corrupt(_) => "DOCS_PARSE_CORRUPT",
            Self::Unsupported(_) => "DOCS_PARSE_UNSUPPORTED_FORMAT",
            Self::OutputLimit | Self::Oom => "DOCS_PARSE_OOM",
            Self::Timeout => "DOCS_PARSE_TIMEOUT",
            Self::Crash(_) => "DOCS_PARSE_CRASH",
            Self::ScannedPdf => "DOCS_PARSE_SCANNED_PDF",
            Self::LimitPages { .. } => "DOCS_PARSE_LIMIT_PAGES",
            Self::Normalize(_) => "DOCS_PARSE_IR_INVALID",
            Self::CodecUnavailable { .. } => "DOCS_PARSE_CODEC_UNAVAILABLE",
            Self::MediaTooLong => "DOCS_SOURCE_MEDIA_TOO_LONG",
            Self::HostDecodeMismatch => "DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH",
            Self::AsrFailed => "DOCS_PARSE_ASR_FAILED",
            Self::DiarizationFailed => "DOCS_PARSE_DIARIZATION_FAILED",
            _ => return None,
        })
    }

    /// Counts toward the circuit breaker.
    pub fn is_infrastructure(&self) -> bool {
        matches!(self, Self::Unavailable(_) | Self::Protocol(_))
    }

    /// `429` is retried after a short pause without consuming an attempt.
    pub fn consumes_attempt(&self) -> bool {
        !matches!(self, Self::Busy { .. } | Self::CircuitOpen { .. })
    }
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SidecarError {}

// ---- circuit breaker ----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BreakerState {
    Closed { failures: u32 },
    Open { until: Instant },
    HalfOpen { probing: bool },
}

/// Consecutive infrastructure errors open it; after `open_for` one probe
/// request is let through (half open); success closes it, failure reopens.
#[derive(Clone, Debug)]
pub struct Breaker {
    threshold: u32,
    open_for: Duration,
    state: BreakerState,
}

impl Breaker {
    pub fn new(threshold: u32, open_for: Duration) -> Self {
        Self {
            threshold,
            open_for,
            state: BreakerState::Closed { failures: 0 },
        }
    }

    /// The spec's breaker: 3 errors, 30 s.
    pub fn standard() -> Self {
        Self::new(3, Duration::from_secs(30))
    }

    pub fn is_open(&self, now: Instant) -> bool {
        match self.state {
            BreakerState::Closed { .. } => false,
            BreakerState::Open { until } => now < until,
            BreakerState::HalfOpen { probing } => probing,
        }
    }

    /// Ask to send a request. `Err` carries the time to wait.
    pub fn acquire(&mut self, now: Instant) -> Result<(), Duration> {
        match self.state {
            BreakerState::Closed { .. } => Ok(()),
            BreakerState::Open { until } if now < until => Err(until - now),
            BreakerState::Open { .. } => {
                self.state = BreakerState::HalfOpen { probing: true };
                Ok(())
            }
            BreakerState::HalfOpen { probing: false } => {
                self.state = BreakerState::HalfOpen { probing: true };
                Ok(())
            }
            BreakerState::HalfOpen { probing: true } => Err(Duration::from_millis(500)),
        }
    }

    pub fn success(&mut self) {
        self.state = BreakerState::Closed { failures: 0 };
    }

    pub fn infrastructure_failure(&mut self, now: Instant) {
        self.state = match self.state {
            BreakerState::Closed { failures } if failures + 1 < self.threshold => {
                BreakerState::Closed {
                    failures: failures + 1,
                }
            }
            _ => BreakerState::Open {
                until: now + self.open_for,
            },
        };
    }
}

// ---- health monitor --------------------------------------------------------------

/// `GET /healthz` verdicts: 3 consecutive failures (5 s apart, driven by the
/// caller) ask the daemon to `docker restart` the container.
#[derive(Clone, Debug, Default)]
pub struct HealthMonitor {
    failures: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthVerdict {
    Healthy,
    Degraded,
    Restart,
}

impl HealthMonitor {
    pub const FAILURES_BEFORE_RESTART: u32 = 3;

    pub fn record(&mut self, healthy: bool) -> HealthVerdict {
        if healthy {
            self.failures = 0;
            return HealthVerdict::Healthy;
        }
        self.failures += 1;
        if self.failures >= Self::FAILURES_BEFORE_RESTART {
            self.failures = 0;
            HealthVerdict::Restart
        } else {
            HealthVerdict::Degraded
        }
    }
}

// ---- wire -----------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SidecarConfig {
    /// `host:port` of the sidecar (`doc-parser:7481` inside compose).
    pub addr: String,
    pub token: String,
    pub connect_timeout: Duration,
    /// Extra read time on top of the request deadline.
    pub grace: Duration,
    /// This endpoint is an `ocr-parser`: parse requests say `options.ocr = true`.
    pub ocr: bool,
}

impl SidecarConfig {
    pub fn new(addr: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            token: token.into(),
            connect_timeout: Duration::from_secs(5),
            grace: Duration::from_secs(15),
            ocr: false,
        }
    }

    /// The same endpoint settings for an `ocr-parser` sidecar.
    pub fn for_ocr(mut self) -> Self {
        self.ocr = true;
        self
    }

    /// Ask a Docling doc-parser to OCR embedded pictures too (B-S9-04). Only for an image whose
    /// `/v1/info` says `limits.ocr`; an older image answers 422.
    pub fn with_picture_ocr(self) -> Self {
        self.for_ocr()
    }
}

/// Counters of what the client saw, for the doctor and the tests.
#[derive(Debug, Default)]
pub struct SidecarStats {
    pub crashes: AtomicU64,
    pub timeouts: AtomicU64,
    pub ooms: AtomicU64,
    pub busy: AtomicU64,
    pub infrastructure: AtomicU64,
}

impl SidecarStats {
    pub fn snapshot(&self) -> [u64; 5] {
        [
            self.crashes.load(Ordering::Relaxed),
            self.timeouts.load(Ordering::Relaxed),
            self.ooms.load(Ordering::Relaxed),
            self.busy.load(Ordering::Relaxed),
            self.infrastructure.load(Ordering::Relaxed),
        ]
    }
}

pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

/// Progress of one parse: pages are PDF pages; other formats report 0 of 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub pages_done: u32,
    pub page_count: u32,
}

pub struct SidecarClient {
    cfg: SidecarConfig,
    breaker: Mutex<Breaker>,
    pub stats: SidecarStats,
    /// Scanned pdf and standalone images go here (D69, D76); without it they park as unsupported.
    ocr: Option<Box<SidecarClient>>,
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

impl SidecarClient {
    pub fn new(cfg: SidecarConfig) -> Self {
        Self {
            cfg,
            breaker: Mutex::new(Breaker::standard()),
            stats: SidecarStats::default(),
            ocr: None,
        }
    }

    /// Route scanned PDFs and images to `ocr` (an endpoint built with [`SidecarConfig::for_ocr`]).
    pub fn with_ocr(mut self, ocr: SidecarClient) -> Self {
        self.ocr = Some(Box::new(ocr));
        self
    }

    pub fn ocr_client(&self) -> Option<&SidecarClient> {
        self.ocr.as_deref()
    }

    pub fn with_breaker(cfg: SidecarConfig, breaker: Breaker) -> Self {
        Self {
            cfg,
            breaker: Mutex::new(breaker),
            stats: SidecarStats::default(),
            ocr: None,
        }
    }

    pub fn config(&self) -> &SidecarConfig {
        &self.cfg
    }

    pub fn breaker_open(&self) -> bool {
        self.breaker
            .lock()
            .map(|b| b.is_open(Instant::now()))
            .unwrap_or(true)
    }

    fn http(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        content_type: Option<&str>,
        auth: bool,
        read_timeout: Duration,
    ) -> Result<Response, SidecarError> {
        let unavailable = |what: &str, e: std::io::Error| {
            SidecarError::Unavailable(format!("{what}: {}", e.kind()))
        };
        let addr = self
            .cfg
            .addr
            .to_socket_addrs()
            .map_err(|e| unavailable("resolve", e))?
            .next()
            .ok_or_else(|| SidecarError::Unavailable("resolve: no address".into()))?;
        let mut s = TcpStream::connect_timeout(&addr, self.cfg.connect_timeout)
            .map_err(|e| unavailable("connect", e))?;
        s.set_read_timeout(Some(read_timeout))
            .and_then(|()| s.set_write_timeout(Some(read_timeout)))
            .map_err(|e| unavailable("socket", e))?;
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
            self.cfg.addr,
            body.len()
        );
        if auth {
            head.push_str(&format!("Authorization: Bearer {}\r\n", self.cfg.token));
        }
        if let Some(ct) = content_type {
            head.push_str(&format!("Content-Type: {ct}\r\n"));
        }
        head.push_str("\r\n");
        // A server that answers early (401, 400) may close before the body is
        // sent; keep reading the response in that case.
        let write = s
            .write_all(head.as_bytes())
            .and_then(|()| s.write_all(body));
        let mut raw = Vec::new();
        let read = s
            .take(MAX_RESPONSE_BYTES as u64 + 64 * 1024)
            .read_to_end(&mut raw);
        if raw.is_empty() {
            let e = read.err().or(write.err());
            return Err(SidecarError::Unavailable(format!(
                "no response: {}",
                e.map_or("closed".to_owned(), |e| e.kind().to_string())
            )));
        }
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| SidecarError::Protocol("response head is cut".into()))?;
        let head_text = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status: u16 = head_text
            .lines()
            .next()
            .and_then(|l| l.split(' ').nth(1))
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| SidecarError::Protocol("bad status line".into()))?;
        let declared: Option<usize> = head_text.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())
                .flatten()
        });
        let body = raw[split + 4..].to_vec();
        match declared {
            Some(n) if n == body.len() => {}
            Some(_) => {
                return Err(SidecarError::Unavailable("response body is cut".into()));
            }
            None if status == 204 => {}
            None => return Err(SidecarError::Protocol("no Content-Length".into())),
        }
        Ok(Response { status, body })
    }

    /// One call through the breaker, with typed error mapping.
    pub(crate) fn call(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        json_body: bool,
        auth: bool,
        read_timeout: Duration,
    ) -> Result<Response, SidecarError> {
        {
            let mut b = self.breaker.lock().map_err(|_| poisoned())?;
            if let Err(wait) = b.acquire(Instant::now()) {
                return Err(SidecarError::CircuitOpen {
                    retry_after_ms: wait.as_millis() as u64,
                });
            }
        }
        let ct = json_body.then_some("application/json");
        let result = self.http(method, path, body, ct, auth, read_timeout);
        let mut b = self.breaker.lock().map_err(|_| poisoned())?;
        match result {
            Ok(resp) if resp.status >= 500 && typed_code(&resp.body).is_none() => {
                self.stats.infrastructure.fetch_add(1, Ordering::Relaxed);
                b.infrastructure_failure(Instant::now());
                Err(SidecarError::Unavailable(format!("status {}", resp.status)))
            }
            Ok(resp) => {
                // The sidecar is alive, whatever it said about this blob.
                b.success();
                Ok(resp)
            }
            Err(e) => {
                if e.is_infrastructure() {
                    self.stats.infrastructure.fetch_add(1, Ordering::Relaxed);
                    b.infrastructure_failure(Instant::now());
                }
                Err(e)
            }
        }
    }

    // ---- endpoints ----

    pub fn healthy(&self) -> bool {
        self.http("GET", "/healthz", b"", None, false, Duration::from_secs(5))
            .is_ok_and(|r| r.status == 200)
    }

    pub fn info(&self) -> Result<Value, SidecarError> {
        let r = self.call("GET", "/v1/info", b"", false, true, Duration::from_secs(10))?;
        let v: Value = decode(&r, 200)?;
        if v.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
            return Err(SidecarError::Protocol("unexpected protocol".into()));
        }
        Ok(v)
    }

    /// `PUT /v1/blobs/{sha256}`: true when the sidecar already had it.
    pub fn put_blob(&self, sha: &str, bytes: &[u8]) -> Result<bool, SidecarError> {
        let r = self.call(
            "PUT",
            &format!("/v1/blobs/{sha}"),
            bytes,
            false,
            true,
            self.cfg.grace + Duration::from_secs(60),
        )?;
        match r.status {
            201 => Ok(false),
            200 => Ok(true),
            _ => Err(map_error(&r)),
        }
    }

    pub fn delete_blob(&self, sha: &str) {
        let _ = self.call(
            "DELETE",
            &format!("/v1/blobs/{sha}"),
            b"",
            false,
            true,
            Duration::from_secs(10),
        );
    }

    pub fn probe(&self, sha: &str, format: &str) -> Result<Value, SidecarError> {
        let body = json!({"blob": sha, "format": format}).to_string();
        let r = self.call(
            "POST",
            "/v1/probe",
            body.as_bytes(),
            true,
            true,
            Duration::from_secs(40),
        )?;
        decode(&r, 200).map_err(|_| map_error(&r))
    }

    /// `POST /v1/parse`; the raw response JSON on success.
    pub fn parse_raw(
        &self,
        sha: &str,
        format: &str,
        page_range: Option<(u32, u32)>,
        deadline_ms: u64,
    ) -> Result<Value, SidecarError> {
        let mut req = json!({
            "blob": sha, "format": format,
            "options": {"ocr": self.cfg.ocr, "table_structure": true},
            "deadline_ms": deadline_ms.min(DEADLINE_MS),
        });
        if let Some((s, e)) = page_range {
            req["page_range"] = json!([s, e]);
        }
        let r = self.call(
            "POST",
            "/v1/parse",
            req.to_string().as_bytes(),
            true,
            true,
            Duration::from_millis(deadline_ms) + self.cfg.grace,
        )?;
        if r.status == 200 {
            let v: Value = serde_json::from_slice(&r.body)
                .map_err(|_| SidecarError::Protocol("parse response is not JSON".into()))?;
            if v.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
                return Err(SidecarError::Protocol("unexpected protocol".into()));
            }
            return Ok(v);
        }
        let err = map_error(&r);
        match &err {
            SidecarError::Crash(_) => self.stats.crashes.fetch_add(1, Ordering::Relaxed),
            SidecarError::Timeout => self.stats.timeouts.fetch_add(1, Ordering::Relaxed),
            SidecarError::Oom => self.stats.ooms.fetch_add(1, Ordering::Relaxed),
            SidecarError::Busy { .. } => self.stats.busy.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
        Err(err)
    }

    // ---- whole documents ----

    /// Parse a pdf, docx or pptx to canonical IR. The blob is uploaded once,
    /// PDFs are probed, then parsed in windows of [`PDF_WINDOW`] pages; a
    /// window that overflows the output limit or the memory limit is halved
    /// once. `progress` is called after every window. The scratch copy is
    /// deleted at the end, also on failure.
    pub fn parse_bytes(
        &self,
        bytes: &[u8],
        format: DocFormat,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Normalized, SidecarError> {
        let name = match format {
            DocFormat::Pdf => "pdf",
            DocFormat::Docx => "docx",
            DocFormat::Pptx => "pptx",
            DocFormat::PdfScanned | DocFormat::Image => {
                let ocr = self.ocr.as_deref().ok_or_else(|| {
                    SidecarError::Unsupported("no ocr-parser is configured".into())
                })?;
                return ocr.parse_ocr(bytes, format, None, progress);
            }
            _ => return Err(SidecarError::Unsupported("not a sidecar format".into())),
        };
        let sha = sha256_hex(bytes);
        self.put_blob(&sha, bytes)?;
        let result = self.parse_uploaded(&sha, bytes, format, name, progress);
        self.delete_blob(&sha);
        result
    }

    fn parse_uploaded(
        &self,
        sha: &str,
        bytes: &[u8],
        format: DocFormat,
        name: &str,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Normalized, SidecarError> {
        let mut responses: Vec<WindowResponse> = Vec::new();
        if format == DocFormat::Pdf {
            let probe = self.probe(sha, "pdf")?;
            let n = |k: &str| probe.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
            let flag = |k: &str| probe.get(k).and_then(Value::as_bool).unwrap_or(false);
            if flag("needs_password") || flag("encrypted") {
                return Err(SidecarError::Encrypted);
            }
            let pages = n("page_count");
            if pages == 0 {
                return Err(SidecarError::Corrupt("pdf has no pages".into()));
            }
            if pages > MAX_PDF_PAGES {
                return Err(SidecarError::LimitPages { pages });
            }
            if n("text_layer_pages") * 100 < pages * TEXT_LAYER_MIN_PERCENT {
                // The Docling pipeline has no OCR; a scanned pdf goes to the ocr-parser when there is one.
                return match self.ocr.as_deref() {
                    Some(ocr) => ocr.parse_ocr(bytes, DocFormat::PdfScanned, Some(pages), progress),
                    None => Err(SidecarError::ScannedPdf),
                };
            }
            let mut start = 1;
            while start <= pages {
                let end = (start + PDF_WINDOW - 1).min(pages);
                self.window(sha, name, (start, end), &mut responses, true)?;
                progress(Progress {
                    pages_done: end,
                    page_count: pages,
                });
                start = end + 1;
            }
        } else {
            let v = self.parse_raw(sha, name, None, DEADLINE_MS)?;
            responses.push(WindowResponse::from_value(v));
            progress(Progress {
                pages_done: 0,
                page_count: 0,
            });
        }
        let fragment = docling::assemble(format, blob_b3(bytes), responses)
            .map_err(|e| SidecarError::Normalize(e.to_string()))?;
        finish(fragment).map_err(|e| SidecarError::Normalize(format!("{e:?}")))
    }

    /// OCR path (this client is an ocr-parser): a standalone image is one request, a scanned pdf goes in
    /// windows of [`OCR_PDF_WINDOW`] pages. `pages` is the page count when the caller already knows it.
    pub fn parse_ocr(
        &self,
        bytes: &[u8],
        format: DocFormat,
        pages: Option<u32>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Normalized, SidecarError> {
        let sha = sha256_hex(bytes);
        self.put_blob(&sha, bytes)?;
        let result = self.parse_ocr_uploaded(&sha, bytes, format, pages, progress);
        self.delete_blob(&sha);
        result
    }

    fn parse_ocr_uploaded(
        &self,
        sha: &str,
        bytes: &[u8],
        format: DocFormat,
        pages: Option<u32>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Normalized, SidecarError> {
        let mut responses: Vec<Value> = Vec::new();
        if format == DocFormat::Image {
            responses.push(self.parse_raw(sha, "image", None, DEADLINE_MS)?);
            progress(Progress {
                pages_done: 0,
                page_count: 0,
            });
        } else {
            let pages =
                pages.ok_or_else(|| SidecarError::Unsupported("page count unknown".into()))?;
            if pages > MAX_PDF_PAGES {
                return Err(SidecarError::LimitPages { pages });
            }
            let mut start = 1;
            while start <= pages {
                let end = (start + OCR_PDF_WINDOW - 1).min(pages);
                self.ocr_window(sha, (start, end), &mut responses, true)?;
                progress(Progress {
                    pages_done: end,
                    page_count: pages,
                });
                start = end + 1;
            }
        }
        let fragment = paddle::assemble(format, blob_b3(bytes), responses)
            .map_err(|e| SidecarError::Normalize(e.to_string()))?;
        finish(fragment).map_err(|e| SidecarError::Normalize(format!("{e:?}")))
    }

    fn ocr_window(
        &self,
        sha: &str,
        (s, e): (u32, u32),
        out: &mut Vec<Value>,
        may_split: bool,
    ) -> Result<(), SidecarError> {
        match self.parse_raw(sha, "pdf", Some((s, e)), DEADLINE_MS) {
            Ok(v) => {
                out.push(v);
                Ok(())
            }
            Err(SidecarError::OutputLimit | SidecarError::Oom) if may_split && e > s => {
                let mid = s + (e - s) / 2;
                self.ocr_window(sha, (s, mid), out, false)?;
                self.ocr_window(sha, (mid + 1, e), out, false)
            }
            Err(e) => Err(e),
        }
    }

    /// One window; on output-limit or OOM split it in two and retry once.
    fn window(
        &self,
        sha: &str,
        name: &str,
        (s, e): (u32, u32),
        out: &mut Vec<WindowResponse>,
        may_split: bool,
    ) -> Result<(), SidecarError> {
        match self.parse_raw(sha, name, Some((s, e)), DEADLINE_MS) {
            Ok(v) => {
                out.push(WindowResponse::from_value(v));
                Ok(())
            }
            Err(SidecarError::OutputLimit | SidecarError::Oom) if may_split && e > s => {
                let mid = s + (e - s) / 2;
                self.window(sha, name, (s, mid), out, false)?;
                self.window(sha, name, (mid + 1, e), out, false)
            }
            Err(e) => Err(e),
        }
    }
}

fn poisoned() -> SidecarError {
    SidecarError::Unavailable("breaker lock poisoned".into())
}

/// `{"error":{"code":..}}` of a typed response.
pub(crate) fn typed_code(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    v.get("error")?.get("code")?.as_str().map(str::to_owned)
}

fn decode(r: &Response, want: u16) -> Result<Value, SidecarError> {
    if r.status != want {
        return Err(map_error(r));
    }
    serde_json::from_slice(&r.body).map_err(|_| SidecarError::Protocol("body is not JSON".into()))
}

/// Typed sidecar failure from a non-success response.
pub(crate) fn map_error(r: &Response) -> SidecarError {
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    let err = v.get("error");
    let code = err
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let msg = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    match (r.status, code) {
        (429, _) => SidecarError::Busy {
            retry_after_ms: err
                .and_then(|e| e.get("retry_after_ms"))
                .and_then(Value::as_u64)
                .unwrap_or(2000),
        },
        (_, "PARSER_ENCRYPTED") => SidecarError::Encrypted,
        (_, "PARSER_CORRUPT") => SidecarError::Corrupt(msg),
        (_, "PARSER_UNSUPPORTED") => SidecarError::Unsupported(msg),
        (_, "PARSER_OUTPUT_LIMIT") => SidecarError::OutputLimit,
        (_, "PARSER_TIMEOUT") => SidecarError::Timeout,
        (_, "PARSER_CRASH") => SidecarError::Crash(msg),
        (_, "PARSER_OOM") => SidecarError::Oom,
        (_, "PARSER_BLOB_MISSING") => SidecarError::BlobMissing,
        (_, "PARSER_HASH_MISMATCH") => SidecarError::HashMismatch,
        (_, "PARSER_SCRATCH_FULL") => SidecarError::ScratchFull,
        (400, _) => SidecarError::BadInput(msg),
        (401, _) => SidecarError::Protocol("sidecar rejected the token".into()),
        (s, _) => SidecarError::Unavailable(format!("status {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> Instant {
        // A fixed base so tests are independent of the wall clock.
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_secs(secs)
    }

    #[test]
    fn breaker_opens_after_three_infrastructure_errors() {
        let mut b = Breaker::standard();
        assert!(b.acquire(t(0)).is_ok());
        b.infrastructure_failure(t(0));
        b.infrastructure_failure(t(1));
        assert!(!b.is_open(t(1)));
        b.success(); // any success resets the streak
        b.infrastructure_failure(t(2));
        b.infrastructure_failure(t(3));
        assert!(b.acquire(t(3)).is_ok());
        b.infrastructure_failure(t(4));
        assert!(b.is_open(t(5)));
        let wait = b.acquire(t(10)).unwrap_err();
        assert!(wait <= Duration::from_secs(30) && wait > Duration::from_secs(20));
    }

    #[test]
    fn breaker_half_open_lets_one_probe_through() {
        let mut b = Breaker::standard();
        for i in 0..3 {
            b.infrastructure_failure(t(i));
        }
        assert!(b.acquire(t(31)).is_err()); // open until t(2)+30
        assert!(b.acquire(t(33)).is_ok()); // half open: the probe
        assert!(b.acquire(t(33)).is_err()); // everyone else waits
        b.infrastructure_failure(t(33)); // probe failed: open again
        assert!(b.acquire(t(40)).is_err());
        assert!(b.acquire(t(64)).is_ok());
        b.success();
        assert!(b.acquire(t(64)).is_ok() && !b.is_open(t(64)));
    }

    #[test]
    fn health_monitor_asks_for_a_restart_after_three_failures() {
        let mut h = HealthMonitor::default();
        assert_eq!(h.record(false), HealthVerdict::Degraded);
        assert_eq!(h.record(false), HealthVerdict::Degraded);
        assert_eq!(h.record(true), HealthVerdict::Healthy);
        assert_eq!(h.record(false), HealthVerdict::Degraded);
        assert_eq!(h.record(false), HealthVerdict::Degraded);
        assert_eq!(h.record(false), HealthVerdict::Restart);
        assert_eq!(h.record(false), HealthVerdict::Degraded);
    }

    #[test]
    fn typed_errors_map_to_state_codes() {
        let r = |status, code: &str| Response {
            status,
            body: json!({"error": {"code": code, "message": "m"}})
                .to_string()
                .into_bytes(),
        };
        assert_eq!(
            map_error(&r(422, "PARSER_ENCRYPTED")).code(),
            Some("DOCS_PARSE_ENCRYPTED")
        );
        assert_eq!(
            map_error(&r(422, "PARSER_CORRUPT")).code(),
            Some("DOCS_PARSE_CORRUPT")
        );
        assert_eq!(
            map_error(&r(504, "PARSER_TIMEOUT")).code(),
            Some("DOCS_PARSE_TIMEOUT")
        );
        assert_eq!(
            map_error(&r(500, "PARSER_CRASH")).code(),
            Some("DOCS_PARSE_CRASH")
        );
        assert_eq!(
            map_error(&r(507, "PARSER_OOM")).code(),
            Some("DOCS_PARSE_OOM")
        );
        let busy = map_error(&r(429, "PARSER_BUSY"));
        assert_eq!(
            busy,
            SidecarError::Busy {
                retry_after_ms: 2000
            }
        );
        assert!(!busy.consumes_attempt() && busy.code().is_none());
        assert!(SidecarError::Unavailable("x".into()).is_infrastructure());
        assert!(!SidecarError::Crash("x".into()).is_infrastructure());
        assert!(map_error(&r(502, "")).is_infrastructure());
    }

    #[test]
    fn a_dead_port_is_infrastructure_and_opens_the_breaker() {
        // Port 9 on localhost refuses connections.
        let mut cfg = SidecarConfig::new("127.0.0.1:9", "t");
        cfg.connect_timeout = Duration::from_millis(300);
        let c = SidecarClient::new(cfg);
        for _ in 0..3 {
            assert!(matches!(c.info(), Err(SidecarError::Unavailable(_))));
        }
        assert!(c.breaker_open());
        assert!(matches!(c.info(), Err(SidecarError::CircuitOpen { .. })));
        assert_eq!(c.stats.snapshot()[4], 3);
    }

    /// A tiny sidecar: every request is answered by `answer(method, path, body)`.
    fn mock(
        answer: impl Fn(&str, &str, &[u8]) -> (u16, String) + Send + 'static,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::BufRead;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { break };
                let mut reader = std::io::BufReader::new(conn);
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0u8; len];
                reader.read_exact(&mut body).unwrap();
                let mut parts = first.split_whitespace();
                let (method, path) = (parts.next().unwrap(), parts.next().unwrap());
                log.lock().unwrap().push(format!("{method} {path}"));
                let (status, out) = answer(method, path, &body);
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    out.len()
                );
                let mut conn = reader.into_inner();
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(out.as_bytes());
            }
        });
        (addr, seen)
    }

    fn ocr_body(page_from: u64, page_to: u64) -> String {
        let pages: Vec<Value> = (page_from..=page_to)
            .map(|p| {
                json!({"page": p, "px_w": 1654, "px_h": 2339, "pt_w": 595.0, "pt_h": 842.0,
                       "blocks": [{"label": "text", "kind": "paragraph", "text": format!("第{p}页 陈子墨"),
                                   "bbox_px": [10, 20, 300, 60], "score": 0.97}]})
            })
            .collect();
        json!({"protocol": PROTOCOL, "parser": {"id": "paddleocr", "version": "3.7.0", "adapter_version": "1"},
               "kind": "pdf", "page_count": 6, "pages": [page_from, page_to], "page_list": pages,
               "profile": {"tier": "cpu", "isa": "x", "models": []}, "warnings": []})
        .to_string()
    }

    #[test]
    fn a_scanned_pdf_goes_to_the_ocr_parser_in_windows() {
        let (doc_addr, doc_seen) = mock(|m, p, _| {
            match (m, p) {
            ("PUT", _) => (201, "{}".into()),
            ("DELETE", _) => (204, String::new()),
            ("POST", "/v1/probe") => (
                200,
                json!({"page_count": 6, "encrypted": false, "needs_password": false, "text_layer_pages": 0})
                    .to_string(),
            ),
            _ => (500, "{}".into()),
        }
        });
        let (ocr_addr, ocr_seen) = mock(|m, p, body| match (m, p) {
            ("PUT", _) => (201, "{}".into()),
            ("DELETE", _) => (204, String::new()),
            ("POST", "/v1/parse") => {
                let req: Value = serde_json::from_slice(body).unwrap();
                assert_eq!(req["options"]["ocr"], json!(true));
                let r = req["page_range"].as_array().unwrap();
                (
                    200,
                    ocr_body(r[0].as_u64().unwrap(), r[1].as_u64().unwrap()),
                )
            }
            _ => (500, "{}".into()),
        });
        let client = SidecarClient::new(SidecarConfig::new(doc_addr, "t")).with_ocr(
            SidecarClient::new(SidecarConfig::new(ocr_addr, "t").for_ocr()),
        );
        let mut progress = Vec::new();
        let n = client
            .parse_bytes(b"%PDF-1.7 fake", DocFormat::Pdf, &mut |p| progress.push(p))
            .unwrap();
        assert_eq!(n.ir.format, DocFormat::PdfScanned);
        assert_eq!(n.ir.blocks.len(), 6);
        assert_eq!(n.ir.blocks[5].anchor.fragment(), "#p6");
        assert_eq!(
            progress,
            vec![
                Progress {
                    pages_done: 4,
                    page_count: 6
                },
                Progress {
                    pages_done: 6,
                    page_count: 6
                }
            ]
        );
        assert!(
            doc_seen
                .lock()
                .unwrap()
                .iter()
                .all(|l| !l.contains("/v1/parse"))
        );
        let ocr = ocr_seen.lock().unwrap();
        assert_eq!(
            ocr.iter()
                .filter(|l| l.as_str() == "POST /v1/parse")
                .count(),
            2
        );
        assert!(
            ocr.iter().any(|l| l.starts_with("DELETE ")),
            "scratch copy is deleted"
        );
    }

    #[test]
    fn an_image_needs_the_ocr_parser_and_a_scanned_pdf_without_one_stays_unsupported() {
        let (addr, _) = mock(|m, p, _| {
            match (m, p) {
            ("PUT", _) => (201, "{}".into()),
            ("DELETE", _) => (204, String::new()),
            ("POST", "/v1/probe") => (
                200,
                json!({"page_count": 2, "encrypted": false, "needs_password": false, "text_layer_pages": 0})
                    .to_string(),
            ),
            _ => (500, "{}".into()),
        }
        });
        let plain = SidecarClient::new(SidecarConfig::new(addr, "t"));
        assert_eq!(
            plain
                .parse_bytes(b"%PDF-", DocFormat::Pdf, &mut |_| {})
                .unwrap_err(),
            SidecarError::ScannedPdf
        );
        assert!(matches!(
            plain.parse_bytes(b"\x89PNG", DocFormat::Image, &mut |_| {}),
            Err(SidecarError::Unsupported(_))
        ));
        let (ocr_addr, _) = mock(|m, p, body| match (m, p) {
            ("PUT", _) => (201, "{}".into()),
            ("DELETE", _) => (204, String::new()),
            ("POST", "/v1/parse") => {
                let req: Value = serde_json::from_slice(body).unwrap();
                assert_eq!(req["format"], "image");
                assert!(req.get("page_range").is_none());
                let mut v: Value = serde_json::from_str(&ocr_body(1, 1)).unwrap();
                v["kind"] = json!("image");
                v["page_list"][0]["pt_w"] = Value::Null;
                v["page_list"][0]["pt_h"] = Value::Null;
                (200, v.to_string())
            }
            _ => (500, "{}".into()),
        });
        let direct = SidecarClient::new(SidecarConfig::new(ocr_addr, "t").for_ocr());
        let n = direct.parse_bytes(b"\x89PNG", DocFormat::Image, &mut |_| {});
        // The direct client has no ocr of its own, so an image is refused there too.
        assert!(matches!(n, Err(SidecarError::Unsupported(_))));
        let routed = SidecarClient::new(SidecarConfig::new("127.0.0.1:9", "t")).with_ocr(direct);
        let n = routed
            .parse_bytes(b"\x89PNG", DocFormat::Image, &mut |_| {})
            .unwrap();
        assert_eq!(n.ir.format, DocFormat::Image);
        assert_eq!(n.ir.blocks[0].kind, oo_core::docir::BlockKind::ImageText);
    }
}
