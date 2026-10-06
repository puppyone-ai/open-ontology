#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pg_onboarding::{apply_reviewed, bind_generation, config_identity_hex, inspect, sha256_hex};
use serde_json::Value;

const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a261";
const SCHEMA_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CAPTURED_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const REVIEWED_HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const SEALED_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PACK_CLERK: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const MANIFEST_CLERK: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const PACK_PEEK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const MANIFEST_PEEK: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const TABLE_DIGEST: &str = "3333333333333333333333333333333333333333333333333333333333333333";

fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn pt(fill: &str) -> String {
    format!("pt_{fill}")
}

fn pc(fill: &str) -> String {
    format!("pc_{fill}")
}

#[test]
fn sha256_and_generation_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let preimage = "oo-pg-config-identity/v1\nsource_id=a261src\nhost=oo-a261-pg\nport=5432\ndatabase=oo_a261_src\nusername=oo_a261_reader\nsecret_ref=secret://pg/a261/reader\nconnect_ms=30000\nstatement_ms=60000\nlock_ms=30000\nmax_rows=10000\nmax_bytes=8388608\ntable\tshop\tcustomer\ntable\tshop\torder_doc\n";
    assert_eq!(sha256_hex(preimage.as_bytes()), "a4a889fba1436935fb1fe1af9c30a8a79109794d94600fa4ce9d2e1b21f7b618");
    assert_eq!(
        config_identity_hex(
            "a261src",
            "oo-a261-pg",
            5432,
            "oo_a261_src",
            "oo_a261_reader",
            "secret://pg/a261/reader",
            &[("shop", "customer"), ("shop", "order_doc")]
        ),
        "a4a889fba1436935fb1fe1af9c30a8a79109794d94600fa4ce9d2e1b21f7b618"
    );
    assert_eq!(bind_generation(48, 1).unwrap(), 48);
    assert!(bind_generation(1, 1).is_err());
    assert!(bind_generation(0, 1).is_err());
}

#[test]
fn refuses_secret_symlink_and_existing_run() {
    let root = scratch("refuse");
    let bins = root.join("bin");
    fs::create_dir(&bins).unwrap();
    let real = bins.join("init-pg");
    fs::write(&real, b"not-executed").unwrap();
    let hash = sha256_hex(&fs::read(&real).unwrap());
    let link = bins.join("linked");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let secrets = root.join("secrets");
    fs::create_dir(&secrets).unwrap();
    fs::write(secrets.join("reader.pass"), b"s3cret-value-aaa\n").unwrap();
    fs::write(secrets.join("admin.pass"), b"admin-value-bbbb\n").unwrap();
    fs::write(secrets.join("runtime.pass"), b"runtime-value-ccc\n").unwrap();
    let run = root.join("run");
    let cfg = root.join("inspect.json");
    fs::write(
        &cfg,
        inspect_config(&root, &hash, &real, "s3cret-value-aaa"),
    )
    .unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.detail, "secret");
    assert!(!run.exists());

    fs::write(&cfg, inspect_config(&root, &hash, &link, "a261src")).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.detail, "symlink");
    assert!(!run.exists());

    fs::create_dir(&run).unwrap();
    fs::write(&cfg, inspect_config(&root, &hash, &real, "a261src")).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.detail, "run_dir");
}

