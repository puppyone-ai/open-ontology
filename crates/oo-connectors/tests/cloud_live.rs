//! READY TO RUN BY THE OWNER: the real bucket test for OSS, COS, R2 (and S3), B-S11-01, 10 §4.3.7.
//!
//! No real bucket was available when this was written and no account may be created for it, so every
//! "not confirmed" cell of the compatibility table is still unconfirmed until this runs. It writes only under
//! its own prefix (`oo-cloud-live-<pid>/`, removed at the end) and records what the store answered.
//!
//! ```text
//! OO_CLOUD_BACKEND=oss            # oss | cos | r2 | s3
//! OO_CLOUD_BUCKET=<bucket>        # COS: name-appid
//! OO_CLOUD_REGION=cn-hangzhou     # not needed for r2
//! OO_CLOUD_ACCOUNT_ID=<id>        # r2 only
//! OO_CLOUD_ENDPOINT=<url>         # optional override (internal endpoint, a proxy)
//! OO_CLOUD_SECRET_REF=file:/abs/path/to/credential.json   # or env:NAME; never the key itself
//! OO_CLOUD_EVIDENCE=/tmp/cloud-live-oss.json              # optional: observed answers, no secrets
//! cargo test -p oo-connectors --features objectstore-oss,objectstore-cos --test cloud_live -- --ignored --test-threads=1
//! ```
//!
//! The credential file is `{"access_key_id": "...", "secret_access_key": "..."}` (a `session_token` is
//! accepted). It is read through `secret_ref` like a Drive does; this file never prints or stores it.
//! `objectstore-oss` and `objectstore-cos` are aliases of `objectstore-s3` (the S3 compatible path; coordinator
//! patch `B-S11-01-features.patch`); the test also builds with `--features objectstore-s3`.
#![cfg(feature = "objectstore-s3")]

use std::collections::BTreeSet;

use oo_connectors::objectstore::{
    DocCapability, DocSourceError, DocumentSource, ListCursor, ReadExpect,
};
use oo_connectors::objectstore_s3::compat::{self, Crc64Kind, crc64};
use oo_connectors::objectstore_s3::opendal::{S3Config, S3Source, SecretResolver};
use serde_json::{Value, json};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn need(k: &str) -> String {
    env(k).unwrap_or_else(|| panic!("{k} is not set (see the header of cloud_live.rs)"))
}

fn source() -> (S3Source, String) {
    let backend = need("OO_CLOUD_BACKEND");
    let prefix = format!("oo-cloud-live-{}/", std::process::id());
    let mut cfg = json!({"bucket": need("OO_CLOUD_BUCKET"), "prefix": prefix, "secret_ref": need("OO_CLOUD_SECRET_REF")});
    for (k, e) in [
        ("region", "OO_CLOUD_REGION"),
        ("account_id", "OO_CLOUD_ACCOUNT_ID"),
        ("endpoint", "OO_CLOUD_ENDPOINT"),
    ] {
        if let Some(v) = env(e) {
            cfg[k] = Value::String(v);
        }
    }
    let resolver = SecretResolver {
        secrets_dir: std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(".config/open-ontology/secrets"),
    };
    let config = S3Config::from_drive_config_for(&backend, &cfg, &resolver)
        .expect("the drive config of this backend");
    (S3Source::open(&config).expect("open"), backend)
}

