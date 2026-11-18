//! OSS, COS and R2 as `objectstore-s3` Drives (10 §4.3.7, B-S11-01): the S3 compatible path with each cloud's
//! defaults, and what a local store answers when it is made to answer the way the cloud documents.
//!
//! No real bucket was reachable when this was written. What is proved here:
//!
//! * the config of each provider resolves to the right endpoint, addressing style and region, and refuses
//!   what could change the host (`oss`, `cos`, `r2` unit tests in `compat.rs` and the config tests below);
//! * a store that sends `x-oss-hash-crc64ecma` or `x-cos-hash-crc64ecma` (ECMA, decimal) gets the download
//!   verified, a corrupt transfer is fetched again and three in a row end in `ChecksumMismatch`;
//! * an R2 like store (no CRC64, no versions, multipart ETag) is read and its capabilities say so;
//! * a store whose `Date` is ten minutes off is reported as clock skew;
//! * `cloud_mimic_minio` (ignored; needs `OO_S3_*` pointing at a MinIO or RustFS) puts a header injecting
//!   proxy in front of the store, so the same checks run against a real S3 implementation.
//!
//! The real bucket test is `cloud_live.rs` (ignored, credentials only through `secret_ref`).
#![cfg(feature = "objectstore-s3")]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oo_connectors::objectstore::{DocBackend, DocCapability, DocSourceError, DocumentSource};
use oo_connectors::objectstore_s3::compat::{Crc64Kind, crc64};
use oo_connectors::objectstore_s3::opendal::{
    S3Config, S3Credentials, S3Source, SecretResolver, Streamed,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What the look-alike cloud answers.
#[derive(Clone)]
struct Cloud {
    body: Arc<Vec<u8>>,
    /// `(header name, value)` that carries the whole object CRC64.
    crc_header: Option<(&'static str, String)>,
    etag: &'static str,
    date: Option<&'static str>,
    version: Option<&'static str>,
    corrupt_gets: Arc<AtomicU32>,
}

async fn serve(c: Cloud) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let c = c.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                let mut got = 0;
                loop {
                    let Ok(n) = sock.read(&mut buf[got..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    got += n;
                    if buf[..got].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..got]).to_string();
                let method = head.split(' ').next().unwrap_or("").to_owned();
                let header = |name: &str| {
                    head.lines().find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
                    })
                };
                let total = c.body.len();
                let mut extra = format!(
                    "ETag: {}\r\nLast-Modified: Tue, 17 Nov 2026 10:00:00 GMT\r\n",
                    c.etag
                );
                if let Some((k, v)) = &c.crc_header {
                    extra.push_str(&format!("{k}: {v}\r\n"));
                }
                if let Some(d) = c.date {
                    extra.push_str(&format!("Date: {d}\r\n"));
                }
                if let Some(v) = c.version {
                    extra.push_str(&format!("x-amz-version-id: {v}\r\n"));
                }
                if method == "HEAD" {
                    let r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\n{extra}Connection: close\r\n\r\n"
                    );
                    let _ = sock.write_all(r.as_bytes()).await;
                    return;
                }
                if let Some(m) = header("if-match")
                    && m != c.etag
                {
                    let _ = sock
                        .write_all(b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }
                let (start, end) = header("range")
                    .and_then(|r| {
                        let r = r.strip_prefix("bytes=")?.to_owned();
                        let (a, z) = r.split_once('-')?;
                        let a: usize = a.parse().ok()?;
                        let z: usize = z.parse().unwrap_or(total - 1);
                        Some((a, z.min(total - 1)))
                    })
                    .unwrap_or((0, total - 1));
                let mut data = c.body[start..=end].to_vec();
                if c.corrupt_gets.load(Ordering::SeqCst) > 0
                    && c.corrupt_gets.fetch_sub(1, Ordering::SeqCst) > 0
                    && !data.is_empty()
                {
                    data[0] ^= 0xff;
                }
                let r = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{total}\r\n{extra}Connection: close\r\n\r\n",
                    data.len()
                );
                let _ = sock.write_all(r.as_bytes()).await;
                let _ = sock.write_all(&data).await;
            });
        }
    });
    format!("http://{addr}")
}

fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 37 % 251) as u8).collect()
}

fn cloud(body: Vec<u8>, crc_header: Option<(&'static str, String)>) -> Cloud {
    Cloud {
        body: Arc::new(body),
        crc_header,
        etag: "\"d41d8cd98f00b204e9800998ecf8427e\"",
        date: None,
        version: None,
        corrupt_gets: Arc::new(AtomicU32::new(0)),
    }
}

fn drive_config(backend: &str, endpoint: &str) -> serde_json::Value {
    let region = if backend == "r2" { "auto" } else { "cn-test-1" };
    let mut v = json!({"bucket": "bkt", "endpoint": endpoint, "region": region, "path_style": true, "secret_ref": "cred"});
    if backend == "r2" {
        v["account_id"] = json!("0123456789abcdef0123456789abcdef");
    }
    v
}

fn resolver() -> (SecretResolver, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "oo-cloud-mimic-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("cred"),
        r#"{"access_key_id":"AKTEST","secret_access_key":"SKTEST"}"#,
    )
    .unwrap();
    (
        SecretResolver {
            secrets_dir: dir.clone(),
        },
        dir,
    )
}

fn open(backend: &str, endpoint: &str) -> S3Source {
    let (r, dir) = resolver();
    let cfg =
        S3Config::from_drive_config_for(backend, &drive_config(backend, endpoint), &r).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    S3Source::open(&cfg).unwrap()
}