#[test]
fn fake_compilers_cover_both_phases() {
    let root = scratch("chain");
    let child = compile_child(&root);
    let hash = sha256_hex(&fs::read(&child).unwrap());
    let digest = config_identity_hex(
        "a261src",
        "oo-a261-pg",
        5432,
        "oo_a261_src",
        "oo_a261_reader",
        "secret://pg/a261/reader",
        &[("shop", "customer"), ("shop", "order_doc")],
    );

    let deadline = root.join("deadline");
    install_bins(&child, &deadline);
    fs::write(deadline.join("bin").join("sleep-init"), b"1").unwrap();
    write_canned(&deadline.join("bin").join("canned"), &digest);
    let secrets = write_secrets(&deadline);
    let run = deadline.join("run");
    let cfg = deadline.join("inspect.json");
    fs::write(&cfg, inspect_config_bins(&deadline, &hash, &secrets, true, Some(200))).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "init-pg");
    assert_eq!(err.detail, "deadline");
    let outcome: Value = serde_json::from_slice(&fs::read(run.join("outcome.json")).unwrap()).unwrap();
    assert_eq!(outcome["status"], "failed");
    assert_eq!(outcome["database_state"], "unchanged_by_this_tool");
    assert_eq!(fs::read_to_string(run.join("steps").join("init-pg.exit")).unwrap(), "timeout\n");
    assert!(run.join("steps").join("import-not-ready.stderr").is_file());

    let broken = root.join("broken");
    install_bins(&child, &broken);
    fs::write(broken.join("bin").join("fail-catalog"), b"1").unwrap();
    write_canned(&broken.join("bin").join("canned"), &digest);
    let secrets = write_secrets(&broken);
    let run = broken.join("run");
    let cfg = broken.join("inspect.json");
    fs::write(&cfg, inspect_config_bins(&broken, &hash, &secrets, false, None)).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "catalog-pg");
    assert!(run.join("steps").join("import-pg.stdout").is_file());
    assert!(!run.join("captured-draft.json").exists());
    let outcome: Value = serde_json::from_slice(&fs::read(run.join("outcome.json")).unwrap()).unwrap();
    assert_eq!(outcome["status"], "failed");

    let ok = root.join("ok");
    install_bins(&child, &ok);
    write_canned(&ok.join("bin").join("canned"), &digest);
    let secrets = write_secrets(&ok);
    fs::write(ok.join("membership.ready"), b"ready\n").unwrap();
    let run = ok.join("run");
    let cfg = ok.join("inspect.json");
    fs::write(&cfg, inspect_config_bins(&ok, &hash, &secrets, true, None)).unwrap();
    inspect(&cfg, &run).unwrap();
    let manifest = fs::read(run.join("inspect-manifest.json")).unwrap();
    let sidecar = fs::read_to_string(run.join("inspect-manifest.json.sha256")).unwrap();
    assert_eq!(sidecar.trim(), sha256_hex(&manifest));
    let manifest: Value = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(manifest["mapping_run_generation"], 48);
    assert_eq!(manifest["source_cut"], 48);
    assert_eq!(manifest["target_fence"], 1);
    assert_eq!(manifest["config_identity_sha256"], digest);
    assert_eq!(manifest["non_claims"]["mapping_rows_read"], false);
    assert!(!manifest.to_string().contains("pw-reader-a261-orange"));
    let outcome: Value = serde_json::from_slice(&fs::read(run.join("outcome.json")).unwrap()).unwrap();
    assert_eq!(outcome["status"], "awaiting_review");

    let saved = manifest_bytes(&run);
    let mut flipped = saved.clone();
    flipped[20] ^= 0x01;
    fs::write(run.join("inspect-manifest.json"), &flipped).unwrap();
    write_operator(&ok, &hash);
    let apply_cfg = ok.join("apply.json");
    let err = apply_reviewed(&apply_cfg, &run).unwrap_err();
    assert_eq!(err.detail, "manifest");
    assert!(!run.join("apply-outcome.json").exists());
    fs::write(run.join("inspect-manifest.json"), &saved).unwrap();

    apply_reviewed(&apply_cfg, &run).unwrap();
    let applied: Value = serde_json::from_slice(&fs::read(run.join("apply-outcome.json")).unwrap()).unwrap();
    assert_eq!(applied["status"], "ready_for_trusted_query");
    assert_eq!(applied["mapping_run_generation"], 48);
    assert_eq!(applied["analyze_published"], false);
    assert_eq!(applied["non_claims"]["parent_acceptance"], false);
    assert_eq!(applied["non_claims"]["semantic_publication"], "missing");
    assert_eq!(applied["non_claims"]["mapping_rows_read"], false);
    assert_eq!(applied["hidden"][0]["reason"], "classification.absent");
    assert_eq!(applied["provision"][0]["effect"], "installed");
    assert_eq!(applied["provision"][1]["effect"], "replay");
    assert_eq!(fs::read_to_string(run.join("steps").join("query-clerk-before.exit")).unwrap(), "5\n");
    assert_eq!(fs::read_to_string(run.join("steps").join("query-clerk-hidden.exit")).unwrap(), "6\n");
    assert_eq!(fs::read_to_string(run.join("steps").join("query-clerk-ok.exit")).unwrap(), "0\n");
    assert!(run.join("bundles").join("clerk.json").is_file());
    assert!(run.join("bundles").join("peek.json").is_file());
    assert!(run.join("captured-draft.json").is_file());
    let again = apply_reviewed(&apply_cfg, &run).unwrap_err();
    assert_eq!(again.detail, "terminal");
    assert!(run.join("captured-draft.json").is_file());
}

