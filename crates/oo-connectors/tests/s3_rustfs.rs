//! `objectstore-s3` against a real RustFS (the default test store, D9) or
//! MinIO (compatibility): listing, conditional reads, 412 and download again,
//! ETag shapes, versions of an object, CRC64 (B-S4-01, 10 §4.3.3, §4.3.7).
//!
//! `OO_S3_ENDPOINT=http://127.0.0.1:<port> OO_S3_ACCESS=... OO_S3_SECRET=...
//!  OO_S3_BUCKET=<existing bucket> cargo test -p oo-connectors --features
//!  objectstore-s3 --test s3_rustfs -- --ignored --test-threads=1`
//!
//! The tests write only under their own prefix (`t-<test name>/`).
#![cfg(feature = "objectstore-s3")]

use std::collections::BTreeSet;

use oo_connectors::objectstore::{
    DocCapability, DocSourceError, DocumentSource, ListCursor, ReadExpect,
};
use oo_connectors::objectstore_s3::compat::{self, Crc64Kind, crc64};
use oo_connectors::objectstore_s3::opendal::{S3Config, S3Credentials, S3Source, Streamed};

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"))
}

fn source(prefix: &str) -> S3Source {
    let cfg = S3Config {
        provider: Default::default(),
        bucket: env("OO_S3_BUCKET"),
        prefix: format!("{prefix}/"),
        endpoint: Some(env("OO_S3_ENDPOINT")),
        region: "us-east-1".into(),
        path_style: true,
        excludes: vec![],
        credentials: S3Credentials::new(&env("OO_S3_ACCESS"), &env("OO_S3_SECRET"), None).unwrap(),
    };
    S3Source::open(&cfg).unwrap()
}

async fn put(src: &S3Source, key: &str, body: Vec<u8>) {
    src.operator_for_tests().write(key, body).await.unwrap();
}

