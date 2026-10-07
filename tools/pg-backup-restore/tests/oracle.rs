//! Formula, manifest, and refusal oracles. They do not need PostgreSQL.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pg_backup_restore::cut::{self, RelationDigest, WriterBlock};
use pg_backup_restore::decide::{self, publish_offset, Report};
use pg_backup_restore::fixture::{self, control_sql, enum_sql};
use pg_backup_restore::gold::{self, entry_qty};
use pg_backup_restore::json;
use pg_backup_restore::manifest::{self, Manifest, NamedHash};
use pg_backup_restore::pg::read_secret;
use pg_backup_restore::protocol::{self, load_operator, schema_path, Operator};
use pg_backup_restore::sha256::{self, hash};
use pg_backup_restore::ops;

#[test]
fn sha256_matches_the_reviewed_vectors() {
    assert!(sha256::self_check());
    assert_eq!(sha256::hex(&hash(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256::hex(&hash(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn shop_lines_follow_the_public_fixture() {
    assert_eq!(gold::party_line(0), "p00000\t\\N\tt\ttel-00000\tnid-00000\n");
    assert_eq!(gold::account_line(0), "0\tp00000\tf\t\\N\n");
    assert_eq!(gold::entry_line(0), "0\t0\t-9223372036854775808\t\\N\t\\N\n");
    assert_eq!(gold::entry_line(3), "3\t3\t-44997\t1234567890.1234\tm3\n");
    assert_eq!(entry_qty(0), i64::MIN);
    assert_eq!(entry_qty(1), i64::MAX);
    assert_eq!(entry_qty(2), 0);
    assert_eq!(gold::format_amount(4).as_deref(), Some("-1234567890.1234"));
    assert_eq!(gold::format_amount(5).as_deref(), Some("0.0001"));
    assert_eq!(gold::format_amount(6).as_deref(), Some("-0.0001"));
    assert_eq!(gold::format_amount(29), None);
    assert_eq!(gold::null_amount_count(), 3104);
    assert_eq!(gold::entry_probe(3), "3|1234567890.1234|-44997");
    let bundle = gold::shop_bundle_sha256();
    assert_eq!(bundle.len(), 64);
}

#[test]
fn offset_is_never_the_minimum_and_history_withholds_it() {
    let kept = publish_offset(false, "100000", Some("1"));
    assert!(!kept.min_used);
    assert_eq!(kept.published.as_deref(), Some("100000"));
    let withheld = publish_offset(true, "100000", Some("999999"));
    assert!(!withheld.min_used);
    assert_eq!(withheld.published, None);
}

#[test]
fn issued_keys_are_reused_only_when_the_identity_repeats() {
    assert!(!decide::key_reused("k-0001|member-a\nk-0002|member-b\n"));
    assert!(decide::key_reused("k-0001|member-a\nk-0001|member-c\n"));
    assert!(decide::key_reused("k-0003|member-a\nk-0002|member-a\n"));
    assert_eq!(decide::window_seconds(10, 12), Some(2));
    assert_eq!(decide::window_seconds(12, 10), None);
}

#[test]
fn readiness_constants_stay_closed() {
    let mut report = Report {
        replay_ready: false,
        reason: String::new(),
        reasons: Vec::new(),
        history_insufficient: false,
        wal_moved: true,
        checkpoint_copy_ok: true,
        shop_gold_ok: true,
        counts_ok: true,
        cut_proven: true,
        decimal_ok: true,
        null_ok: true,
        comment_ok: true,
        key_reused: false,
        key_count: 2,
        post_cut_absent_on_restore: true,
        post_cut_present_on_source: true,
        clock_anomaly: false,
        offset_published: Some("100000".to_string()),
        rpo_window_seconds: Some(3),
        rto_seconds: Some(4),
    };
    report.seal();
    assert!(report.replay_ready);
    assert_eq!(report.reason, "ok");
    let body = decide::report_json(&report);
    let parsed = json::parse(&body).unwrap();
    assert!(!parsed.bool_key("all_state_certified").unwrap());
    assert!(!parsed.bool_key("parent_acceptance").unwrap());
    assert!(!parsed.bool_key("production_ops").unwrap());
    assert!(!parsed.bool_key("production_slo_certified").unwrap());
    assert!(!parsed.bool_key("pitr_ready").unwrap());
    assert!(!parsed.bool_key("offset_min_used").unwrap());
    assert_eq!(parsed.string("w04_gate").unwrap(), "open");
    assert_eq!(parsed.string("offset_published").unwrap(), "100000");
    assert!(parsed.bool_key("replay_ready").unwrap());
    assert!(parsed.bool_key("cut_proven").unwrap());
    assert!(!parsed.bool_key("history_insufficient").unwrap());
    assert!(parsed.bool_key("wal_moved").unwrap());
    assert_eq!(decide::REMAINING.len(), 15);
    let refused = decide::report_json(&Report::refused("manifest_absent"));
    let refused_value = json::parse(&refused).unwrap();
    assert!(!refused_value.bool_key("replay_ready").unwrap());
    assert!(!refused_value.bool_key("cut_proven").unwrap());
    assert!(refused_value.bool_key("history_insufficient").unwrap());
    assert_eq!(refused_value.string("reason").unwrap(), "manifest_absent");
    assert_eq!(refused_value.get("offset_published"), Some(&json::Value::Null));
}

#[test]
fn an_unproven_cut_does_not_publish_readiness_or_an_offset() {
    let mut report = Report {
        replay_ready: true,
        reason: String::new(),
        reasons: Vec::new(),
        history_insufficient: false,
        wal_moved: false,
        checkpoint_copy_ok: true,
        shop_gold_ok: true,
        counts_ok: true,
        cut_proven: false,
        decimal_ok: true,
        null_ok: true,
        comment_ok: true,
        key_reused: false,
        key_count: 2,
        post_cut_absent_on_restore: true,
        post_cut_present_on_source: true,
        clock_anomaly: false,
        offset_published: Some("100000".to_string()),
        rpo_window_seconds: Some(1),
        rto_seconds: Some(1),
    };
    report.seal();
    assert!(!report.replay_ready);
    assert!(report.history_insufficient);
    assert_eq!(report.offset_published, None);
    let parsed = json::parse(&decide::report_json(&report)).unwrap();
    assert!(!parsed.bool_key("replay_ready").unwrap());
    assert!(parsed.bool_key("history_insufficient").unwrap());
    assert_eq!(parsed.get("offset_published"), Some(&json::Value::Null));
}

#[test]
fn manifest_round_trips_and_rejects_a_bad_quiescent_flag() {
    let manifest = sample_manifest();
    let body = manifest::to_json(&manifest);
    let parsed = manifest::parse(&body).unwrap();
    assert_eq!(parsed.entry_count_before, 90000);
    assert_eq!(parsed.entry_count_after, 90000);
    assert!(!parsed.history_insufficient);
    assert!(parsed.wal_moved);
    assert_eq!(parsed.dumps.len(), 1);
    assert_eq!(parsed.shop_gold_sha256, manifest.shop_gold_sha256);
    let broken = body.replace("\"quiescent\": true", "\"quiescent\": false");
    assert!(manifest::parse(&broken).is_err());
    let forged = body.replace("\"cut_mode\": \"writers_blocked\"", "\"cut_mode\": \"unproven\"");
    assert!(manifest::parse(&forged).is_err());
}

#[test]
fn relation_digest_must_match_the_bound_gold_hash() {
    let manifest = sample_manifest();
    let pairs: Vec<(&str, &str)> = manifest.gold.iter().map(|item| (item.name.as_str(), item.sha256.as_str())).collect();
    assert!(cut::digests_bound(&manifest.relation_digests, &pairs).is_ok());
    let mut broken = manifest.relation_digests.clone();
    broken[0].gold_sha256 = sha256::hex(&hash(b"other"));
    let err = cut::digests_bound(&broken, &pairs).unwrap_err();
    assert_eq!(err.detail, "gold_hash_mismatch");
    assert_eq!(err.exit, 4);
}

#[test]
fn sealed_manifest_refuses_a_changed_sidecar() {
    let root = scratch("seal");
    let backup = root.join("backup");
    fs::create_dir_all(&backup).unwrap();
    let operator = Operator {
        tool_dir: PathBuf::from("/usr/lib/postgresql/17/bin"),
        password_file: root.join("admin.pass"),
        schema_root: root.clone(),
        backup_dir: backup.clone(),
        report_path: root.join("readiness.json"),
        work_dir: root.join("work"),
    };
    manifest::seal(&operator, &sample_manifest()).unwrap();
    assert!(manifest::read_sealed(&backup).is_ok());
    let side = backup.join("manifest.sha256");
    chmod(&side, 0o644);
    fs::write(&side, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n").unwrap();
    let err = manifest::read_sealed(&backup).unwrap_err();
    assert_eq!(err.exit, 4);
    assert_eq!(err.detail, "manifest_hash_mismatch");
    fs::remove_file(backup.join("manifest.json")).unwrap();
    fs::remove_file(&side).unwrap();
    let missing = manifest::read_sealed(&backup).unwrap_err();
    assert_eq!(missing.detail, "manifest_absent");
}

#[test]
fn operator_file_rejects_unknown_keys_and_host_package_paths() {
    let root = scratch("operator");
    let path = root.join("operator.json");
    fs::write(&path, frozen_operator("/usr/lib/postgresql/17/bin")).unwrap();
    let loaded = load_operator(&path).unwrap();
    assert_eq!(loaded.tool_dir, PathBuf::from("/usr/lib/postgresql/17/bin"));
    fs::write(&path, frozen_operator("/opt/homebrew/opt/libpq/bin")).unwrap();
    let err = load_operator(&path).unwrap_err();
    assert_eq!(err.exit, 2);
    let mut extra = frozen_operator("/usr/lib/postgresql/17/bin");
    extra = extra.replacen("{\n", "{\n  \"extra\": \"no\",\n", 1);
    fs::write(&path, extra).unwrap();
    assert_eq!(load_operator(&path).unwrap_err().exit, 2);
    let caller_cut = frozen_operator("/usr/lib/postgresql/17/bin").replacen("{\n", "{\n  \"quiescent\": true,\n", 1);
    fs::write(&path, caller_cut).unwrap();
    assert_eq!(load_operator(&path).unwrap_err().exit, 2);
    assert!(schema_path(&root, "..").is_err());
    assert!(schema_path(&root, "a//b").is_err());
}

#[test]
fn password_file_must_be_a_private_regular_file() {
    let root = scratch("secret");
    let path = root.join("admin.pass");
    fs::write(&path, "abcdef0123456789").unwrap();
    chmod(&path, 0o644);
    assert_eq!(read_secret(&path).unwrap_err().exit, 2);
    chmod(&path, 0o600);
    assert!(read_secret(&path).is_ok());
    fs::write(&path, "bad:pass").unwrap();
    chmod(&path, 0o600);
    assert_eq!(read_secret(&path).unwrap_err().exit, 2);
}

#[test]
fn checkpoint_sql_mentions_the_frozen_rows() {
    let digest = sha256::hex(&hash(b"schema-file"));
    let control = control_sql(&digest, &digest, &digest).unwrap();
    assert!(control.contains("cut-a356-1"));
    assert!(control.contains("txn-a356-1"));
    assert!(control.contains("k-0001"));
    assert!(control.contains("k-0002"));
    assert!(control.contains("2026-03-31"));
    assert!(!control.contains("refused_column"));
    let enumerated = enum_sql(&digest).unwrap();
    assert!(enumerated.contains("'confirmed'"));
    assert!(enumerated.contains("'approved'"));
    assert_eq!(fixture::ROLES.len(), 7);
    assert_eq!(protocol::SCHEMAS.len(), 4);
    assert_eq!(protocol::copies().len(), 15);
}

#[test]
fn hash_tree_lists_regular_files_and_skips_its_output() {
    let root = scratch("tree");
    fs::write(root.join("a.txt"), b"hello").unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub").join("b.txt"), b"world").unwrap();
    let sums = root.join("SHA256SUMS");
    ops::hash_tree(&root, &sums).unwrap();
    let body = fs::read_to_string(&sums).unwrap();
    assert!(body.contains(&format!("{}  a.txt\n", sha256::hex(&hash(b"hello")))));
    assert!(body.contains(&format!("{}  sub/b.txt\n", sha256::hex(&hash(b"world")))));
    assert!(!body.contains("SHA256SUMS"));
    let side = fs::read_to_string(root.join("SHA256SUMS.sha256")).unwrap();
    assert_eq!(side.trim(), sha256::hex(&hash(body.as_bytes())));
}

fn sample_manifest() -> Manifest {
    let mut manifest = Manifest {
        pg_dump_version: "pg_dump (PostgreSQL) 17.10".to_string(),
        pg_restore_version: "pg_restore (PostgreSQL) 17.10".to_string(),
        psql_version: "psql (PostgreSQL) 17.10".to_string(),
        tool_dir: "/usr/lib/postgresql/17/bin".to_string(),
        cut_mode: cut::WRITERS_BLOCKED.to_string(),
        history_insufficient: false,
        wal_moved: true,
        lsn_before: "0/100".to_string(),
        lsn_after: "0/200".to_string(),
        entry_count_before: 90000,
        entry_count_after: 90000,
        last_write_unix: 10,
        backup_started_unix: 11,
        backup_finished_unix: 12,
        dumps: vec![NamedHash {
            name: "oo_a356_agent.dump".to_string(),
            sha256: sha256::hex(&hash(b"dump")),
        }],
        gold: Vec::new(),
        schemas: Vec::new(),
        relation_digests: Vec::new(),
        writer_blocks: Vec::new(),
        shop_gold_sha256: sha256::hex(&hash(b"shop")),
    };
    fill_proof(&mut manifest);
    manifest
}

fn fill_proof(manifest: &mut Manifest) {
    let mut digests = Vec::new();
    let mut gold = Vec::new();
    let mut control_relations = Vec::new();
    let mut agent_relations = Vec::new();
    for (database, relation, gold_file) in cut::sealed_pairs() {
        let sha = sha256::hex(&hash(relation.as_bytes()));
        digests.push(RelationDigest {
            content_md5: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
            database: database.to_string(),
            gold_sha256: sha.clone(),
            relation: relation.to_string(),
        });
        gold.push(NamedHash { name: gold_file.to_string(), sha256: sha });
        if database == protocol::CONTROL_DB {
            control_relations.push(relation.to_string());
        } else {
            agent_relations.push(relation.to_string());
        }
    }
    control_relations.sort();
    agent_relations.sort();
    digests.sort_by(|left, right| left.database.cmp(&right.database).then(left.relation.cmp(&right.relation)));
    gold.sort_by(|left, right| left.name.cmp(&right.name));
    manifest.relation_digests = digests;
    manifest.gold = gold;
    manifest.writer_blocks = vec![proof_block(protocol::CONTROL_DB, control_relations), proof_block(protocol::AGENT_DB, agent_relations)];
}

fn proof_block(database: &str, relations: Vec<String>) -> WriterBlock {
    WriterBlock {
        application_name: cut::application_name(database).unwrap().to_string(),
        database: database.to_string(),
        lock_mode: "SHARE".to_string(),
        probe_rejected: true,
        relations,
    }
}

fn frozen_operator(tool_dir: &str) -> String {
    format!(
        "{{\n  \"backup_dir\": \"/var/lib/postgresql/a356-backup\",\n  \"caller\": \"trusted_local_admin\",\n  \"host\": \"/var/run/postgresql\",\n  \"lineage_version\": \"pg-backup-restore/v1\",\n  \"maintenance_database\": \"postgres\",\n  \"owner\": \"a356\",\n  \"password_file\": \"/var/lib/postgresql/a356-admin.pass\",\n  \"port\": \"5432\",\n  \"report_path\": \"/var/lib/postgresql/a356-work/readiness.json\",\n  \"restore_databases\": [\"oo_a356_restore_control\", \"oo_a356_restore_agent\"],\n  \"schema_root\": \"/schemas\",\n  \"schemas\": [\n    {{\"database\": \"oo_a356_control\", \"file\": \"tools/cdc-offset-store/migrations/oo_cdc_offset_control_v1.sql\", \"id\": \"cdc-offset-store-v1\"}},\n    {{\"database\": \"oo_a356_control\", \"file\": \"tools/session-store/migrations/oo_session_control_v1.sql\", \"id\": \"session-store-v1\"}},\n    {{\"database\": \"oo_a356_control\", \"file\": \"tools/calendar-store/migrations/oo_calendar_control_v1.sql\", \"id\": \"calendar-store-v1\"}},\n    {{\"database\": \"oo_a356_agent\", \"file\": \"tools/enum-store/migrations/oo_enum_control_v1.sql\", \"id\": \"enum-store-v1\"}}\n  ],\n  \"session\": \"eef831aa-dba4-4af2-a78b-541bc9e0a356\",\n  \"source_cut\": \"cut-a356-1\",\n  \"source_databases\": [\"oo_a356_control\", \"oo_a356_agent\"],\n  \"target_fence\": \"fence-a356-restore\",\n  \"tool_dir\": \"{tool_dir}\",\n  \"user\": \"oo_a356_admin\",\n  \"work_dir\": \"/var/lib/postgresql/a356-work\"\n}}\n"
    )
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("oo-a356-{name}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn chmod(path: &Path, mode: u32) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions).unwrap();
}