fn manifest_bytes(run: &Path) -> Vec<u8> {
    fs::read(run.join("inspect-manifest.json")).unwrap()
}

fn compile_child(root: &Path) -> PathBuf {
    let source = root.join("child.rs");
    fs::write(&source, CHILD).unwrap();
    let output = root.join("child-bin");
    let status = Command::new("rustc")
        .arg("--edition")
        .arg("2024")
        .arg(&source)
        .arg("-o")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    output
}

fn install_bins(child: &Path, dir: &Path) {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for name in [
        "init-pg",
        "oo-pipeline",
        "oo-builder",
        "reviewed-effective-oos",
        "reviewed-policy",
        "install-column-acl",
        "provision-mapping-reader",
        "analyze-pg",
        "role-pack-bundle",
        "query-pg",
    ] {
        fs::copy(child, bin.join(name)).unwrap();
    }
}

fn write_secrets(dir: &Path) -> PathBuf {
    let secrets = dir.join("secrets");
    fs::create_dir_all(&secrets).unwrap();
    fs::write(secrets.join("reader.pass"), b"pw-reader-a261-orange\n").unwrap();
    fs::write(secrets.join("admin.pass"), b"pw-admin-a261-copper\n").unwrap();
    fs::write(secrets.join("runtime.pass"), b"pw-runtime-a261-birch\n").unwrap();
    fs::write(secrets.join("login.pass"), b"pw-login-a261-maple\n").unwrap();
    fs::write(secrets.join("ca.crt"), b"not-a-secret-ca\n").unwrap();
    secrets
}

fn fill(byte: char) -> String {
    byte.to_string().repeat(60)
}