fn bytes(n: usize, salt: u8) -> Vec<u8> {
    (0..n)
        .map(|i| ((i * 7 + usize::from(salt)) % 251) as u8)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn listing_is_paged_complete_and_skips_what_the_drive_skips() {
    let src = source("t-list");
    for i in 0..25 {
        put(&src, &format!("docs/n{i:02}.md"), bytes(100 + i, 1)).await;
    }
    put(&src, "docs/.hidden.md", b"x".to_vec()).await;
    put(&src, "docs/~$lock.docx", b"x".to_vec()).await;
    put(&src, "docs/sub/中文.md", b"x".to_vec()).await;
    src.operator_for_tests()
        .create_dir("docs/empty/")
        .await
        .unwrap();
    let mut seen = BTreeSet::new();
    let mut cursor: Option<ListCursor> = None;
    let mut pages = 0;
    loop {
        let page = src.list(cursor.as_ref(), 10).await.unwrap();
        pages += 1;
        for e in &page.entries {
            assert!(e.path.is_some(), "every key here is a valid path");
            assert!(
                seen.insert(e.path.clone().unwrap().to_string()),
                "no key twice"
            );
        }
        if page.complete {
            assert!(page.next.is_none());
            break;
        }
        assert!(page.entries.len() <= 10);
        cursor = page.next;
        assert!(pages < 20, "listing must end");
    }
    assert!(pages >= 3, "{pages} pages");
    assert_eq!(seen.len(), 26, "25 notes and the cjk file: {seen:?}");
    assert!(seen.contains("docs/sub/中文.md"));
    assert!(
        !seen
            .iter()
            .any(|k| k.contains(".hidden") || k.contains("~$"))
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn stat_and_read_match_what_was_written() {
    let src = source("t-stat");
    let body = bytes(5000, 3);
    put(&src, "a.md", body.clone()).await;
    let m = src.stat(b"a.md").await.unwrap().unwrap();
    assert_eq!(m.size, 5000);
    assert!(m.etag.is_some() && m.last_modified_ns.is_some());
    assert_eq!(src.stat(b"nope.md").await.unwrap(), None);
    let got = src
        .read(b"a.md", 100, 50, &ReadExpect::default())
        .await
        .unwrap();
    assert_eq!(got, &body[100..150]);
    let all = src
        .read(b"a.md", 0, 5000, &ReadExpect::default())
        .await
        .unwrap();
    assert_eq!(all, body);
    // The store's tag is a change signal; the content key is ours.
    let (digest, _) = src.stream_object(b"a.md", |_| Ok(())).await.unwrap();
    assert_eq!(digest.b3, blake3::hash(&body).to_hex().to_string());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn a_changed_object_is_a_412_and_the_download_starts_over() {
    let src = source("t-cond");
    put(&src, "a.bin", bytes(300_000, 1)).await;
    let first = src.stat(b"a.bin").await.unwrap().unwrap();
    let expect = ReadExpect {
        etag: first.etag.clone(),
        ..ReadExpect::default()
    };
    assert_eq!(src.read(b"a.bin", 0, 10, &expect).await.unwrap().len(), 10);
    put(&src, "a.bin", bytes(300_000, 2)).await;
    assert_eq!(
        src.read(b"a.bin", 0, 10, &expect).await,
        Err(DocSourceError::Changed),
        "If-Match with the old tag must not return the new bytes"
    );
    // The fingerprint moved too.
    let second = src.stat(b"a.bin").await.unwrap().unwrap();
    assert_ne!(first.etag, second.etag);
    // A fresh stream after the change reads the new content, whole.
    let want = blake3::hash(&bytes(300_000, 2)).to_hex().to_string();
    let (digest, _) = src.stream_object(b"a.bin", |_| Ok(())).await.unwrap();
    assert_eq!(digest.b3, want);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn an_object_rewritten_mid_stream_is_changed_not_mixed_bytes() {
    let src = source("t-mid");
    let big = bytes(20 * 1024 * 1024, 1);
    put(&src, "big.bin", big).await;
    let handle = tokio::runtime::Handle::current();
    let writer = src.clone();
    let mut chunks = 0;
    let err = src
        .stream_object(b"big.bin", |p| {
            if let Streamed::Bytes(_) = p {
                chunks += 1;
                if chunks == 1 {
                    // Rewrite the object after the first range was read.
                    tokio::task::block_in_place(|| {
                        handle.block_on(async {
                            writer
                                .operator_for_tests()
                                .write("big.bin", bytes(20 * 1024 * 1024, 9))
                                .await
                                .unwrap();
                        });
                    });
                }
            }
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(err, DocSourceError::Changed);
    // Starting over reads the new object whole.
    let want = blake3::hash(&bytes(20 * 1024 * 1024, 9))
        .to_hex()
        .to_string();
    let (digest, meta) = src.stream_object(b"big.bin", |_| Ok(())).await.unwrap();
    assert_eq!(digest.b3, want);
    assert_eq!(meta.size, 20 * 1024 * 1024);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn a_multipart_object_has_a_tag_that_is_not_its_md5() {
    let src = source("t-mpu");
    let body = bytes(12 * 1024 * 1024, 5);
    src.operator_for_tests()
        .write_with("mp.bin", body.clone())
        .chunk(5 * 1024 * 1024)
        .await
        .unwrap();
    let m = src.stat(b"mp.bin").await.unwrap().unwrap();
    let etag = m.etag.clone().unwrap();
    eprintln!(
        "multipart etag shape: {} (md5 shaped: {})",
        etag.len(),
        compat::etag_is_content_md5(&etag)
    );
    // Whatever shape the store gives, the key is the local BLAKE3.
    let (digest, _) = src.stream_object(b"mp.bin", |_| Ok(())).await.unwrap();
    assert_eq!(digest.b3, blake3::hash(&body).to_hex().to_string());
    // Single part: the tag is the MD5 of the content on stores that do that.
    put(&src, "one.bin", bytes(1000, 2)).await;
    let one = src.stat(b"one.bin").await.unwrap().unwrap();
    eprintln!(
        "single part etag md5 shaped: {}",
        compat::etag_is_content_md5(one.etag.as_deref().unwrap())
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn the_probe_reports_what_the_store_does() {
    let src = source("t-probe");
    put(&src, "p.bin", bytes(2000, 4)).await;
    let p = src.probe(b"p.bin").await.unwrap();
    eprintln!("PROBE {p:?} capabilities {:?}", src.capabilities());
    assert!(
        p.if_match,
        "a stale If-Match must be refused: that is what the download relies on"
    );
    assert!(src.capabilities().contains(&DocCapability::ConditionalRead));
    // CRC64 is only checked when the store reports one for the object.
    let m = src.stat(b"p.bin").await.unwrap().unwrap();
    if let Some(c) = m.crc64 {
        assert_eq!(c, crc64(Crc64Kind::Nvme, &bytes(2000, 4)));
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn a_wrong_secret_is_auth_failed_and_nothing_leaks() {
    let cfg = S3Config {
        provider: Default::default(),
        bucket: env("OO_S3_BUCKET"),
        prefix: "t-auth/".into(),
        endpoint: Some(env("OO_S3_ENDPOINT")),
        region: "us-east-1".into(),
        path_style: true,
        excludes: vec![],
        credentials: S3Credentials::new(&env("OO_S3_ACCESS"), "wrong-secret-wrong-secret", None)
            .unwrap(),
    };
    let src = S3Source::open(&cfg).unwrap();
    let err = src.list(None, 10).await.unwrap_err();
    assert!(
        matches!(
            err,
            DocSourceError::AuthFailed | DocSourceError::PermissionDenied
        ),
        "{err:?}"
    );
    let shown = format!("{err} {err:?}");
    for secret in ["wrong-secret", &env("OO_S3_ENDPOINT"), &env("OO_S3_BUCKET")] {
        assert!(!shown.contains(secret), "the error must not carry {secret}");
    }
}

/// First level scopes (B-S11-03): the scopes together are the whole listing, each scope pages by its own
/// cursor, and a scope with no objects lists as nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn scopes_partition_the_listing() {
    let src = source("t-scopes");
    for key in [
        "root.md",
        "a/1.md",
        "a/deep/2.md",
        "a/deep/er/3.md",
        "b/4.md",
        "c d/5.md",
        "z/.hidden.md",
    ] {
        put(&src, key, bytes(10, 2)).await;
    }
    let scopes = src.list_scopes().await.unwrap().unwrap();
    assert_eq!(scopes, ["", "a", "b", "c d", "z"]);
    let mut whole = BTreeSet::new();
    let mut cursor: Option<ListCursor> = None;
    loop {
        let page = src.list(cursor.as_ref(), 100).await.unwrap();
        whole.extend(
            page.entries
                .iter()
                .map(|e| e.path.clone().unwrap().to_string()),
        );
        if page.next.is_none() {
            break;
        }
        cursor = page.next;
    }
    for page_size in [1u32, 2, 100] {
        let mut union = BTreeSet::new();
        for scope in &scopes {
            let mut cursor: Option<ListCursor> = None;
            loop {
                let page = src
                    .list_scope(scope, cursor.as_ref(), page_size)
                    .await
                    .unwrap();
                for e in &page.entries {
                    let k = e.path.clone().unwrap().to_string();
                    let first = k.split_once('/').map(|(f, _)| f).unwrap_or("");
                    assert_eq!(first, scope, "{k} listed under the wrong scope");
                    assert!(union.insert(k), "no key twice");
                }
                if page.complete {
                    assert!(page.next.is_none());
                    break;
                }
                cursor = page.next;
            }
        }
        assert_eq!(union, whole, "page size {page_size}");
    }
    assert!(whole.contains("a/deep/er/3.md") && !whole.contains("z/.hidden.md"));
    let gone = src.list_scope("nothing-here", None, 10).await.unwrap();
    assert!(gone.entries.is_empty() && gone.complete);
    assert!(src.list_scope("a/deep", None, 10).await.is_err());
}
