//! Registering an object-store Drive (docs-api 1.10): `s3`, `oss`, `cos` and `r2`, each with the contract's
//! two references (`access_key_id`, `secret_access_key`) and with the single `secret_ref` of a JSON credential,
//! against a real RustFS. The registered config must give a working source (the credentials resolve and sign).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> OO_S3_ENDPOINT=http://127.0.0.1:<port> OO_S3_ACCESS=.. OO_S3_SECRET=..
//!  OO_S3_BUCKET=<bucket> cargo test -p oo-docs --locked --features objectstore-s3 --test drive_s3_family_live
//!  -- --ignored --test-threads=1`
#![cfg(feature = "objectstore-s3")]

mod common;

use std::path::PathBuf;

use common::engine::{Opts, TestEngine};
use oo_connectors::objectstore_s3::opendal::{S3Config, S3Source, SecretResolver};
use oo_docs::api::FactorySyncPort;
use oo_docs::ingest::{AnyFactory, IngestLimits};
use serde_json::{Value, json};

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"))
}

fn secret_file(dir: &std::path::Path, name: &str, text: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    format!("file:{}", p.display())
}

/// Registers, reads back, builds the source from the stored config and writes and reads one object.
async fn register_and_use(e: &TestEngine, backend: &str, shape: &str, config: Value) {
    let name = format!("{backend}-{shape}");
    let r = e
        .post(
            "/v1/docs/drives",
            &json!({"name": name, "backend": backend, "config": config}),
        )
        .await;
    assert_eq!(
        r.status,
        201,
        "{name}: {}",
        String::from_utf8_lossy(&r.body)
    );
    let drive = r.json();
    let id = drive["id"].as_str().unwrap();
    let shown = e.get(&format!("/v1/docs/drives/{id}")).await.json();
    // References come back as given; a key never does.
    let text = shown["config"].to_string();
    assert!(
        !text.contains(&env("OO_S3_SECRET")),
        "{name}: secret in the answer"
    );
    assert_eq!(shown["config"]["bucket"], env("OO_S3_BUCKET"));

    let resolver = SecretResolver {
        secrets_dir: PathBuf::from("/nonexistent"),
    };
    let cfg = S3Config::from_drive_config_for(backend, &shown["config"], &resolver)
        .unwrap_or_else(|e| panic!("{name}: stored config does not build: {e:?}"));
    let src = S3Source::open(&cfg).unwrap();
    let key = format!("h1-{name}/hello.txt");
    let op = src.operator_for_tests();
    op.write(&key, b"hello".to_vec())
        .await
        .unwrap_or_else(|e| panic!("{name}: credentials do not work: {e}"));
    assert_eq!(op.read(&key).await.unwrap().to_vec(), b"hello");
    op.delete(&key).await.unwrap();
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT and a RustFS (OO_S3_*)"]
async fn every_s3_family_backend_registers_with_the_pair_and_with_one_reference() {
    // Registration opens the source (as the engine binary does), so the credentials are really used.
    let tweak: common::engine::Tweak = Box::new(|app| {
        app.sync = std::sync::Arc::new(FactorySyncPort::new(
            app.pool.clone(),
            AnyFactory {
                secrets_dir: PathBuf::from("/nonexistent"),
            },
            IngestLimits::default(),
        ));
    });
    let e = TestEngine::start(
        "drive_s3_family",
        Opts {
            tweak: Some(tweak),
            ..Opts::default()
        },
    )
    .await;
    let dir = std::env::temp_dir().join(format!("oo-h1-secrets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (access, secret) = (env("OO_S3_ACCESS"), env("OO_S3_SECRET"));
    let single = secret_file(
        &dir,
        "cred.json",
        &json!({"access_key_id": access, "secret_access_key": secret}).to_string(),
    );
    // A trailing newline in a plain value file is not part of the value.
    let id_ref = secret_file(&dir, "key-id", &format!("{access}\n"));
    let sk_ref = secret_file(&dir, "key-secret", &format!("{secret}\n"));
    let base = |backend: &str| {
        let mut c = json!({
            "bucket": env("OO_S3_BUCKET"),
            "endpoint": env("OO_S3_ENDPOINT"),
            "region": "us-east-1",
            "path_style": true,
        });
        match backend {
            "r2" => c["account_id"] = json!("0123456789abcdef0123456789abcdef"),
            "oss" | "cos" => c["internal"] = json!(false),
            _ => {}
        }
        c
    };
    for backend in ["s3", "oss", "cos", "r2"] {
        let mut pair = base(backend);
        pair["access_key_id"] = json!({"secret_ref": id_ref});
        pair["secret_access_key"] = json!({"secret_ref": sk_ref});
        register_and_use(&e, backend, "pair", pair).await;

        let mut one = base(backend);
        one["secret_ref"] = json!(single);
        register_and_use(&e, backend, "single", one).await;
    }

    // What the engine must still refuse.
    let refuse = |what: &'static str, mut c: Value, edit: Box<dyn Fn(&mut Value)>| {
        edit(&mut c);
        (what, c)
    };
    let cases = [
        refuse(
            "both shapes",
            base("s3"),
            Box::new(|c| {
                c["secret_ref"] = json!(single);
                c["access_key_id"] = json!({"secret_ref": id_ref});
                c["secret_access_key"] = json!({"secret_ref": sk_ref});
            }),
        ),
        refuse(
            "only the key id",
            base("s3"),
            Box::new(|c| c["access_key_id"] = json!({"secret_ref": id_ref})),
        ),
        refuse(
            "an inline key",
            base("s3"),
            Box::new(|c| {
                c["access_key_id"] = json!("AKIAINLINE");
                c["secret_access_key"] = json!("inline-secret");
            }),
        ),
        refuse(
            "a bad reference",
            base("s3"),
            Box::new(|c| {
                c["access_key_id"] = json!({"secret_ref": "relative/path"});
                c["secret_access_key"] = json!({"secret_ref": sk_ref});
            }),
        ),
        refuse("no credentials", base("s3"), Box::new(|_| {})),
    ];
    for (what, config) in cases {
        let r = e
            .post(
                "/v1/docs/drives",
                &json!({"name": format!("bad-{}", what.replace(' ', "-")), "backend": "s3", "config": config}),
            )
            .await;
        assert_eq!(
            r.status,
            400,
            "{what}: {}",
            String::from_utf8_lossy(&r.body)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    e.finish().await;
}