fn write_canned(dir: &Path, digest: &str) {
    fs::create_dir_all(dir).unwrap();
    let customer_pt = pt(&fill('a'));
    let order_pt = pt(&fill('b'));
    let customer_id = pc(&fill('c'));
    let display = pc(&fill('d'));
    let order_id = pc(&fill('e'));
    fs::write(
        dir.join("init.stdout"),
        b"{\"full_import\":\"installed\",\"historical_v2\":false,\"install\":\"bootstrapped\",\"protocol\":\"oo.pipeline.init-pg-result/v1\"}\n",
    )
    .unwrap();
    let import = format!(
        "{{\"protocol\":\"oo.pipeline.import-pg-result/v1\",\"run_id\":\"{OWNER}\",\"source_generation\":48,\"state\":\"awaiting_publish\",\"table_count\":2,\"tables\":[{{\"canonical_digest\":\"{TABLE_DIGEST}\",\"columns\":[{{\"name\":\"customer_id\",\"physical_name\":\"{customer_id}\"}},{{\"name\":\"display_name\",\"physical_name\":\"{display}\"}}],\"name\":\"customer\",\"physical_name\":\"{customer_pt}\",\"rows\":3,\"schema\":\"shop\"}},{{\"canonical_digest\":\"{TABLE_DIGEST}\",\"columns\":[{{\"name\":\"order_id\",\"physical_name\":\"{order_id}\"}}],\"name\":\"order_doc\",\"physical_name\":\"{order_pt}\",\"rows\":3,\"schema\":\"shop\"}}],\"target_generation\":1}}\n"
    );
    fs::write(dir.join("import-ok.stdout"), import).unwrap();
    fs::write(dir.join("catalog.stdout"), b"{\"protocol\":\"oo.source.pgsql/v1\"}").unwrap();
    fs::write(
        dir.join("captured.stdout"),
        format!(
            "{{\"protocol\":\"oo.builder.captured-pg-draft-command/v1\",\"wrapper\":{{\"draft_hash\":\"{CAPTURED_HASH}\",\"schema_hash\":\"{SCHEMA_HASH}\"}}}}"
        ),
    )
    .unwrap();
    fs::write(
        dir.join("review.stdout"),
        format!(
            "{{\"protocol\":\"oo.builder.captured-pg-draft-command/v1\",\"wrapper\":{{\"draft_hash\":\"{REVIEWED_HASH}\",\"schema_hash\":\"{SCHEMA_HASH}\"}}}}"
        ),
    )
    .unwrap();
    fs::write(
        dir.join("effective.stdout"),
        format!(
            "{{\"effective_oos\":{{\"catalog_version\":\"cat-a261\",\"knowledge\":{{\"id\":\"know-a261\",\"version\":\"1\"}},\"ontology_version\":\"ont-a261\",\"policy\":{{\"id\":\"pol-a261\",\"version\":\"1\"}},\"project\":\"a261src\"}},\"hidden\":[{{\"column\":\"private_note\",\"dataset\":\"shop.customer\",\"reason\":\"classification.absent\"}}],\"protocol\":\"oo.builder.reviewed-effective-oos-command/v1\",\"provenance\":{{\"authentication\":\"not_claimed\",\"authorization\":\"not_claimed\",\"grants\":\"not_claimed\",\"publication\":\"not_claimed\",\"schema_hash\":\"{SCHEMA_HASH}\",\"sealed_source_sha256\":\"{SEALED_HASH}\",\"semantic_publication\":\"not_claimed\"}}}}"
        ),
    )
    .unwrap();
    fs::write(dir.join("policy-clerk.stdout"), policy_stdout("clerk", digest, &customer_pt, &order_pt, &customer_id, &order_id)).unwrap();
    fs::write(dir.join("policy-peek.stdout"), policy_stdout("peek", digest, &customer_pt, &order_pt, &display, &order_id)).unwrap();
    fs::write(
        dir.join("install-clerk.stdout"),
        install_stdout("clerk", &customer_pt, &order_pt, &customer_id, &order_id, true),
    )
    .unwrap();
    fs::write(
        dir.join("install-peek.stdout"),
        install_stdout("peek", &customer_pt, &order_pt, &display, &order_id, false),
    )
    .unwrap();
    fs::write(
        dir.join("provision-clerk.stdout"),
        b"{\"effect\":\"installed\",\"login\":\"oo_a261_login\",\"protocol\":\"oo.gateway.provision-mapping-reader-cli/v1\",\"role\":\"oo_r_clerk\"}\n",
    )
    .unwrap();
    fs::write(
        dir.join("provision-peek.stdout"),
        b"{\"effect\":\"replay\",\"login\":\"oo_a261_login\",\"protocol\":\"oo.gateway.provision-mapping-reader-cli/v1\",\"role\":\"oo_r_peek\"}\n",
    )
    .unwrap();
    fs::write(
        dir.join("analyze.stdout"),
        format!(
            "{{\"config_digest\":\"{digest}\",\"estimate_is_exact\":false,\"protocol\":\"oo.pipeline.analyze-pg-result/v1\",\"published\":false,\"source_generation\":48}}\n"
        ),
    )
    .unwrap();
    fs::write(dir.join("role-pack-clerk.stdout"), pack_stdout(PACK_CLERK, MANIFEST_CLERK)).unwrap();
    fs::write(dir.join("role-pack-peek.stdout"), pack_stdout(PACK_PEEK, MANIFEST_PEEK)).unwrap();
    fs::write(dir.join("bundle-clerk.json"), bundle_body(PACK_CLERK, MANIFEST_CLERK)).unwrap();
    fs::write(dir.join("bundle-peek.json"), bundle_body(PACK_PEEK, MANIFEST_PEEK)).unwrap();
}

