//! Remote originals: ranged download into the CAS with `If-Match`, restart on
//! a change, flat memory, and the cache (B-S4-03, 10 §4.3.3).
//!
//! `OO_S3_ENDPOINT=http://127.0.0.1:<port> OO_S3_ACCESS=.. OO_S3_SECRET=..
//!  OO_S3_BUCKET=<bucket> cargo test -p oo-docs --locked --features
//!  objectstore-s3 --test download -- --ignored --test-threads=1`
#![cfg(feature = "objectstore-s3")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use oo_connectors::objectstore_s3::opendal::{S3Config, S3Credentials, S3Source};
use oo_docs::cas::{Cas, CasKind};
use oo_docs::ingest::AnySource;
use oo_docs::ingest::download::{fetch_original, fetch_original_observed};

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"))
}

fn source(prefix: &str) -> (S3Source, AnySource) {
    let cfg = S3Config {
        bucket: env("OO_S3_BUCKET"),
        prefix: format!("{prefix}/"),
        endpoint: Some(env("OO_S3_ENDPOINT")),
        region: "us-east-1".into(),
        path_style: true,
        excludes: vec![],
        credentials: S3Credentials::new(&env("OO_S3_ACCESS"), &env("OO_S3_SECRET"), None).unwrap(),
    };
    let s = S3Source::open(&cfg).unwrap();
    (s.clone(), AnySource::S3(s))
}

fn body(n: usize, salt: u8) -> Vec<u8> {
    let mut x = 0x9e37_79b9u32 ^ u32::from(salt);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

fn rss_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

fn cas_in(name: &str) -> (Arc<Cas>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("oo-dl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (Arc::new(Cas::open(&dir).unwrap()), dir)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn a_200_mib_original_streams_into_the_cas_in_under_64_mib() {
    let (s3, any) = source("t-dl200");
    const SIZE: usize = 200 * 1024 * 1024;
    {
        // Upload in pieces so the test's own footprint stays small.
        let mut w = s3
            .operator_for_tests()
            .writer_with("big.bin")
            .chunk(8 * 1024 * 1024)
            .await
            .unwrap();
        let piece = body(8 * 1024 * 1024, 1);
        for _ in 0..(SIZE / piece.len()) {
            w.write(piece.clone()).await.unwrap();
        }
        w.close().await.unwrap();
    }
    let (cas, dir) = cas_in("200");
    let base = rss_kib();
    let peak = Arc::new(AtomicU64::new(base));
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (peak, stop) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                peak.fetch_max(rss_kib(), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        })
    };
    let t0 = std::time::Instant::now();
    let got = fetch_original(&any, &cas, b"big.bin", None).await.unwrap();
    let secs = t0.elapsed().as_secs_f64();
    stop.store(true, Ordering::Relaxed);
    sampler.join().unwrap();
    let growth_mib = (peak.load(Ordering::Relaxed).saturating_sub(base)) as f64 / 1024.0;
    eprintln!(
        "DOWNLOAD 200 MiB in {secs:.1}s, RSS growth {growth_mib:.1} MiB (base {} MiB)",
        base / 1024
    );
    assert_eq!(got.put.size, SIZE as u64);
    assert!(!got.from_cache && got.restarts == 0);
    assert!(growth_mib < 64.0, "peak memory growth {growth_mib:.1} MiB");
    assert!(cas.exists(CasKind::Orig, &got.put.b3).unwrap());
    cas.verify(CasKind::Orig, &got.put.b3).unwrap();
    // The second fetch with the same hash is the cache: no network.
    let again = fetch_original(&any, &cas, b"nothing-there.bin", Some(&got.put.b3))
        .await
        .unwrap();
    assert!(again.from_cache);
    assert_eq!(again.put.size, SIZE as u64);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn an_object_changed_during_the_download_is_downloaded_again() {
    let (s3, any) = source("t-dlmid");
    const SIZE: usize = 40 * 1024 * 1024;
    s3.operator_for_tests()
        .write("a.bin", body(SIZE, 1))
        .await
        .unwrap();
    let (cas, dir) = cas_in("mid");
    let handle = tokio::runtime::Handle::current();
    let writer = s3.clone();
    let mut done = false;
    let got = fetch_original_observed(&any, &cas, b"a.bin", None, |received| {
        if !done && received >= 8 * 1024 * 1024 {
            done = true;
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    writer
                        .operator_for_tests()
                        .write("a.bin", body(SIZE, 2))
                        .await
                        .unwrap();
                });
            });
        }
    })
    .await
    .unwrap();
    assert_eq!(got.restarts, 1, "one restart, not a failure");
    assert_eq!(
        got.put.b3,
        blake3::hash(&body(SIZE, 2)).to_hex().to_string()
    );
    // Only the object that was complete and consistent is in the CAS.
    let orig_files = walkdir_count(&dir.join("orig"));
    assert_eq!(
        orig_files, 1,
        "the partial first download left nothing behind"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a RustFS or MinIO (OO_S3_*)"]
async fn a_download_that_is_not_the_wanted_version_is_changed() {
    let (s3, any) = source("t-dlwant");
    s3.operator_for_tests()
        .write("a.bin", body(1000, 1))
        .await
        .unwrap();
    let (cas, dir) = cas_in("want");
    let other = blake3::hash(&body(1000, 7)).to_hex().to_string();
    let err = fetch_original(&any, &cas, b"a.bin", Some(&other))
        .await
        .unwrap_err();
    assert_eq!(err, oo_connectors::objectstore::DocSourceError::Changed);
    let _ = std::fs::remove_dir_all(dir);
}

fn walkdir_count(p: &std::path::Path) -> usize {
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.is_dir() {
            n += walkdir_count(&path);
        } else {
            n += 1;
        }
    }
    n
}
