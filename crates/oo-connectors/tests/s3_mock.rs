//! `objectstore-s3` against a tiny in-process S3 look-alike: retries on 5xx,
//! `Retry-After`, `If-Match` failures and CRC64 verification, with no
//! container (B-S4-01). The real stores are in `s3_rustfs.rs`.
#![cfg(feature = "objectstore-s3")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oo_connectors::objectstore::{DocSourceError, DocumentSource, ReadExpect};
use oo_connectors::objectstore_s3::compat::{Crc64Kind, crc64, encode_base64_u64};
use oo_connectors::objectstore_s3::opendal::{S3Config, S3Credentials, S3Source, Streamed};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
struct Behaviour {
    body: Arc<Vec<u8>>,
    /// Answer this many requests with 503 first.
    fail_first: Arc<AtomicU32>,
    /// `x-amz-checksum-crc64nvme` to send; `None` = none.
    crc: Option<u64>,
    /// How many GETs sent a wrong body byte (to force CRC retries).
    corrupt_gets: Arc<AtomicU32>,
    etag: &'static str,
    requests: Arc<AtomicU32>,
    throttle_once: Arc<AtomicU32>,
}

async fn serve(b: Behaviour) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let b = b.clone();
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
                let mut lines = head.lines();
                let first = lines.next().unwrap_or("").to_owned();
                let method = first.split(' ').next().unwrap_or("").to_owned();
                let header = |name: &str| {
                    head.lines().find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
                    })
                };
                b.requests.fetch_add(1, Ordering::SeqCst);
                if b.throttle_once.load(Ordering::SeqCst) > 0
                    && b.throttle_once.fetch_sub(1, Ordering::SeqCst) > 0
                {
                    let _ = sock
                        .write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }
                if b.fail_first.load(Ordering::SeqCst) > 0
                    && b.fail_first.fetch_sub(1, Ordering::SeqCst) > 0
                {
                    let _ = sock
                        .write_all(b"HTTP/1.1 503 Slow Down\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }
                let total = b.body.len();
                let mut extra = format!(
                    "ETag: {}\r\nLast-Modified: Tue, 17 Nov 2026 10:00:00 GMT\r\n",
                    b.etag
                );
                if let Some(c) = b.crc {
                    extra.push_str(&format!(
                        "x-amz-checksum-crc64nvme: {}\r\n",
                        encode_base64_u64(c)
                    ));
                }
                if method == "HEAD" {
                    let r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\n{extra}Connection: close\r\n\r\n"
                    );
                    let _ = sock.write_all(r.as_bytes()).await;
                    return;
                }
                if let Some(m) = header("if-match") {
                    if m != b.etag {
                        let _ = sock
                            .write_all(b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                        return;
                    }
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
                let mut data = b.body[start..=end].to_vec();
                if b.corrupt_gets.load(Ordering::SeqCst) > 0
                    && b.corrupt_gets.fetch_sub(1, Ordering::SeqCst) > 0
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

fn behaviour(body: Vec<u8>) -> Behaviour {
    let crc = Some(crc64(Crc64Kind::Nvme, &body));
    Behaviour {
        body: Arc::new(body),
        fail_first: Arc::new(AtomicU32::new(0)),
        crc,
        corrupt_gets: Arc::new(AtomicU32::new(0)),
        etag: "\"d41d8cd98f00b204e9800998ecf8427e\"",
        requests: Arc::new(AtomicU32::new(0)),
        throttle_once: Arc::new(AtomicU32::new(0)),
    }
}

fn source(endpoint: String) -> S3Source {
    let cfg = S3Config {
        bucket: "bkt".into(),
        prefix: String::new(),
        endpoint: Some(endpoint),
        region: "us-east-1".into(),
        path_style: true,
        excludes: vec![],
        credentials: S3Credentials::new("AKTEST", "SKTEST", None).unwrap(),
    };
    S3Source::open(&cfg).unwrap()
}

fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_5xx_is_retried_and_the_object_is_read() {
    let body = payload(10_000);
    let b = behaviour(body.clone());
    b.fail_first.store(2, Ordering::SeqCst);
    let src = source(serve(b.clone()).await);
    let (digest, meta) = src.stream_object(b"a.md", |_| Ok(())).await.unwrap();
    assert_eq!(digest.len, 10_000);
    assert_eq!(meta.size, 10_000);
    assert!(b.requests.load(Ordering::SeqCst) >= 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_429_surfaces_as_throttled_after_the_retries() {
    let b = behaviour(payload(10));
    b.throttle_once.store(100, Ordering::SeqCst);
    let src = source(serve(b).await);
    let err = src.stat(b"a.md").await.unwrap_err();
    assert!(
        matches!(err, DocSourceError::Throttled { retry_after_ms } if retry_after_ms >= 1000),
        "{err:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_etag_is_changed_not_mixed_bytes() {
    let b = behaviour(payload(100));
    let src = source(serve(b).await);
    let stale = ReadExpect {
        etag: Some("\"ffffffffffffffffffffffffffffffff\"".into()),
        ..ReadExpect::default()
    };
    assert_eq!(
        src.read(b"a.md", 0, 10, &stale).await,
        Err(DocSourceError::Changed)
    );
    let ok = ReadExpect {
        etag: Some("\"d41d8cd98f00b204e9800998ecf8427e\"".into()),
        ..ReadExpect::default()
    };
    assert_eq!(src.read(b"a.md", 5, 10, &ok).await.unwrap().len(), 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupt_transfer_is_downloaded_again_by_crc64() {
    let body = payload(20_000);
    let b = behaviour(body.clone());
    b.corrupt_gets.store(1, Ordering::SeqCst);
    let src = source(serve(b.clone()).await);
    let mut got = Vec::new();
    let (digest, meta) = src
        .stream_object(b"a.md", |c| {
            match c {
                Streamed::Bytes(b) => got.extend_from_slice(&b),
                Streamed::Restart => got.clear(),
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(digest.len, 20_000);
    assert_eq!(meta.crc64, Some(crc64(Crc64Kind::Nvme, &body)));
    assert_eq!(
        got, body,
        "after the restart only the clean download remains"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn three_corrupt_transfers_end_in_a_checksum_error() {
    let b = behaviour(payload(20_000));
    b.corrupt_gets.store(100, Ordering::SeqCst);
    let src = source(serve(b).await);
    let err = src.stream_object(b"a.md", |_| Ok(())).await.unwrap_err();
    assert_eq!(err, DocSourceError::Protocol("checksum mismatch"));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_crc64_there_is_nothing_to_verify() {
    let mut b = behaviour(payload(500));
    b.crc = None;
    b.corrupt_gets.store(1, Ordering::SeqCst);
    let src = source(serve(b).await);
    let (_, meta) = src.stream_object(b"a.md", |_| Ok(())).await.unwrap();
    assert_eq!(meta.crc64, None);
}