fn policy_stdout(
    role: &str,
    digest: &str,
    _customer_pt: &str,
    _order_pt: &str,
    _first_col: &str,
    _order_col: &str,
) -> String {
    let customer_cols = if role == "clerk" {
        r#"{"name":"customer_id","pii":{"class":"none"},"semantic_type":"identifier","status":"confirmed"}"#.to_string()
    } else {
        r#"{"name":"display_name","pii":{"class":"declared","explicit_allow":true},"semantic_type":"person_name","status":"confirmed"}"#.to_string()
    };
    let order_cols = r#"{"name":"order_id","pii":{"class":"none"},"semantic_type":"identifier","status":"confirmed"}"#;
    let customer = format!(
        r#"{{"config_digest":"{digest}","generation":48,"login":"oo_a261_login","project":"a261src","role":"{role}","row_filter":null,"table":{{"columns":[{customer_cols}],"name":"customer","namespace":"shop"}}}}"#
    );
    if role == "peek" {
        return format!(
            r#"{{"protocol":"oo.gateway.reviewed-policy-cli/v1","policies":[{customer}]}}"#
        );
    }
    let order = format!(
        r#"{{"config_digest":"{digest}","generation":48,"login":"oo_a261_login","project":"a261src","role":"{role}","row_filter":null,"table":{{"columns":[{order_cols}],"name":"order_doc","namespace":"shop"}}}}"#
    );
    format!(r#"{{"protocol":"oo.gateway.reviewed-policy-cli/v1","policies":[{customer},{order}]}}"#)
}

fn install_stdout(role: &str, customer_pt: &str, order_pt: &str, customer_col: &str, order_col: &str, with_order: bool) -> String {
    let customer = format!(
        r#"{{"physical_columns":["{customer_col}"],"physical_table":"{customer_pt}"}}"#
    );
    let tables = if with_order {
        format!(
            "{customer},{{\"physical_columns\":[\"{order_col}\"],\"physical_table\":\"{order_pt}\"}}"
        )
    } else {
        customer
    };
    format!(
        "{{\"effect\":\"installed\",\"grantor\":\"oo_fixture_admin\",\"protocol\":\"oo.gateway.install-column-acl-cli/v1\",\"role\":\"oo_r_{role}\",\"tables\":[{tables}]}}\n"
    )
}

fn pack_stdout(pack: &str, manifest: &str) -> String {
    format!(
        "{{\"manifest\":{{\"authentication\":\"not_claimed\",\"authorization\":\"not_claimed\",\"mount\":\"not_performed\",\"publication\":\"not_claimed\",\"row_security_reproven\":false,\"samples\":\"omitted\"}},\"manifest_sha256\":\"{manifest}\",\"pack_digest\":\"{pack}\",\"protocol\":\"oo.builder.role-pack-command/v1\"}}"
    )
}

fn bundle_body(pack: &str, manifest: &str) -> String {
    format!(
        "{{\"manifest_sha256\":\"{manifest}\",\"pack_digest\":\"{pack}\",\"protocol\":\"oo.builder.role-pack-bundle/v1\",\"statuses\":{{\"authentication\":\"not_claimed\",\"mount\":\"not_performed\",\"publication\":\"not_claimed\"}}}}\n"
    )
}

fn inspect_config(root: &Path, hash: &str, init: &Path, source_id: &str) -> String {
    let secrets = root.join("secrets");
    format!(
        r#"{{
  "protocol": "oo.onboarding.inspect/v1",
  "caller": "trusted_local_admin",
  "binaries": {{
    "init_pg": {{"path": "{init}", "sha256": "{hash}"}},
    "import_pg": {{"path": "{init}", "sha256": "{hash}"}},
    "catalog_pg": {{"path": "{init}", "sha256": "{hash}"}},
    "captured_pg_draft": {{"path": "{init}", "sha256": "{hash}"}}
  }},
  "source": {{
    "source_id": "{source_id}",
    "host": "oo-a261-pg",
    "port": 5432,
    "database": "oo_a261_src",
    "username": "oo_a261_reader",
    "secret_ref": "secret://pg/a261/reader",
    "password_file": "{reader}",
    "ca_file": "{ca}",
    "tables": [
      {{"schema": "shop", "table": "customer"}},
      {{"schema": "shop", "table": "order_doc"}}
    ]
  }},
  "target": {{
    "host": "oo-a261-pg",
    "port": 5432,
    "database": "oo_a261_dst",
    "administrator": "oo_fixture_admin",
    "runtime_principal": "oo_a261_runtime",
    "admin_password_file": "{admin}",
    "runtime_password_file": "{runtime}",
    "ca_file": "{ca}"
  }},
  "shard": "a261main",
  "owner": "{OWNER}",
  "source_generation": 48,
  "connect_ms": 30000
}}"#,
        init = init.display(),
        reader = secrets.join("reader.pass").display(),
        admin = secrets.join("admin.pass").display(),
        runtime = secrets.join("runtime.pass").display(),
        ca = secrets.join("ca.crt").display(),
    )
}