fn bytes(n: usize, salt: u8) -> Vec<u8> {
    (0..n)
        .map(|i| ((i * 7 + usize::from(salt)) % 251) as u8)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real bucket and credentials through secret_ref (OO_CLOUD_*)"]
async fn the_cloud_answers_the_way_the_compat_table_says() {
    let (src, backend) = source();
    let op = src.operator_for_tests();
    let mut seen = json!({"backend": backend, "table": format!("{:?}", compat::COMPAT_TABLE.iter().map(|r| r.provider).collect::<Vec<_>>())});

    // 1. Listing: paged, complete, every key once (page size 10 over 25 objects).
    for i in 0..25 {
        op.write(&format!("docs/n{i:02}.md"), bytes(100 + i, 1))
            .await
            .unwrap();
    }
    let mut keys = BTreeSet::new();
    let (mut cursor, mut pages): (Option<ListCursor>, u32) = (None, 0);
    loop {
        let page = src.list(cursor.as_ref(), 10).await.expect("list");
        pages += 1;
        for e in &page.entries {
            assert!(keys.insert(e.raw_key.clone()), "no key twice");
        }
        if page.complete {
            break;
        }
        cursor = page.next;
        assert!(pages < 50, "listing must end");
    }
    assert_eq!(keys.len(), 25);
    assert!(pages >= 3, "the page limit is honoured");
    seen["list"] = json!({"objects": 25, "pages": pages, "consistent_after_write": true});

    // 2. A small object: ETag is the MD5 of the content (or not: recorded), stat carries size and mtime.
    let small = bytes(4096, 3);
    op.write("one.bin", small.clone()).await.unwrap();
    let meta = src.stat(b"one.bin").await.unwrap().expect("stat");
    assert_eq!(meta.size, 4096);
    seen["etag_single_part_is_md5"] = json!(
        meta.etag
            .as_deref()
            .is_some_and(compat::etag_is_content_md5)
    );
    seen["versioning"] = json!(meta.version_id.is_some());
    seen["crc64_on_head"] = json!(meta.crc64.is_some());

    // 3. If-Match on a ranged read: a stale tag is `Changed`, the right one reads.
    let stale = ReadExpect {
        etag: Some("\"00000000000000000000000000000000\"".into()),
        ..Default::default()
    };
    assert_eq!(
        src.read(b"one.bin", 0, 10, &stale).await,
        Err(DocSourceError::Changed),
        "If-Match is honoured"
    );
    let good = ReadExpect {
        etag: meta.etag.clone(),
        ..Default::default()
    };
    assert_eq!(
        src.read(b"one.bin", 5, 10, &good).await.unwrap(),
        small[5..15]
    );
    seen["if_match"] = json!(true);

    // 4. A multipart sized object (20 MiB): the ETag is not the content MD5, and the content key stays BLAKE3.
    let big = bytes(20 * 1024 * 1024, 9);
    op.write("big.bin", big.clone()).await.unwrap();
    let (digest, bmeta) = src
        .stream_object(b"big.bin", |_| Ok(()))
        .await
        .expect("download");
    assert_eq!(digest.len, big.len() as u64);
    seen["etag_multipart_is_md5"] = json!(
        bmeta
            .etag
            .as_deref()
            .is_some_and(compat::etag_is_content_md5)
    );
    // When the cloud reports a CRC64 it must be the right kind and equal ours (checked inside the download too).
    if let Some(c) = bmeta.crc64 {
        let kind = if backend == "oss" || backend == "cos" {
            Crc64Kind::Ecma
        } else {
            Crc64Kind::Nvme
        };
        assert_eq!(
            c,
            crc64(kind, &big),
            "the reported CRC64 is of the whole object"
        );
        seen["crc64_kind"] = json!(format!("{kind:?}"));
    }

    // 5. The capability probe (what the Drive will record).
    let p = src.probe(b"one.bin").await.unwrap();
    seen["probe"] = json!({"if_match": p.if_match, "strong_etag": p.strong_etag, "versioning": p.versioning, "crc64": p.crc64.map(|k| format!("{k:?}"))});

    // 6. Clock.
    seen["clock_skew_ms"] = json!(src.clock_skew());
    assert!(
        src.clock_skew().is_none(),
        "the machine clock is within five minutes of the store"
    );

    // Clean up our prefix.
    for i in 0..25 {
        let _ = op.delete(&format!("docs/n{i:02}.md")).await;
    }
    for k in ["one.bin", "big.bin"] {
        let _ = op.delete(k).await;
    }
    if let Some(path) = env("OO_CLOUD_EVIDENCE") {
        std::fs::write(path, serde_json::to_string_pretty(&seen).unwrap() + "\n").unwrap();
    }
    println!("{seen}");
    let _ = DocCapability::List;
}