#[test]
fn each_provider_resolves_its_endpoint_style_and_region() {
    let (r, dir) = resolver();
    let base = |extra: serde_json::Value| {
        let mut v = json!({"bucket": "bkt", "secret_ref": "cred"});
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    };
    let oss = S3Config::from_drive_config_for("oss", &base(json!({"region": "cn-hangzhou"})), &r)
        .unwrap();
    assert_eq!(
        oss.endpoint.as_deref(),
        Some("https://s3.oss-cn-hangzhou.aliyuncs.com")
    );
    assert!(!oss.path_style, "OSS only serves virtual-hosted buckets");
    let cos = S3Config::from_drive_config_for("cos", &base(json!({"region": "ap-guangzhou"})), &r)
        .unwrap();
    assert_eq!(
        cos.endpoint.as_deref(),
        Some("https://cos.ap-guangzhou.myqcloud.com")
    );
    assert!(!cos.path_style);
    let r2 = S3Config::from_drive_config_for(
        "r2",
        &base(json!({"account_id": "0123456789abcdef0123456789abcdef"})),
        &r,
    )
    .unwrap();
    assert_eq!(r2.region, "auto");
    assert_eq!(
        r2.endpoint.as_deref(),
        Some("https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com")
    );
    assert!(r2.path_style);
    let s3 = S3Config::from_drive_config_for("s3", &base(json!({})), &r).unwrap();
    assert!(
        s3.endpoint.is_none() && !s3.path_style,
        "AWS: SDK endpoint, virtual-hosted"
    );
    // A self-hosted endpoint wins and is path style.
    let own = S3Config::from_drive_config_for(
        "oss",
        &base(json!({"endpoint": "http://127.0.0.1:9"})),
        &r,
    )
    .unwrap();
    assert!(own.path_style);
    // What cannot be resolved, or could move the host, is refused.
    for bad in [
        ("oss", base(json!({}))),
        ("cos", base(json!({"region": "a.evil.com"}))),
        ("r2", base(json!({}))),
        ("r2", base(json!({"account_id": "x.evil.com"}))),
        ("gcs", base(json!({}))),
    ] {
        assert!(
            S3Config::from_drive_config_for(bad.0, &bad.1, &r).is_err(),
            "{}",
            bad.0
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_source_reports_the_backend_of_its_provider() {
    let url = serve(cloud(payload(10), None)).await;
    for (b, want) in [
        ("s3", DocBackend::S3),
        ("oss", DocBackend::Oss),
        ("cos", DocBackend::Cos),
        ("r2", DocBackend::R2),
    ] {
        assert_eq!(open(b, &url).backend(), want);
    }
}

async fn crc_flow(backend: &str, header: &'static str) {
    let body = payload(20_000);
    let value = crc64(Crc64Kind::Ecma, &body).to_string();
    let c = cloud(body.clone(), Some((header, value)));
    c.corrupt_gets.store(1, Ordering::SeqCst);
    let src = open(backend, &serve(c.clone()).await);
    let mut got = Vec::new();
    let (digest, meta) = src
        .stream_object(b"contracts/a.pdf", |p| {
            match p {
                Streamed::Bytes(b) => got.extend_from_slice(&b),
                Streamed::Restart => got.clear(),
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(digest.len, 20_000);
    assert_eq!(meta.crc64, Some(crc64(Crc64Kind::Ecma, &body)));
    assert_eq!(
        got, body,
        "after the restart only the clean download remains"
    );
    // Three bad downloads in a row end in the spec error.
    c.corrupt_gets.store(100, Ordering::SeqCst);
    let src = open(backend, &serve(c).await);
    assert_eq!(
        src.stream_object(b"a.pdf", |_| Ok(())).await.unwrap_err(),
        DocSourceError::ChecksumMismatch
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn oss_crc64_ecma_is_verified_and_a_corrupt_transfer_is_fetched_again() {
    crc_flow("oss", "x-oss-hash-crc64ecma").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cos_crc64_ecma_is_verified_and_a_corrupt_transfer_is_fetched_again() {
    crc_flow("cos", "x-cos-hash-crc64ecma").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_r2_like_store_has_no_crc_no_versions_and_a_multipart_etag() {
    let mut c = cloud(payload(5_000), None);
    c.etag = "\"d41d8cd98f00b204e9800998ecf8427e-3\"";
    let src = open("r2", &serve(c).await);
    let p = src.probe(b"a.md").await.unwrap();
    assert!(!p.strong_etag, "a multipart tag is only a change signal");
    assert!(!p.versioning && p.crc64.is_none());
    let caps: BTreeSet<DocCapability> = src.capabilities();
    assert!(caps.contains(&DocCapability::RangedRead) && !caps.contains(&DocCapability::Crc64));
    assert!(
        !caps.contains(&DocCapability::Versioning) && !caps.contains(&DocCapability::StrongEtag)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oss_like_store_with_versions_and_ecma_reports_those_capabilities() {
    let body = payload(3_000);
    let mut c = cloud(
        body.clone(),
        Some((
            "x-oss-hash-crc64ecma",
            crc64(Crc64Kind::Ecma, &body).to_string(),
        )),
    );
    c.version = Some("CAEQ-v1");
    let src = open("oss", &serve(c).await);
    let p = src.probe(b"a.md").await.unwrap();
    assert!(p.strong_etag && p.versioning && p.if_match);
    assert_eq!(p.crc64, Some(Crc64Kind::Ecma));
    assert!(src.capabilities().contains(&DocCapability::Crc64));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_store_ten_minutes_off_is_reported_as_clock_skew() {
    let mut c = cloud(payload(10), None);
    // Far from any real clock: the skew is whatever the difference is, well above five minutes.
    c.date = Some("Tue, 17 Nov 2026 10:10:00 GMT");
    let src = open("cos", &serve(c).await);
    assert!(src.clock_skew().is_none(), "nothing seen yet");
    src.stat(b"a.md").await.unwrap();
    assert!(src.clock_skew().is_some());
    let mut ok = cloud(payload(10), None);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // `Date` of this very second, built from the epoch without a date library.
    let days = now / 86_400;
    let (y, m, d) = civil(days as i64);
    let rem = now % 86_400;
    let date: &'static str = Box::leak(
        format!(
            "Mon, {d:02} {} {y} {:02}:{:02}:{:02} GMT",
            [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
            ][m as usize - 1],
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
        .into_boxed_str(),
    );
    ok.date = Some(date);
    let src = open("cos", &serve(ok).await);
    src.stat(b"a.md").await.unwrap();
    assert!(
        src.clock_skew().is_none(),
        "a clock that agrees is not reported"
    );
}

/// Days since 1970-01-01 to (year, month, day).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The same checks against a real S3 implementation: a header injecting proxy in front of MinIO or RustFS
/// makes it answer like OSS and COS do (`OO_S3_ENDPOINT`, `OO_S3_ACCESS`, `OO_S3_SECRET`, `OO_S3_BUCKET`).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn cloud_mimic_minio() {
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let upstream = env("OO_S3_ENDPOINT");
    let mk = |endpoint: &str| S3Config {
        provider: oo_connectors::objectstore_s3::compat::Provider::Oss,
        bucket: env("OO_S3_BUCKET"),
        prefix: "t-cloud-mimic/".into(),
        endpoint: Some(endpoint.to_owned()),
        region: "cn-test-1".into(),
        path_style: true,
        excludes: vec![],
        credentials: S3Credentials::new(&env("OO_S3_ACCESS"), &env("OO_S3_SECRET"), None).unwrap(),
    };
    let direct = S3Source::open(&mk(&upstream)).unwrap();
    let body: Vec<u8> = (0..300_000usize).map(|i| (i * 13 % 251) as u8).collect();
    direct
        .operator_for_tests()
        .write("contracts/a.pdf", body.clone())
        .await
        .unwrap();
    let crc = crc64(Crc64Kind::Ecma, &body).to_string();
    let proxy = proxy_adding(&upstream, "x-oss-hash-crc64ecma", &crc).await;
    let src = S3Source::open(&mk(&proxy)).unwrap();
    let (digest, meta) = src
        .stream_object(b"contracts/a.pdf", |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(digest.len, body.len() as u64);
    assert_eq!(
        meta.crc64,
        Some(crc64(Crc64Kind::Ecma, &body)),
        "the OSS header was read through a real S3 store"
    );
    assert_eq!(src.backend(), DocBackend::Oss);
    // A wrong header value makes every download fail the check.
    let bad = proxy_adding(&upstream, "x-oss-hash-crc64ecma", "12345").await;
    let src = S3Source::open(&mk(&bad)).unwrap();
    assert_eq!(
        src.stream_object(b"contracts/a.pdf", |_| Ok(()))
            .await
            .unwrap_err(),
        DocSourceError::ChecksumMismatch
    );
    direct
        .operator_for_tests()
        .delete("contracts/a.pdf")
        .await
        .unwrap();
}

/// A one-hop HTTP proxy that adds a header to every response.
async fn proxy_adding(upstream: &str, name: &'static str, value: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (upstream, value) = (upstream.to_owned(), value.to_owned());
    let client = reqwest::Client::new();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let (upstream, value, client) = (upstream.clone(), value.clone(), client.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                let mut got = 0;
                loop {
                    let Ok(n) = sock.read(&mut buf[got..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    got += n;
                    if buf[..got].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..got]).to_string();
                let mut lines = head.lines();
                let mut first = lines.next().unwrap_or("").split(' ');
                let (method, target) = (first.next().unwrap_or("GET"), first.next().unwrap_or("/"));
                let mut req = client.request(
                    method.parse().unwrap_or(reqwest::Method::GET),
                    format!("{upstream}{target}"),
                );
                for l in lines {
                    if let Some((k, v)) = l.split_once(':') {
                        let k = k.trim();
                        // The Host header stays the client's: it is part of what the request signature covers.
                        if !k.eq_ignore_ascii_case("connection")
                            && !k.eq_ignore_ascii_case("content-length")
                        {
                            req = req.header(k, v.trim());
                        }
                    }
                }
                let Ok(resp) = req.send().await else { return };
                let status = resp.status();
                let mut out = format!(
                    "HTTP/1.1 {} {}\r\n",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("")
                );
                for (k, v) in resp.headers() {
                    if k != "content-length" && k != "transfer-encoding" && k != "connection" {
                        out.push_str(&format!("{k}: {}\r\n", v.to_str().unwrap_or("")));
                    }
                }
                out.push_str(&format!("{name}: {value}\r\n"));
                let is_head = method == "HEAD";
                let len = resp
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let bytes = if is_head {
                    Default::default()
                } else {
                    resp.bytes().await.unwrap_or_default()
                };
                out.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    if is_head {
                        len.unwrap_or_default()
                    } else {
                        bytes.len().to_string()
                    }
                ));
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&bytes).await;
            });
        }
    });
    format!("http://{addr}")
}