fn inspect_config_bins(dir: &Path, hash: &str, secrets: &Path, membership: bool, init_ms: Option<u64>) -> String {
    let bin = dir.join("bin");
    let ready = if membership {
        format!(",\n  \"runtime_membership_ready_file\": \"{}\"", dir.join("membership.ready").display())
    } else {
        String::new()
    };
    let deadlines = if let Some(ms) = init_ms {
        format!(",\n  \"deadlines_ms\": {{\"init_ms\": {ms}}}")
    } else {
        String::new()
    };
    format!(
        r#"{{
  "protocol": "oo.onboarding.inspect/v1",
  "caller": "trusted_local_admin",
  "binaries": {{
    "init_pg": {{"path": "{init}", "sha256": "{hash}"}},
    "import_pg": {{"path": "{pipeline}", "sha256": "{hash}"}},
    "catalog_pg": {{"path": "{pipeline}", "sha256": "{hash}"}},
    "captured_pg_draft": {{"path": "{builder}", "sha256": "{hash}"}}
  }},
  "source": {{
    "source_id": "a261src",
    "host": "oo-a261-pg",
    "port": 5432,
    "database": "oo_a261_src",
    "username": "oo_a261_reader",
    "secret_ref": "secret://pg/a261/reader",
    "password_file": "{reader}",
    "ca_file": "{ca}",
    "tables": [
      {{"schema": "shop", "table": "customer"}},
      {{"schema": "shop", "table": "order_doc"}}
    ]
  }},
  "target": {{
    "host": "oo-a261-pg",
    "port": 5432,
    "database": "oo_a261_dst",
    "administrator": "oo_fixture_admin",
    "runtime_principal": "oo_a261_runtime",
    "admin_password_file": "{admin}",
    "runtime_password_file": "{runtime}",
    "ca_file": "{ca}"
  }},
  "shard": "a261main",
  "owner": "{OWNER}",
  "source_generation": 48,
  "connect_ms": 30000{ready}{deadlines}
}}"#,
        init = bin.join("init-pg").display(),
        pipeline = bin.join("oo-pipeline").display(),
        builder = bin.join("oo-builder").display(),
        reader = secrets.join("reader.pass").display(),
        admin = secrets.join("admin.pass").display(),
        runtime = secrets.join("runtime.pass").display(),
        ca = secrets.join("ca.crt").display(),
    )
}

fn write_operator(dir: &Path, hash: &str) {
    let bin = dir.join("bin");
    let secrets = dir.join("secrets");
    fs::write(dir.join("reviews.json"), b"[]\n").unwrap();
    fs::write(dir.join("decisions.json"), b"[]\n").unwrap();
    fs::write(
        dir.join("vis-clerk.json"),
        b"{\"protocol\":\"oo.builder.role-visibility/v1\",\"role\":\"oo_r_clerk\"}\n",
    )
    .unwrap();
    fs::write(
        dir.join("vis-peek.json"),
        b"{\"protocol\":\"oo.builder.role-visibility/v1\",\"role\":\"oo_r_peek\"}\n",
    )
    .unwrap();
    let body = format!(
        r#"{{
  "protocol": "oo.onboarding.apply-reviewed/v1",
  "caller": "trusted_local_admin",
  "binaries": {{
    "review_pg_draft": {{"path": "{builder}", "sha256": "{hash}"}},
    "reviewed_effective_oos": {{"path": "{effective}", "sha256": "{hash}"}},
    "reviewed_policy": {{"path": "{policy}", "sha256": "{hash}"}},
    "install_column_acl": {{"path": "{install}", "sha256": "{hash}"}},
    "provision_mapping_reader": {{"path": "{provision}", "sha256": "{hash}"}},
    "analyze_pg": {{"path": "{analyze}", "sha256": "{hash}"}},
    "role_pack": {{"path": "{builder}", "sha256": "{hash}"}},
    "role_pack_bundle": {{"path": "{bundle}", "sha256": "{hash}"}},
    "query_pg": {{"path": "{query}", "sha256": "{hash}"}}
  }},
  "reviews_file": "{reviews}",
  "decisions_file": "{decisions}",
  "settings": {{"project": "a261src"}},
  "schema": "shop",
  "query_login": {{"user": "oo_a261_login", "password_file": "{login}"}},
  "analyze": {{"statement_timeout_ms": 5000, "lock_timeout_ms": 5000, "connect_ms": 30000}},
  "roles": [
    {{
      "slug": "clerk",
      "allow": [
        {{"dataset": "shop.customer", "column": "customer_id", "explicit_allow": false}},
        {{"dataset": "shop.order_doc", "column": "order_id", "explicit_allow": false}}
      ],
      "visibility_file": "{vis_clerk}",
      "queries": [
        {{
          "name": "clerk-before",
          "when": "before_analyze",
          "expected_exit": 5,
          "expected_stderr": "query-pg: policy",
          "statement": {{"source": "request", "sql": "SELECT BEFORE", "max_rows": 20, "max_result_bytes": 8192, "wall_clock_ms": 30000}}
        }},
        {{
          "name": "clerk-ok",
          "when": "after_bundle",
          "expected_exit": 0,
          "statement": {{"source": "request", "sql": "SELECT ok", "max_rows": 20, "max_result_bytes": 8192, "wall_clock_ms": 30000}}
        }},
        {{
          "name": "clerk-hidden",
          "when": "after_bundle",
          "expected_exit": 6,
          "expected_stderr": "query-pg: guard",
          "statement": {{"source": "request", "sql": "SELECT HIDDEN", "max_rows": 20, "max_result_bytes": 8192, "wall_clock_ms": 30000}}
        }}
      ]
    }},
    {{
      "slug": "peek",
      "allow": [
        {{"dataset": "shop.customer", "column": "display_name", "explicit_allow": true}}
      ],
      "visibility_file": "{vis_peek}",
      "queries": [
        {{
          "name": "peek-ok",
          "when": "after_bundle",
          "expected_exit": 0,
          "statement": {{"source": "request", "sql": "SELECT ok", "max_rows": 20, "max_result_bytes": 8192, "wall_clock_ms": 30000}}
        }}
      ]
    }}
  ]
}}"#,
        builder = bin.join("oo-builder").display(),
        effective = bin.join("reviewed-effective-oos").display(),
        policy = bin.join("reviewed-policy").display(),
        install = bin.join("install-column-acl").display(),
        provision = bin.join("provision-mapping-reader").display(),
        analyze = bin.join("analyze-pg").display(),
        bundle = bin.join("role-pack-bundle").display(),
        query = bin.join("query-pg").display(),
        reviews = dir.join("reviews.json").display(),
        decisions = dir.join("decisions.json").display(),
        login = secrets.join("login.pass").display(),
        vis_clerk = dir.join("vis-clerk.json").display(),
        vis_peek = dir.join("vis-peek.json").display(),
    );
    fs::write(dir.join("apply.json"), body).unwrap();
}

const CHILD: &str = r#"
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    let exe = env::current_exe().unwrap();
    let name = exe.file_name().unwrap().to_string_lossy().to_string();
    let dir = exe.parent().unwrap().to_path_buf();
    if name == "init-pg" && dir.join("sleep-init").is_file() {
        thread::sleep(Duration::from_secs(5));
    }
    if name == "oo-pipeline" && env::args().nth(1).as_deref() == Some("catalog-pg") && dir.join("fail-catalog").is_file() {
        return ExitCode::from(1);
    }
    let mut stdin = Vec::new();
    let _ = io::stdin().read_to_end(&mut stdin);
    let args: Vec<String> = env::args().skip(1).collect();
    let canned = dir.join("canned");
    match name.as_str() {
        "init-pg" => copy_out(&canned.join("init.stdout"), 0),
        "oo-pipeline" => pipeline(&dir, &canned, &args),
        "oo-builder" => builder(&canned, &args, &stdin),
        "reviewed-effective-oos" => copy_out(&canned.join("effective.stdout"), 0),
        "reviewed-policy" => {
            let path = if stdin.windows(b"\"role\":\"clerk\"".len()).any(|w| w == b"\"role\":\"clerk\"") {
                canned.join("policy-clerk.stdout")
            } else {
                canned.join("policy-peek.stdout")
            };
            copy_out(&path, 0)
        }
        "install-column-acl" => {
            let path = if stdin.windows(b"\"role\":\"clerk\"".len()).any(|w| w == b"\"role\":\"clerk\"") {
                canned.join("install-clerk.stdout")
            } else {
                canned.join("install-peek.stdout")
            };
            copy_out(&path, 0)
        }
        "provision-mapping-reader" => {
            let path = if stdin.windows(b"\"role\":\"clerk\"".len()).any(|w| w == b"\"role\":\"clerk\"") {
                canned.join("provision-clerk.stdout")
            } else {
                canned.join("provision-peek.stdout")
            };
            copy_out(&path, 0)
        }
        "analyze-pg" => copy_out(&canned.join("analyze.stdout"), 0),
        "role-pack-bundle" => bundle(&canned, &args),
        "query-pg" => query(&args),
        _ => ExitCode::from(2),
    }
}

fn one_arg(args: &[String], name: &str) -> bool {
    args.len() == 1 && args[0] == name
}

fn pipeline(dir: &std::path::Path, canned: &std::path::Path, args: &[String]) -> ExitCode {
    if one_arg(args, "import-pg") {
        let counter = dir.join("import.count");
        if !counter.exists() {
            fs::write(&counter, b"1").unwrap();
            let _ = io::stderr().write_all(b"target schema is not ready\n");
            return ExitCode::from(1);
        }
        return copy_out(&canned.join("import-ok.stdout"), 0);
    }
    if one_arg(args, "catalog-pg") {
        return copy_out(&canned.join("catalog.stdout"), 0);
    }
    ExitCode::from(2)
}

fn builder(canned: &std::path::Path, args: &[String], stdin: &[u8]) -> ExitCode {
    if one_arg(args, "captured-pg-draft") {
        return copy_out(&canned.join("captured.stdout"), 0);
    }
    if one_arg(args, "review-pg-draft") {
        return copy_out(&canned.join("review.stdout"), 0);
    }
    if one_arg(args, "role-pack") {
        let path = if stdin.windows(b"oo_r_clerk".len()).any(|w| w == b"oo_r_clerk") {
            canned.join("role-pack-clerk.stdout")
        } else {
            canned.join("role-pack-peek.stdout")
        };
        return copy_out(&path, 0);
    }
    ExitCode::from(2)
}

fn bundle(canned: &std::path::Path, args: &[String]) -> ExitCode {
    let mut request = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--request" && index + 1 < args.len() {
            request = Some(args[index + 1].as_str());
            index += 2;
        } else if args[index] == "--output" && index + 1 < args.len() {
            output = Some(args[index + 1].as_str());
            index += 2;
        } else {
            return ExitCode::from(2);
        }
    }
    let Some(request) = request else { return ExitCode::from(2); };
    let Some(output) = output else { return ExitCode::from(2); };
    let bytes = fs::read(request).unwrap_or_default();
    let src = if bytes.windows(b"oo_r_clerk".len()).any(|w| w == b"oo_r_clerk") {
        canned.join("bundle-clerk.json")
    } else {
        canned.join("bundle-peek.json")
    };
    fs::write(output, fs::read(src).unwrap()).unwrap();
    ExitCode::SUCCESS
}

fn query(args: &[String]) -> ExitCode {
    if args.len() != 2 || args[0] != "--request" {
        return ExitCode::from(2);
    }
    let text = fs::read_to_string(&args[1]).unwrap_or_default();
    if text.contains("HIDDEN") {
        let _ = io::stderr().write_all(b"query-pg: guard\n");
        return ExitCode::from(6);
    }
    if text.contains("BEFORE") {
        let _ = io::stderr().write_all(b"query-pg: policy\n");
        return ExitCode::from(5);
    }
    let _ = io::stdout().write_all(b"{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"1\"}]}\n");
    ExitCode::SUCCESS
}

fn copy_out(path: &std::path::Path, code: u8) -> ExitCode {
    let bytes = fs::read(path).unwrap_or_default();
    let _ = io::stdout().write_all(&bytes);
    ExitCode::from(code)
}
"#;
